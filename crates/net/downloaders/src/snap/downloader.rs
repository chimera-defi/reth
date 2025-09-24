//! Snap sync downloader implementation.

use crate::snap::{queue::SnapRequestQueue, task::TaskSnapDownloader};
use alloy_primitives::{B256, Bytes};
use futures::Stream;
use reth_config::config::SnapSyncConfig;
use reth_eth_wire_types::snap::{
    AccountData, AccountRangeMessage, ByteCodesMessage, StorageData, StorageRangesMessage,
    TrieNodesMessage,
};
use reth_network_p2p::snap::client::SnapClient;
use reth_network_p2p::error::PeerRequestResult;
use reth_storage_api::HeaderProvider;
use std::{
    collections::BinaryHeap,
    fmt::Debug,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

/// Result of a snap sync download operation
#[derive(Debug, Clone)]
pub enum SnapSyncResult {
    /// Account range data
    AccountRange(AccountRangeMessage),
    /// Storage ranges data
    StorageRanges(StorageRangesMessage),
    /// Byte codes data
    ByteCodes(ByteCodesMessage),
    /// Trie nodes data
    TrieNodes(TrieNodesMessage),
}

/// Error type for snap sync operations
#[derive(thiserror::Error, Debug)]
pub enum SnapSyncError {
    /// Network error
    #[error("Network error: {0}")]
    Network(#[from] reth_network_p2p::error::PeerRequestError),
    /// Invalid state root
    #[error("Invalid state root: {0}")]
    InvalidStateRoot(B256),
    /// Missing account data
    #[error("Missing account data for hash: {0}")]
    MissingAccountData(B256),
    /// Missing storage data
    #[error("Missing storage data for account: {0}")]
    MissingStorageData(B256),
    /// Invalid proof
    #[error("Invalid proof")]
    InvalidProof,
}


/// Snap sync downloader that downloads Ethereum state snapshots
#[derive(Debug)]
pub struct SnapSyncDownloader<C: SnapClient, Provider: HeaderProvider> {
    /// The snap client for making requests
    client: Arc<C>,
    /// Database provider for reading headers
    provider: Provider,
    /// Configuration for the downloader
    config: SnapSyncConfig,
    /// Request queue for managing pending requests
    request_queue: SnapRequestQueue,
    /// Task downloader for handling individual requests
    task_downloader: TaskSnapDownloader<C>,
    /// Buffered results ready to be returned
    buffered_results: BinaryHeap<OrderedSnapResult>,
}

/// Ordered snap sync result for maintaining proper order
#[derive(Debug, Clone)]
struct OrderedSnapResult {
    /// The result data
    result: SnapSyncResult,
    /// Order index for maintaining sequence
    order_index: usize,
}

impl PartialEq for OrderedSnapResult {
    fn eq(&self, other: &Self) -> bool {
        self.order_index == other.order_index
    }
}

impl Eq for OrderedSnapResult {}

impl PartialOrd for OrderedSnapResult {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedSnapResult {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse order so that lower indices come first
        other.order_index.cmp(&self.order_index)
    }
}

impl<C: SnapClient, Provider: HeaderProvider> SnapSyncDownloader<C, Provider> {
    /// Create a new snap sync downloader
    pub fn new(client: Arc<C>, provider: Provider, config: SnapSyncConfig) -> Self {
        let task_downloader = TaskSnapDownloader::new(client.clone());
        Self {
            client,
            provider,
            config,
            request_queue: SnapRequestQueue::new(),
            task_downloader,
            buffered_results: BinaryHeap::new(),
        }
    }

    /// Start downloading account ranges for the given state root
    pub async fn start_account_range_download(
        &mut self,
        state_root: B256,
    ) -> Result<(), SnapSyncError> {
        // For now, we'll start with a simple range from zero to max
        // In a full implementation, this would be more sophisticated
        let starting_hash = B256::ZERO;
        let limit_hash = B256::MAX;
        
        let request_id = self.request_queue.next_request_id();
        let request = reth_eth_wire_types::snap::GetAccountRangeMessage {
            request_id,
            root_hash: state_root,
            starting_hash,
            limit_hash,
            response_bytes: self.config.max_response_bytes,
        };

        self.request_queue.push_account_range(request);
        Ok(())
    }

    /// Process the next pending request
    async fn process_next_request(&mut self) -> Result<Option<SnapSyncResult>, SnapSyncError> {
        // Try to process account range requests first
        if let Some(request) = self.request_queue.pop_account_range() {
            let result = self.client.get_account_range(request).await?;
            return Ok(Some(SnapSyncResult::AccountRange(result)));
        }

        // Try to process storage range requests
        if let Some(request) = self.request_queue.pop_storage_range() {
            let result = self.client.get_storage_ranges(request).await?;
            return Ok(Some(SnapSyncResult::StorageRanges(result)));
        }

        // Try to process byte code requests
        if let Some(request) = self.request_queue.pop_byte_codes() {
            let result = self.client.get_byte_codes(request).await?;
            return Ok(Some(SnapSyncResult::ByteCodes(result)));
        }

        // Try to process trie node requests
        if let Some(request) = self.request_queue.pop_trie_nodes() {
            let result = self.client.get_trie_nodes(request).await?;
            return Ok(Some(SnapSyncResult::TrieNodes(result)));
        }

        Ok(None)
    }

    /// Get the current state root from the latest header
    pub fn get_current_state_root(&self) -> Result<B256, SnapSyncError> {
        // This would typically get the state root from the latest header
        // For now, we'll return a placeholder
        Ok(B256::ZERO)
    }
}

impl<C: SnapClient, Provider: HeaderProvider> Stream for SnapSyncDownloader<C, Provider> {
    type Item = Result<SnapSyncResult, SnapSyncError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // First, try to return any buffered results
        if let Some(result) = self.buffered_results.pop() {
            return Poll::Ready(Some(Ok(result.result)));
        }

        // If we have pending requests, we need to process them
        // For now, we'll simulate processing by returning a placeholder result
        // In a real implementation, this would involve async network requests
        if self.request_queue.has_pending_requests() {
            // Simulate processing a request and returning a result
            // This prevents the busy-wait loop
            if let Some(_request) = self.request_queue.pop_account_range() {
                // In a real implementation, we would make the network request here
                // For now, we return a placeholder result to prevent infinite polling
                let placeholder_result = SnapSyncResult::AccountRange(
                    reth_eth_wire_types::snap::AccountRangeMessage {
                        request_id: 1,
                        accounts: vec![],
                        proof: vec![],
                    }
                );
                return Poll::Ready(Some(Ok(placeholder_result)));
            }
        }

        // No more work to do
        Poll::Ready(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snap::test_utils::TestSnapClient;

    #[test]
    fn test_snap_sync_config_default() {
        let config = SnapSyncConfig::default();
        assert_eq!(config.max_concurrent_requests, 10);
        assert_eq!(config.max_response_bytes, 2 * 1024 * 1024);
    }

    #[test]
    fn test_snap_request_queue() {
        let mut queue = SnapRequestQueue::new();
        assert_eq!(queue.pending_count(), 0);
        assert!(!queue.has_pending_requests());

        let request_id = queue.next_request_id();
        assert_eq!(request_id, 0);
        assert_eq!(queue.next_request_id(), 1);
    }
}