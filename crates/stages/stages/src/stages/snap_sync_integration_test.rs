//! Integration tests for snap sync with header subscription.

use crate::stages::SnapSyncStage;
use crate::stages::header_subscription::HeaderSubscriptionService;
use reth_config::config::SnapSyncConfig;
use reth_engine_primitives::ConsensusEngineEvent;
use reth_ethereum_primitives::EthPrimitives;
use reth_network_p2p::snap::client::SnapClient;
use reth_eth_wire_types::snap::{AccountRangeMessage, GetAccountRangeMessage};
use reth_network_p2p::{priority::Priority, error::PeerRequestResult};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::time::{sleep, Duration};
use alloy_primitives::B256;
use alloy_consensus::BlockHeader;
use reth_primitives_traits::SealedHeader;

/// Mock snap client for testing
#[derive(Debug, Clone)]
pub struct MockSnapClient;

impl SnapClient for MockSnapClient {
    type Output = std::pin::Pin<Box<dyn std::future::Future<Output = PeerRequestResult<AccountRangeMessage>> + Send + Sync + Unpin>>;

    fn get_account_range_with_priority(
        &self,
        _request: GetAccountRangeMessage,
        _priority: Priority,
    ) -> Self::Output {
        Box::pin(async move {
            // Return a mock empty response
            Ok((B256::ZERO, AccountRangeMessage {
                request_id: 1,
                accounts: vec![],
                proof: vec![],
            }))
        })
    }

    fn get_storage_ranges(&self, _request: reth_eth_wire_types::snap::GetStorageRangesMessage) -> Self::Output {
        todo!()
    }

    fn get_storage_ranges_with_priority(
        &self,
        _request: reth_eth_wire_types::snap::GetStorageRangesMessage,
        _priority: Priority,
    ) -> Self::Output {
        todo!()
    }

    fn get_byte_codes(&self, _request: reth_eth_wire_types::snap::GetByteCodesMessage) -> Self::Output {
        todo!()
    }

    fn get_byte_codes_with_priority(
        &self,
        _request: reth_eth_wire_types::snap::GetByteCodesMessage,
        _priority: Priority,
    ) -> Self::Output {
        todo!()
    }

    fn get_trie_nodes(&self, _request: reth_eth_wire_types::snap::GetTrieNodesMessage) -> Self::Output {
        todo!()
    }

    fn get_trie_nodes_with_priority(
        &self,
        _request: reth_eth_wire_types::snap::GetTrieNodesMessage,
        _priority: Priority,
    ) -> Self::Output {
        todo!()
    }
}

#[tokio::test]
async fn test_snap_sync_with_header_subscription() {
    // Create a broadcast channel for consensus engine events
    let (event_sender, _event_receiver) = broadcast::channel(16);
    
    // Create snap sync config
    let config = SnapSyncConfig {
        enabled: true,
        max_ranges_per_execution: 10,
        max_response_bytes: 1000000,
        request_timeout_seconds: 30,
        range_size: 1000,
        max_retries: 3,
    };
    
    // Create snap client
    let snap_client = Arc::new(MockSnapClient);
    
    // Create snap sync stage with header subscription
    let (snap_sync_stage, mut header_receiver) = SnapSyncStage::with_header_subscription(
        config,
        snap_client,
        event_sender.clone(),
    );
    
    // Verify the stage was created with header receiver
    assert!(snap_sync_stage.header_receiver.is_some());
    
    // Create a test header
    let mut header = BlockHeader::default();
    header.number = 100;
    header.state_root = B256::from([0x42; 32]);
    let sealed_header = SealedHeader::seal_slow(header);
    
    // Send a canonical chain committed event
    let event = ConsensusEngineEvent::CanonicalChainCommitted(
        Box::new(sealed_header.clone()),
        Duration::from_millis(100)
    );
    
    event_sender.send(event).unwrap();
    
    // Wait for the header to be received
    sleep(Duration::from_millis(50)).await;
    
    // Check that the stage received the header
    let target_state_root = snap_sync_stage.get_target_state_root();
    assert!(target_state_root.is_some());
    assert_eq!(target_state_root.unwrap(), sealed_header.state_root);
    
    // Check that the header receiver also received the update
    let received_header = header_receiver.borrow().clone();
    assert!(received_header.is_some());
    assert_eq!(received_header.unwrap().hash(), sealed_header.hash());
}

#[tokio::test]
async fn test_snap_sync_state_root_change_detection() {
    // Create a broadcast channel for consensus engine events
    let (event_sender, _event_receiver) = broadcast::channel(16);
    
    // Create snap sync config
    let config = SnapSyncConfig {
        enabled: true,
        max_ranges_per_execution: 10,
        max_response_bytes: 1000000,
        request_timeout_seconds: 30,
        range_size: 1000,
        max_retries: 3,
    };
    
    // Create snap client
    let snap_client = Arc::new(MockSnapClient);
    
    // Create snap sync stage with header subscription
    let (mut snap_sync_stage, _header_receiver) = SnapSyncStage::with_header_subscription(
        config,
        snap_client,
        event_sender.clone(),
    );
    
    // Initially no state root
    assert!(snap_sync_stage.get_target_state_root().is_none());
    
    // Create first header
    let mut header1 = BlockHeader::default();
    header1.number = 100;
    header1.state_root = B256::from([0x42; 32]);
    let sealed_header1 = SealedHeader::seal_slow(header1);
    
    // Send first event
    let event1 = ConsensusEngineEvent::CanonicalChainCommitted(
        Box::new(sealed_header1.clone()),
        Duration::from_millis(100)
    );
    event_sender.send(event1).unwrap();
    
    // Wait for processing
    sleep(Duration::from_millis(50)).await;
    
    // Check state root
    let state_root1 = snap_sync_stage.get_target_state_root();
    assert!(state_root1.is_some());
    assert_eq!(state_root1.unwrap(), B256::from([0x42; 32]));
    
    // Test state root change detection
    assert!(snap_sync_stage.has_state_root_changed(None));
    assert!(!snap_sync_stage.has_state_root_changed(state_root1));
    
    // Create second header with different state root
    let mut header2 = BlockHeader::default();
    header2.number = 101;
    header2.state_root = B256::from([0x43; 32]);
    let sealed_header2 = SealedHeader::seal_slow(header2);
    
    // Send second event
    let event2 = ConsensusEngineEvent::CanonicalChainCommitted(
        Box::new(sealed_header2.clone()),
        Duration::from_millis(100)
    );
    event_sender.send(event2).unwrap();
    
    // Wait for processing
    sleep(Duration::from_millis(50)).await;
    
    // Check new state root
    let state_root2 = snap_sync_stage.get_target_state_root();
    assert!(state_root2.is_some());
    assert_eq!(state_root2.unwrap(), B256::from([0x43; 32]));
    
    // Test state root change detection
    assert!(snap_sync_stage.has_state_root_changed(state_root1));
    assert!(!snap_sync_stage.has_state_root_changed(state_root2));
}

#[tokio::test]
async fn test_header_subscription_service_directly() {
    // Create a broadcast channel for consensus engine events
    let (event_sender, _event_receiver) = broadcast::channel(16);
    
    // Create header subscription service
    let (header_service, mut header_receiver) = HeaderSubscriptionService::<EthPrimitives>::new(event_sender.clone());
    
    // Start the service in a background task
    let service_handle = tokio::spawn(async move {
        header_service.start().await;
    });
    
    // Create a test header
    let mut header = BlockHeader::default();
    header.number = 100;
    header.state_root = B256::from([0x42; 32]);
    let sealed_header = SealedHeader::seal_slow(header);
    
    // Send a canonical chain committed event
    let event = ConsensusEngineEvent::CanonicalChainCommitted(
        Box::new(sealed_header.clone()),
        Duration::from_millis(100)
    );
    
    event_sender.send(event).unwrap();
    
    // Wait for the header to be received
    sleep(Duration::from_millis(50)).await;
    
    // Check that we received the header
    let received_header = header_receiver.borrow().clone();
    assert!(received_header.is_some());
    assert_eq!(received_header.unwrap().hash(), sealed_header.hash());
    
    // Clean up
    service_handle.abort();
}