//! Task-based snap sync downloader implementation.

use crate::snap::queue::SnapRequestQueue;
use alloy_primitives::B256;
use reth_eth_wire_types::snap::{
    AccountRangeMessage, ByteCodesMessage, GetAccountRangeMessage, GetByteCodesMessage,
    GetStorageRangesMessage, GetTrieNodesMessage, StorageRangesMessage, TrieNodesMessage,
};
use reth_network_p2p::snap::client::SnapClient;
use reth_network_p2p::error::PeerRequestResult;
use std::sync::Arc;

/// Task-based snap sync downloader that handles individual snap sync requests
#[derive(Debug)]
pub struct TaskSnapDownloader<C: SnapClient> {
    /// The snap client for making requests
    client: Arc<C>,
    /// Request queue for managing pending requests
    request_queue: SnapRequestQueue,
}

impl<C: SnapClient> TaskSnapDownloader<C> {
    /// Create a new task-based snap downloader
    pub fn new(client: Arc<C>) -> Self {
        Self {
            client,
            request_queue: SnapRequestQueue::new(),
        }
    }

    /// Download account range
    pub async fn download_account_range(
        &mut self,
        root_hash: B256,
        starting_hash: B256,
        limit_hash: B256,
        response_bytes: u64,
    ) -> PeerRequestResult<AccountRangeMessage> {
        let request_id = self.request_queue.next_request_id();
        let request = GetAccountRangeMessage {
            request_id,
            root_hash,
            starting_hash,
            limit_hash,
            response_bytes,
        };

        self.client.get_account_range(request).await
    }

    /// Download storage ranges
    pub async fn download_storage_ranges(
        &mut self,
        root_hash: B256,
        account_hashes: Vec<B256>,
        starting_hash: B256,
        limit_hash: B256,
        response_bytes: u64,
    ) -> PeerRequestResult<StorageRangesMessage> {
        let request_id = self.request_queue.next_request_id();
        let request = GetStorageRangesMessage {
            request_id,
            root_hash,
            account_hashes,
            starting_hash,
            limit_hash,
            response_bytes,
        };

        self.client.get_storage_ranges(request).await
    }

    /// Download byte codes
    pub async fn download_byte_codes(
        &mut self,
        hashes: Vec<B256>,
        response_bytes: u64,
    ) -> PeerRequestResult<ByteCodesMessage> {
        let request_id = self.request_queue.next_request_id();
        let request = GetByteCodesMessage {
            request_id,
            hashes,
            response_bytes,
        };

        self.client.get_byte_codes(request).await
    }

    /// Download trie nodes
    pub async fn download_trie_nodes(
        &mut self,
        root_hash: B256,
        paths: Vec<reth_eth_wire_types::snap::TriePath>,
        response_bytes: u64,
    ) -> PeerRequestResult<TrieNodesMessage> {
        let request_id = self.request_queue.next_request_id();
        let request = GetTrieNodesMessage {
            request_id,
            root_hash,
            paths,
            response_bytes,
        };

        self.client.get_trie_nodes(request).await
    }

    /// Get the request queue for managing pending requests
    pub fn request_queue(&mut self) -> &mut SnapRequestQueue {
        &mut self.request_queue
    }
}