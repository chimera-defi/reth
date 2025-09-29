#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestStageDB;
    use reth_net_p2p::{
        download::DownloadClient,
        error::PeerRequestResult,
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

    impl SnapClient for MockSnapClient {
        type Output = futures::future::Ready<PeerRequestResult<reth_eth_wire_types::snap::AccountRangeMessage>>;
        
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

        // Simplified implementations for other methods
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
}