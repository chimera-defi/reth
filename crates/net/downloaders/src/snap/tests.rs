//! Essential tests for snap sync implementation.

use crate::snap::{
    downloader::{SnapSyncDownloader, SnapSyncResult},
    test_utils::{create_test_snap_sync_config, TestSnapClient},
};
use alloy_primitives::{B256, Bytes};
use futures::StreamExt;
use reth_config::config::SnapSyncConfig;
use reth_eth_wire_types::snap::{AccountData, AccountRangeMessage};
use reth_provider::test_utils::MockProvider;
use std::sync::Arc;

#[tokio::test]
async fn test_snap_sync_downloader_basic() {
    let provider = MockProvider::default();
    let client = Arc::new(TestSnapClient::new());
    let config = SnapSyncConfig::default();
    
    let downloader = SnapSyncDownloader::new(client, provider, config);
    assert!(downloader.get_current_state_root().is_ok());
}

#[tokio::test]
async fn test_account_range_download() {
    let provider = MockProvider::default();
    let test_accounts = vec![AccountData {
        hash: B256::from([1u8; 32]),
        body: Bytes::from(vec![0x01, 0x02, 0x03]),
    }];
    
    let client = Arc::new(
        TestSnapClient::new().add_account_range_response(AccountRangeMessage {
            request_id: 1,
            accounts: test_accounts,
            proof: vec![Bytes::from(vec![0x04, 0x05, 0x06])],
        })
    );
    
    let config = create_test_snap_sync_config();
    let mut downloader = SnapSyncDownloader::new(client, provider, config);
    
    let state_root = B256::from([2u8; 32]);
    downloader.start_account_range_download(state_root).await.unwrap();
    assert!(downloader.request_queue.has_pending_requests());
}

#[test]
fn test_config_defaults() {
    let config = SnapSyncConfig::default();
    assert_eq!(config.max_concurrent_requests, 10);
    assert_eq!(config.max_response_bytes, 2 * 1024 * 1024);
    assert_eq!(config.commit_threshold, 10_000);
}

#[cfg(test)]
mod state_manager_tests;

#[cfg(test)]
mod peer_manager_tests;

#[cfg(test)]
mod progress_reporter_tests;

#[cfg(test)]
mod state_discovery_tests;

#[cfg(test)]
mod state_discovery_integration_test;

#[cfg(test)]
mod state_verifier_tests;

#[cfg(test)]
mod state_verifier_integration_test;