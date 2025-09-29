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
        let (sender, receiver) = tokio::sync::watch::channel(B256::ZERO);
        
        let stage = SnapSyncStage::new(config, snap_client)
            .with_header_receiver(receiver);
        
        // Test that header receiver is set
        assert!(stage.header_receiver.is_some());
        
        // Test that we can get target state root
        assert_eq!(stage.get_target_state_root(), Some(B256::ZERO));
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
}