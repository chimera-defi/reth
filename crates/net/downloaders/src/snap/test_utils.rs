//! Test utilities for snap sync downloader.

use crate::snap::{downloader::SnapSyncConfig, task::TaskSnapDownloader};
use alloy_primitives::B256;
use reth_eth_wire_types::snap::{
    AccountRangeMessage, ByteCodesMessage, GetAccountRangeMessage, GetByteCodesMessage,
    GetStorageRangesMessage, GetTrieNodesMessage, StorageRangesMessage, TrieNodesMessage,
};
use reth_network_p2p::snap::client::SnapClient;
use reth_network_p2p::error::PeerRequestResult;
use std::sync::Arc;

/// Test snap client for testing purposes
#[derive(Debug, Default)]
pub struct TestSnapClient {
    /// Mock responses for account ranges
    pub account_range_responses: Vec<AccountRangeMessage>,
    /// Mock responses for storage ranges
    pub storage_range_responses: Vec<StorageRangesMessage>,
    /// Mock responses for byte codes
    pub byte_code_responses: Vec<ByteCodesMessage>,
    /// Mock responses for trie nodes
    pub trie_node_responses: Vec<TrieNodesMessage>,
}

impl TestSnapClient {
    /// Create a new test snap client
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a mock account range response
    pub fn add_account_range_response(mut self, response: AccountRangeMessage) -> Self {
        self.account_range_responses.push(response);
        self
    }

    /// Add a mock storage range response
    pub fn add_storage_range_response(mut self, response: StorageRangesMessage) -> Self {
        self.storage_range_responses.push(response);
        self
    }

    /// Add a mock byte code response
    pub fn add_byte_code_response(mut self, response: ByteCodesMessage) -> Self {
        self.byte_code_responses.push(response);
        self
    }

    /// Add a mock trie node response
    pub fn add_trie_node_response(mut self, response: TrieNodesMessage) -> Self {
        self.trie_node_responses.push(response);
        self
    }
}

impl SnapClient for TestSnapClient {
    type Output = std::pin::Pin<Box<dyn std::future::Future<Output = PeerRequestResult<AccountRangeMessage>> + Send + Sync>>;

    fn get_account_range_with_priority(
        &self,
        _request: GetAccountRangeMessage,
        _priority: reth_network_p2p::priority::Priority,
    ) -> Self::Output {
        let response = self.account_range_responses.first().cloned()
            .unwrap_or_else(|| AccountRangeMessage {
                request_id: 0,
                accounts: vec![],
                proof: vec![],
            });
        
        Box::pin(async move { Ok(response) })
    }

    fn get_storage_ranges_with_priority(
        &self,
        _request: GetStorageRangesMessage,
        _priority: reth_network_p2p::priority::Priority,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = PeerRequestResult<StorageRangesMessage>> + Send + Sync>> {
        let response = self.storage_range_responses.first().cloned()
            .unwrap_or_else(|| StorageRangesMessage {
                request_id: 0,
                slots: vec![],
                proof: vec![],
            });
        
        Box::pin(async move { Ok(response) })
    }

    fn get_byte_codes_with_priority(
        &self,
        _request: GetByteCodesMessage,
        _priority: reth_network_p2p::priority::Priority,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = PeerRequestResult<ByteCodesMessage>> + Send + Sync>> {
        let response = self.byte_code_responses.first().cloned()
            .unwrap_or_else(|| ByteCodesMessage {
                request_id: 0,
                codes: vec![],
            });
        
        Box::pin(async move { Ok(response) })
    }

    fn get_trie_nodes_with_priority(
        &self,
        _request: GetTrieNodesMessage,
        _priority: reth_network_p2p::priority::Priority,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = PeerRequestResult<TrieNodesMessage>> + Send + Sync>> {
        let response = self.trie_node_responses.first().cloned()
            .unwrap_or_else(|| TrieNodesMessage {
                request_id: 0,
                nodes: vec![],
            });
        
        Box::pin(async move { Ok(response) })
    }
}

/// Create a test snap sync downloader with default configuration
pub fn create_test_snap_sync_downloader<C: SnapClient + 'static>(
    client: Arc<C>,
) -> TaskSnapDownloader<C> {
    TaskSnapDownloader::new(client)
}

/// Create default snap sync configuration for testing
pub fn create_test_snap_sync_config() -> SnapSyncConfig {
    SnapSyncConfig {
        max_concurrent_requests: 2,
        max_response_bytes: 1024 * 1024, // 1MB
        max_accounts_per_request: 100,
        max_storage_slots_per_request: 100,
        max_byte_codes_per_request: 10,
        max_trie_nodes_per_request: 100,
    }
}