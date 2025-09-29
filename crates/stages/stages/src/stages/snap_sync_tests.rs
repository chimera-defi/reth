#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestStageDB;
    use alloy_primitives::B256;
    use reth_net_p2p::{
        download::DownloadClient,
        snap::SnapClient,
        priority::Priority,
    };
    use reth_network_peers::PeerId;
    use reth_primitives_traits::Header;
    use std::sync::Arc;

    /// Simple mock snap client for testing
    #[derive(Debug, Clone)]
    struct MockSnapClient;

    impl DownloadClient for MockSnapClient {
        fn report_bad_message(&self, _peer_id: PeerId) {}
        fn num_connected_peers(&self) -> usize { 1 }
    }

    // For testing, we'll implement a simplified version that only implements the methods we need
    impl SnapClient for MockSnapClient {
        type Output = futures::future::Ready<reth_net_p2p::error::PeerRequestResult<reth_eth_wire_types::snap::AccountRangeMessage>>;
        
        fn get_account_range_with_priority(
            &self,
            _request: reth_eth_wire_types::snap::GetAccountRangeMessage,
            _priority: Priority,
        ) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: PeerId::random(),
                result: reth_eth_wire_types::snap::AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                },
            }))
        }

        // Stub implementations for other methods (not used in our tests)
        // Note: The trait design has all methods returning the same type, which is a limitation
        fn get_storage_ranges(&self, _request: reth_eth_wire_types::snap::GetStorageRangesMessage) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: PeerId::random(),
                result: reth_eth_wire_types::snap::AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                },
            }))
        }

        fn get_storage_ranges_with_priority(&self, _request: reth_eth_wire_types::snap::GetStorageRangesMessage, _priority: Priority) -> Self::Output {
            self.get_storage_ranges(_request)
        }

        fn get_byte_codes(&self, _request: reth_eth_wire_types::snap::GetByteCodesMessage) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: PeerId::random(),
                result: reth_eth_wire_types::snap::AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                },
            }))
        }

        fn get_byte_codes_with_priority(&self, _request: reth_eth_wire_types::snap::GetByteCodesMessage, _priority: Priority) -> Self::Output {
            self.get_byte_codes(_request)
        }

        fn get_trie_nodes(&self, _request: reth_eth_wire_types::snap::GetTrieNodesMessage) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: PeerId::random(),
                result: reth_eth_wire_types::snap::AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                },
            }))
        }

        fn get_trie_nodes_with_priority(&self, _request: reth_eth_wire_types::snap::GetTrieNodesMessage, _priority: Priority) -> Self::Output {
            self.get_trie_nodes(_request)
        }
    }

    #[test]
    fn test_snap_sync_stage_creation() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let stage = SnapSyncStage::new(config, snap_client);
        assert_eq!(stage.id(), StageId::SnapSync);
    }

    #[test]
    fn test_snap_sync_stage_disabled() {
        let mut config = SnapSyncConfig::default();
        config.enabled = false;
        let snap_client = Arc::new(MockSnapClient);
        let mut stage = SnapSyncStage::new(config, snap_client);
        
        let db = TestStageDB::default();
        let provider = db.factory.database_provider_rw().unwrap();
        let input = ExecInput { target: Some(100), checkpoint: None };
        
        let result = stage.execute(&provider, input);
        assert!(result.is_ok());
        assert!(result.unwrap().done);
    }

    #[test]
    fn test_hashed_state_empty() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let stage = SnapSyncStage::new(config, snap_client);
        
        let db = TestStageDB::default();
        let provider = db.factory.database_provider_rw().unwrap();
        
        let is_empty = stage.is_hashed_state_empty(&provider).unwrap();
        assert!(is_empty);
    }

    #[test]
    fn test_with_header_receiver() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        
        // Create a mock header with a specific state root
        let mock_header = reth_primitives_traits::SealedHeader::new(
            reth_primitives_traits::Header {
                state_root: B256::from_low_u64_be(12345),
                ..Default::default()
            },
            B256::ZERO
        );
        
        let (sender, receiver) = tokio::sync::watch::channel(mock_header);
        
        let stage = SnapSyncStage::new(config, snap_client)
            .with_header_receiver(receiver);
        
        // Test that header receiver is set
        assert!(stage.header_receiver.is_some());
        
        // Test that we can get target state root
        assert_eq!(stage.get_target_state_root(), Some(B256::from_low_u64_be(12345)));
    }

    #[test]
    fn test_create_account_range_request() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let mut stage = SnapSyncStage::new(config, snap_client);
        
        let request = stage.create_account_range_request(B256::ZERO, B256::from([0xff; 32]));
        
        assert_eq!(request.request_id, 1);
        assert_eq!(request.starting_hash, B256::ZERO);
        assert_eq!(request.limit_hash, B256::from([0xff; 32]));
        assert_eq!(request.response_bytes, config.max_response_bytes);
    }

    #[test]
    fn test_process_empty_account_ranges() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let stage = SnapSyncStage::new(config, snap_client);
        
        let db = TestStageDB::default();
        let provider = db.factory.database_provider_rw().unwrap();
        
        let processed = stage.process_account_ranges(provider, vec![]).unwrap();
        assert_eq!(processed, 0);
    }

    #[test]
    fn test_account_range_proof_verification() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let stage = SnapSyncStage::new(config, snap_client);
        
        // Test empty account range
        let empty_range = reth_eth_wire_types::snap::AccountRangeMessage {
            request_id: 1,
            accounts: vec![],
            proof: vec![],
        };
        assert!(stage.verify_account_range_proof(&empty_range).unwrap());
        
        // Test account range with accounts but no proof (should still pass with warning)
        let range_with_accounts = reth_eth_wire_types::snap::AccountRangeMessage {
            request_id: 1,
            accounts: vec![reth_eth_wire_types::snap::AccountData {
                hash: B256::from_low_u64_be(1),
                body: alloy_primitives::Bytes::new(),
            }],
            proof: vec![],
        };
        assert!(stage.verify_account_range_proof(&range_with_accounts).unwrap());
    }

    #[test]
    fn test_retry_logic() {
        let mut config = SnapSyncConfig::default();
        config.max_retry_attempts = 3;
        let snap_client = Arc::new(MockSnapClient);
        let mut stage = SnapSyncStage::new(config, snap_client);
        
        // Create a test request
        let request = reth_eth_wire_types::snap::GetAccountRangeMessage {
            request_id: 1,
            root_hash: B256::ZERO,
            starting_hash: B256::ZERO,
            limit_hash: B256::from_low_u64_be(100),
            response_bytes: 1024,
        };
        
        // Test handling failed request
        stage.handle_failed_request(1, request.clone());
        assert_eq!(stage.retry_attempts.get(&1), Some(&1));
        assert_eq!(stage.failed_requests.len(), 1);
        
        // Test retry queue processing (should not retry immediately)
        stage.process_retry_queue().unwrap();
        assert_eq!(stage.failed_requests.len(), 1); // Still in queue
        
        // Test max retries exceeded
        for _ in 0..3 {
            stage.handle_failed_request(1, request.clone());
        }
        assert_eq!(stage.retry_attempts.get(&1), None); // Removed after max retries
    }

    #[test]
    fn test_peer_selection() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let mut stage = SnapSyncStage::new(config, snap_client);
        
        // Test with no peers
        assert!(stage.select_peer().is_err());
        
        // Add some peers
        let peer1 = PeerId::random();
        let peer2 = PeerId::random();
        let peer3 = PeerId::random();
        
        stage.add_peer(peer1);
        stage.add_peer(peer2);
        stage.add_peer(peer3);
        
        // Test peer selection
        let selected_peer = stage.select_peer().unwrap();
        assert!(stage.available_peers.contains(&selected_peer));
        
        // Test peer metrics update
        stage.update_peer_metrics(peer1, true);
        stage.update_peer_metrics(peer2, false);
        
        let stats = stage.get_peer_stats();
        assert_eq!(stats.len(), 3);
        
        // Test peer removal
        stage.remove_peer(peer1);
        assert!(!stage.available_peers.contains(&peer1));
        assert_eq!(stage.available_peers.len(), 2);
    }

    #[test]
    fn test_configurable_range_size() {
        let mut config = SnapSyncConfig::default();
        config.range_size = 0x1000000000000000; // 1/16th of hash space
        config.min_range_size = 0x10000000000000; // 1/256th of hash space
        config.max_range_size = 0x10000000000000000; // 1/8th of hash space
        config.adaptive_range_sizing = true;
        
        let snap_client = Arc::new(MockSnapClient);
        let mut stage = SnapSyncStage::new(config, snap_client);
        
        // Test initial range size
        assert_eq!(stage.get_current_range_size(), 0x1000000000000000);
        
        // Test network metrics update with good performance
        stage.update_network_metrics(500.0, true); // Fast response, success
        stage.update_network_metrics(600.0, true); // Fast response, success
        stage.update_network_metrics(700.0, true); // Fast response, success
        
        // Should increase range size due to good performance
        let new_size = stage.get_current_range_size();
        assert!(new_size >= 0x1000000000000000); // Should be at least the original size
        
        // Test network metrics update with poor performance
        stage.update_network_metrics(6000.0, false); // Slow response, failure
        stage.update_network_metrics(7000.0, false); // Slow response, failure
        stage.update_network_metrics(8000.0, false); // Slow response, failure
        
        // Should decrease range size due to poor performance
        let adjusted_size = stage.get_current_range_size();
        assert!(adjusted_size <= new_size); // Should be smaller or equal
        
        // Test reset
        stage.reset_range_size();
        assert_eq!(stage.get_current_range_size(), 0x1000000000000000);
    }

    #[test]
    fn test_request_timeout_handling() {
        let mut config = SnapSyncConfig::default();
        config.request_timeout_seconds = 1; // 1 second timeout for testing
        config.max_retry_attempts = 2;
        
        let snap_client = Arc::new(MockSnapClient);
        let mut stage = SnapSyncStage::new(config, snap_client);
        
        // Test request tracking
        stage.start_request_tracking(1);
        assert_eq!(stage.get_active_request_count(), 1);
        
        // Test successful completion
        stage.complete_request_tracking(1, true);
        assert_eq!(stage.get_active_request_count(), 0);
        
        // Test timeout handling
        stage.start_request_tracking(2);
        assert_eq!(stage.get_active_request_count(), 1);
        
        // Simulate timeout by checking timeouts immediately
        // (In real implementation, this would be called after the timeout period)
        stage.check_timeouts().unwrap();
        
        // The request should still be active since we didn't wait for the actual timeout
        assert_eq!(stage.get_active_request_count(), 1);
        
        // Test timeout configuration
        assert_eq!(stage.get_timeout_seconds(), 1);
    }
}