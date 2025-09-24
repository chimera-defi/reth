//! Request queue for snap sync downloads.

use alloy_primitives::B256;
use reth_eth_wire_types::snap::{
    GetAccountRangeMessage, GetByteCodesMessage, GetStorageRangesMessage, GetTrieNodesMessage,
};
use std::collections::VecDeque;

/// Types of snap sync requests
#[derive(Debug, Clone)]
pub enum SnapRequest {
    /// Account range request
    AccountRange(GetAccountRangeMessage),
    /// Storage ranges request
    StorageRanges(GetStorageRangesMessage),
    /// Byte codes request
    ByteCodes(GetByteCodesMessage),
    /// Trie nodes request
    TrieNodes(GetTrieNodesMessage),
}

/// Request queue for managing snap sync requests
#[derive(Debug, Default)]
pub struct SnapRequestQueue {
    /// Pending account range requests
    account_range_requests: VecDeque<GetAccountRangeMessage>,
    /// Pending storage range requests
    storage_range_requests: VecDeque<GetStorageRangesMessage>,
    /// Pending byte code requests
    byte_code_requests: VecDeque<GetByteCodesMessage>,
    /// Pending trie node requests
    trie_node_requests: VecDeque<GetTrieNodesMessage>,
    /// Next request ID to use
    next_request_id: u64,
}

impl SnapRequestQueue {
    /// Create a new empty request queue
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the next request ID and increment the counter
    pub fn next_request_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id += 1;
        id
    }

    /// Add an account range request to the queue
    pub fn push_account_range(&mut self, request: GetAccountRangeMessage) {
        self.account_range_requests.push_back(request);
    }

    /// Add a storage range request to the queue
    pub fn push_storage_range(&mut self, request: GetStorageRangesMessage) {
        self.storage_range_requests.push_back(request);
    }

    /// Add a byte code request to the queue
    pub fn push_byte_codes(&mut self, request: GetByteCodesMessage) {
        self.byte_code_requests.push_back(request);
    }

    /// Add a trie node request to the queue
    pub fn push_trie_nodes(&mut self, request: GetTrieNodesMessage) {
        self.trie_node_requests.push_back(request);
    }

    /// Pop the next account range request
    pub fn pop_account_range(&mut self) -> Option<GetAccountRangeMessage> {
        self.account_range_requests.pop_front()
    }

    /// Pop the next storage range request
    pub fn pop_storage_range(&mut self) -> Option<GetStorageRangesMessage> {
        self.storage_range_requests.pop_front()
    }

    /// Pop the next byte code request
    pub fn pop_byte_codes(&mut self) -> Option<GetByteCodesMessage> {
        self.byte_code_requests.pop_front()
    }

    /// Pop the next trie node request
    pub fn pop_trie_nodes(&mut self) -> Option<GetTrieNodesMessage> {
        self.trie_node_requests.pop_front()
    }

    /// Check if there are any pending requests
    pub fn has_pending_requests(&self) -> bool {
        !self.account_range_requests.is_empty()
            || !self.storage_range_requests.is_empty()
            || !self.byte_code_requests.is_empty()
            || !self.trie_node_requests.is_empty()
    }

    /// Get the total number of pending requests
    pub fn pending_count(&self) -> usize {
        self.account_range_requests.len()
            + self.storage_range_requests.len()
            + self.byte_code_requests.len()
            + self.trie_node_requests.len()
    }
}