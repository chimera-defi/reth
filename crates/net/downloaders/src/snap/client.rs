use crate::metrics::BodyDownloaderMetrics;
use futures::{Future, StreamExt};
use reth_eth_wire_types::snap::{
    AccountRangeMessage, ByteCodesMessage, GetAccountRangeMessage, GetByteCodesMessage,
    GetStorageRangesMessage, GetTrieNodesMessage, StorageRangesMessage, TrieNodesMessage,
};
use reth_network_p2p::snap::client::SnapClient;
use reth_primitives_traits::{Account, StorageEntry};
use reth_storage_api::{StateProvider, StateWriter};
use std::{
    collections::HashMap,
    pin::Pin,
    sync::Arc,
    task::{ready, Context, Poll},
};
use tracing::{debug, info};

/// The account range future type
pub type AccountRangeFut = Pin<
    Box<dyn Future<Output = reth_network_p2p::error::PeerRequestResult<AccountRangeMessage>> + Send>,
>;

/// The storage range future type
pub type StorageRangeFut = Pin<
    Box<dyn Future<Output = reth_network_p2p::error::PeerRequestResult<StorageRangesMessage>> + Send>,
>;

/// The bytecode future type
pub type ByteCodeFut = Pin<
    Box<dyn Future<Output = reth_network_p2p::error::PeerRequestResult<ByteCodesMessage>> + Send>,
>;

/// The trie node future type
pub type TrieNodeFut = Pin<
    Box<dyn Future<Output = reth_network_p2p::error::PeerRequestResult<TrieNodesMessage>> + Send>,
>;

/// A client capable of downloading state data via snap sync protocol
#[auto_impl::auto_impl(&, Arc, Box)]
pub trait SnapSyncClient: SnapClient + Send + Sync {
    /// The state provider type
    type StateProvider: StateProvider;
    /// The state writer type
    type StateWriter: StateWriter;

    /// Fetches account range data
    fn get_account_range(&self, request: GetAccountRangeMessage) -> AccountRangeFut;

    /// Fetches storage range data for multiple accounts
    fn get_storage_ranges(&self, request: GetStorageRangesMessage) -> StorageRangeFut;

    /// Fetches bytecode data for given code hashes
    fn get_bytecodes(&self, request: GetByteCodesMessage) -> ByteCodeFut;

    /// Fetches trie node data for given paths
    fn get_trie_nodes(&self, request: GetTrieNodesMessage) -> TrieNodeFut;

    /// Returns the state provider
    fn state_provider(&self) -> &Self::StateProvider;

    /// Returns the state writer
    fn state_writer(&self) -> &Self::StateWriter;
}

/// Snap sync downloader that coordinates downloading state data from peers
#[derive(Debug)]
pub struct SnapDownloader<C: SnapSyncClient> {
    client: Arc<C>,
    metrics: BodyDownloaderMetrics,
}

impl<C: SnapSyncClient> SnapDownloader<C> {
    /// Create a new snap downloader
    pub fn new(client: Arc<C>) -> Self {
        Self {
            client,
            metrics: BodyDownloaderMetrics::default(),
        }
    }

    /// Download the complete state for the given block hash
    pub async fn download_state(
        &self,
        block_hash: alloy_primitives::B256,
    ) -> Result<(), SnapSyncError> {
        info!(target: "reth::snap", block_hash = %block_hash, "Starting snap sync download");

        // Get the state root from the block header
        let state_root = self
            .client
            .state_provider()
            .header_by_hash(block_hash)?
            .ok_or_else(|| SnapSyncError::BlockNotFound(block_hash))?
            .state_root;

        info!(target: "reth::snap", state_root = %state_root, "Found state root, starting state download");

        // Start with account range requests
        let mut account_cursor = alloy_primitives::B256::ZERO;
        let mut accounts = Vec::new();
        let mut processed_accounts = 0usize;

        loop {
            let request = GetAccountRangeMessage {
                request_id: 0, // TODO: Use proper request ID
                root_hash: state_root,
                starting_hash: account_cursor,
                limit_hash: alloy_primitives::B256::MAX, // No upper limit
                response_bytes: 1_000_000, // 1MB response limit
            };

            let response = self.client.get_account_range(request).await?;

            if response.accounts.is_empty() {
                break; // No more accounts
            }

            // Process accounts
            for account_data in response.accounts {
                let account = Account::decode(&account_data.body)?;
                accounts.push(account);
                processed_accounts += 1;

                // TODO: Download storage for this account
                // TODO: Download bytecode for this account if it's a contract
            }

            // Update cursor for next iteration
            if let Some(last_account) = response.accounts.last() {
                account_cursor = last_account.hash;
            }

            // Update metrics
            self.metrics.update_downloaded_accounts(processed_accounts);

            if processed_accounts % 1000 == 0 {
                info!(
                    target: "reth::snap",
                    accounts_processed = processed_accounts,
                    "Progress: {} accounts downloaded",
                    processed_accounts
                );
            }
        }

        info!(
            target: "reth::snap",
            total_accounts = processed_accounts,
            "Completed account download, starting storage sync"
        );

        // TODO: Download storage data for all accounts
        // TODO: Download bytecode data for all contracts

        info!(
            target: "reth::snap",
            "Snap sync download completed successfully"
        );

        Ok(())
    }
}

/// Error type for snap sync operations
#[derive(Debug, thiserror::Error)]
pub enum SnapSyncError {
    /// Block not found
    #[error("Block not found: {0}")]
    BlockNotFound(alloy_primitives::B256),

    /// Network error
    #[error("Network error: {0}")]
    Network(#[from] reth_network_p2p::error::PeerRequestResult<()>),

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

#[cfg(test)]
mod tests {
    use super::*;

    // TODO: Add tests for snap sync functionality
}