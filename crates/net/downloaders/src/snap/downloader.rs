use crate::{
    metrics::BodyDownloaderMetrics,
    snap::client::{AccountRangeFut, ByteCodeFut, SnapSyncClient, StorageRangeFut, TrieNodeFut},
};
use alloy_primitives::B256;
use futures::{Future, Stream, StreamExt};
use reth_eth_wire_types::snap::{
    AccountRangeMessage, ByteCodesMessage, GetAccountRangeMessage, GetByteCodesMessage,
    GetStorageRangesMessage, GetTrieNodesMessage, StorageRangesMessage, TrieNodesMessage,
};
use reth_network_p2p::{error::PeerRequestResult, snap::client::SnapClient};
use reth_primitives_traits::{Account, StorageEntry};
use reth_storage_api::{StateProvider, StateWriter};
use std::{
    collections::HashMap,
    pin::Pin,
    sync::Arc,
    task::{ready, Context, Poll},
};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Configuration for snap sync downloader
#[derive(Debug, Clone)]
pub struct SnapSyncConfig {
    /// Maximum number of concurrent requests
    pub max_concurrent_requests: usize,
    /// Maximum response size in bytes
    pub max_response_size: u64,
    /// Batch size for account requests
    pub account_batch_size: u64,
    /// Batch size for storage requests
    pub storage_batch_size: u64,
    /// Batch size for bytecode requests
    pub bytecode_batch_size: u64,
}

impl Default for SnapSyncConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: 10,
            max_response_size: 10_000_000, // 10MB
            account_batch_size: 1000,
            storage_batch_size: 1000,
            bytecode_batch_size: 100,
        }
    }
}

/// Main snap sync downloader that orchestrates the entire state download process
#[derive(Debug)]
pub struct SnapSyncDownloader<C: SnapSyncClient> {
    client: Arc<C>,
    config: SnapSyncConfig,
    metrics: BodyDownloaderMetrics,
}

impl<C: SnapSyncClient> SnapSyncDownloader<C> {
    /// Create a new snap sync downloader
    pub fn new(client: Arc<C>, config: SnapSyncConfig) -> Self {
        Self {
            client,
            config,
            metrics: BodyDownloaderMetrics::default(),
        }
    }

    /// Download the complete state for the given block hash
    pub async fn download_state(
        &self,
        block_hash: B256,
    ) -> Result<DownloadStats, SnapSyncError> {
        info!(target: "reth::snap", block_hash = %block_hash, "Starting snap sync download");

        // Get the state root from the block header
        let state_root = self
            .client
            .state_provider()
            .header_by_hash(block_hash)?
            .ok_or_else(|| SnapSyncError::BlockNotFound(block_hash))?
            .state_root;

        info!(target: "reth::snap", state_root = %state_root, "Found state root, starting state download");

        let mut stats = DownloadStats::default();

        // Phase 1: Download all accounts
        let accounts = self.download_accounts(state_root).await?;
        stats.total_accounts = accounts.len();
        info!(target: "reth::snap", accounts = accounts.len(), "Downloaded all accounts");

        // Phase 2: Download storage for all accounts
        let mut total_storage_slots = 0;
        for (i, account) in accounts.iter().enumerate() {
            if i % 100 == 0 {
                info!(
                    target: "reth::snap",
                    progress = i,
                    total = accounts.len(),
                    "Downloading storage: {} / {}",
                    i,
                    accounts.len()
                );
            }

            let storage_count = self.download_storage_for_account(account).await?;
            total_storage_slots += storage_count;
        }
        stats.total_storage_slots = total_storage_slots;
        info!(target: "reth::snap", storage_slots = total_storage_slots, "Downloaded all storage");

        // Phase 3: Download bytecode for contracts
        let bytecode_count = self.download_bytecodes(&accounts).await?;
        stats.total_bytecode = bytecode_count;
        info!(target: "reth::snap", bytecode = bytecode_count, "Downloaded all bytecode");

        info!(target: "reth::snap", "Snap sync download completed successfully");

        Ok(stats)
    }

    /// Download all accounts from the state trie
    async fn download_accounts(&self, state_root: B256) -> Result<Vec<Account>, SnapSyncError> {
        let mut accounts = Vec::new();
        let mut cursor = B256::ZERO;
        let mut request_id = 0u64;

        loop {
            let request = GetAccountRangeMessage {
                request_id: request_id,
                root_hash: state_root,
                starting_hash: cursor,
                limit_hash: B256::MAX,
                response_bytes: self.config.max_response_size,
            };

            let response = self.client.get_account_range(request).await?;

            if response.accounts.is_empty() {
                break; // No more accounts
            }

            // Process accounts
            for account_data in &response.accounts {
                let account = Account::decode(&account_data.body)?;
                accounts.push(account);
            }

            // Update cursor for next iteration
            if let Some(last_account) = response.accounts.last() {
                cursor = last_account.hash;
            }

            request_id += 1;
        }

        Ok(accounts)
    }

    /// Download storage for a specific account
    async fn download_storage_for_account(&self, account: &Account) -> Result<usize, SnapSyncError> {
        let mut storage_slots = 0;
        let mut cursor = B256::ZERO;
        let mut request_id = 0u64;

        loop {
            let request = GetStorageRangesMessage {
                request_id: request_id,
                root_hash: account.storage_root,
                account_hashes: vec![account.address_hash()], // TODO: This should be the account's address hash
                starting_hash: cursor,
                limit_hash: B256::MAX,
                response_bytes: self.config.max_response_size,
            };

            let response = self.client.get_storage_ranges(request).await?;

            if response.slots.is_empty() || response.slots[0].is_empty() {
                break; // No more storage slots
            }

            // Process storage slots
            for storage_data in &response.slots[0] {
                let entry = StorageEntry::decode(&storage_data.data)?;
                // TODO: Store the storage entry
                storage_slots += 1;
            }

            // Update cursor for next iteration
            if let Some(last_slot) = response.slots[0].last() {
                cursor = last_slot.hash;
            }

            request_id += 1;
        }

        Ok(storage_slots)
    }

    /// Download bytecode for all contracts
    async fn download_bytecodes(&self, accounts: &[Account]) -> Result<usize, SnapSyncError> {
        let contract_accounts: Vec<_> = accounts
            .iter()
            .filter(|acc| !acc.bytecode_hash.is_zero())
            .collect();

        if contract_accounts.is_empty() {
            return Ok(0);
        }

        let mut bytecode_count = 0;
        let mut request_id = 0u64;

        // Process in batches
        for chunk in contract_accounts.chunks(self.config.bytecode_batch_size as usize) {
            let hashes: Vec<B256> = chunk.iter().map(|acc| acc.bytecode_hash).collect();

            let request = GetByteCodesMessage {
                request_id: request_id,
                hashes,
                response_bytes: self.config.max_response_size,
            };

            let response = self.client.get_bytecodes(request).await?;

            // TODO: Store the bytecode data
            bytecode_count += response.codes.len();

            request_id += 1;
        }

        Ok(bytecode_count)
    }
}

/// Statistics from the snap sync download
#[derive(Debug, Default)]
pub struct DownloadStats {
    pub total_accounts: usize,
    pub total_storage_slots: usize,
    pub total_bytecode: usize,
}

/// Error type for snap sync operations
#[derive(Debug, thiserror::Error)]
pub enum SnapSyncError {
    /// Block not found
    #[error("Block not found: {0}")]
    BlockNotFound(B256),

    /// Network error
    #[error("Network error: {0}")]
    Network(String),

    /// State provider error
    #[error("State provider error: {0}")]
    StateProvider(#[from] reth_storage_api::StateProviderError),

    /// RLP decode error
    #[error("RLP decode error: {0}")]
    Rlp(#[from] alloy_rlp::Error),

    /// Other error
    #[error("Other error: {0}")]
    Other(#[from] eyre::Error),
}

impl From<PeerRequestResult<AccountRangeMessage>> for SnapSyncError {
    fn from(err: PeerRequestResult<AccountRangeMessage>) -> Self {
        Self::Network(format!("Account range request failed: {:?}", err))
    }
}

impl From<PeerRequestResult<StorageRangesMessage>> for SnapSyncError {
    fn from(err: PeerRequestResult<StorageRangesMessage>) -> Self {
        Self::Network(format!("Storage range request failed: {:?}", err))
    }
}

impl From<PeerRequestResult<ByteCodesMessage>> for SnapSyncError {
    fn from(err: PeerRequestResult<ByteCodesMessage>) -> Self {
        Self::Network(format!("Bytecode request failed: {:?}", err))
    }
}

impl From<PeerRequestResult<TrieNodesMessage>> for SnapSyncError {
    fn from(err: PeerRequestResult<TrieNodesMessage>) -> Self {
        Self::Network(format!("Trie node request failed: {:?}", err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TODO: Add comprehensive tests for snap sync functionality
}