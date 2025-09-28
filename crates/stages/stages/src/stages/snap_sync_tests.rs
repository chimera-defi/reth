#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{ExecuteStageTestRunner, StageTestRunner, TestStageDB};
    use alloy_primitives::{B256, U256};
    use reth_eth_wire_types::snap::{AccountRangeMessage, AccountData, GetAccountRangeMessage};
    use reth_net_p2p::{
        download::DownloadClient,
        error::PeerRequestResult,
        snap::SnapClient,
        priority::Priority,
    };
    use reth_network_peers::PeerId;
    use std::sync::Arc;

    /// Mock snap client for testing
    #[derive(Debug, Clone)]
    struct MockSnapClient;

    impl DownloadClient for MockSnapClient {
        fn report_bad_message(&self, _peer_id: PeerId) {}
        fn num_connected_peers(&self) -> usize { 1 }
    }

    impl SnapClient for MockSnapClient {
        type Output = futures::future::Ready<PeerRequestResult<AccountRangeMessage>>;
        
        fn get_account_range_with_priority(
            &self,
            _request: GetAccountRangeMessage,
            _priority: Priority,
        ) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: PeerId::random(),
                result: AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                },
            }))
        }

        // For testing purposes, we'll return AccountRangeMessage for all methods
        // In a real implementation, these would return their respective message types
        fn get_storage_ranges(&self, _request: reth_eth_wire_types::snap::GetStorageRangesMessage) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: PeerId::random(),
                result: AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                },
            }))
        }

        fn get_storage_ranges_with_priority(
            &self,
            _request: reth_eth_wire_types::snap::GetStorageRangesMessage,
            _priority: Priority,
        ) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: PeerId::random(),
                result: AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                },
            }))
        }

        fn get_byte_codes(&self, _request: reth_eth_wire_types::snap::GetByteCodesMessage) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: PeerId::random(),
                result: AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                },
            }))
        }

        fn get_byte_codes_with_priority(
            &self,
            _request: reth_eth_wire_types::snap::GetByteCodesMessage,
            _priority: Priority,
        ) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: PeerId::random(),
                result: AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                },
            }))
        }

        fn get_trie_nodes(&self, _request: reth_eth_wire_types::snap::GetTrieNodesMessage) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: PeerId::random(),
                result: AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                },
            }))
        }

        fn get_trie_nodes_with_priority(
            &self,
            _request: reth_eth_wire_types::snap::GetTrieNodesMessage,
            _priority: Priority,
        ) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: PeerId::random(),
                result: AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                },
            }))
        }
    }

    #[test]
    fn test_snap_sync_stage_creation() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let stage = SnapSyncStage::new(config, snap_client);
        assert_eq!(stage.config.enabled, false);
        assert_eq!(stage.current_starting_hash, B256::ZERO);
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
    fn test_snap_sync_stage_enabled() {
        let mut config = SnapSyncConfig::default();
        config.enabled = true;
        let snap_client = Arc::new(MockSnapClient);
        let mut stage = SnapSyncStage::new(config, snap_client);
        let db = TestStageDB::default();
        let provider = db.factory.database_provider_rw().unwrap();
        let input = ExecInput { target: Some(100), checkpoint: None };
        let result = stage.execute(&provider, input);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(!output.done);
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
    fn test_proof_verification() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let stage = SnapSyncStage::new(config, snap_client);
        
        // Test with empty proof (should pass)
        let account_range = AccountRangeMessage {
            request_id: 1,
            accounts: vec![],
            proof: vec![],
        };
        assert!(stage.verify_proof(&account_range).unwrap());

        // Test with valid accounts
        let mut accounts = Vec::new();
        for i in 0..5 {
            let mut hash_bytes = [0u8; 32];
            hash_bytes[31] = i as u8;
            let account_hash = B256::from(hash_bytes);
            
            let account = reth_primitives_traits::Account {
                nonce: i,
                balance: U256::from(i * 1000),
                bytecode_hash: None,
            };
            
            let mut account_rlp = Vec::new();
            account.encode(&mut account_rlp);
            
            accounts.push(AccountData {
                hash: account_hash,
                body: account_rlp.into(),
            });
        }
        
        let account_range = AccountRangeMessage {
            request_id: 1,
            accounts,
            proof: vec![],
        };
        assert!(stage.verify_proof(&account_range).unwrap());
    }

    #[test]
    fn test_proof_verification_invalid_order() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let stage = SnapSyncStage::new(config, snap_client);
        
        // Test with accounts in wrong order (should fail)
        let mut accounts = Vec::new();
        for i in (0..5).rev() { // Reverse order
            let mut hash_bytes = [0u8; 32];
            hash_bytes[31] = i as u8;
            let account_hash = B256::from(hash_bytes);
            
            let account = reth_primitives_traits::Account {
                nonce: i,
                balance: U256::from(i * 1000),
                bytecode_hash: None,
            };
            
            let mut account_rlp = Vec::new();
            account.encode(&mut account_rlp);
            
            accounts.push(AccountData {
                hash: account_hash,
                body: account_rlp.into(),
            });
        }
        
        let account_range = AccountRangeMessage {
            request_id: 1,
            accounts,
            proof: vec![],
        };
        assert!(stage.verify_proof(&account_range).is_err());
    }

    #[test]
    fn test_config_validation() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let stage = SnapSyncStage::new(config, snap_client);
        
        // Valid config should pass
        assert!(stage.validate_config().is_ok());
        
        // Test invalid configs
        let mut invalid_config = SnapSyncConfig::default();
        invalid_config.max_ranges_per_execution = 0;
        let invalid_stage = SnapSyncStage::new(invalid_config, snap_client.clone());
        assert!(invalid_stage.validate_config().is_err());
        
        let mut invalid_config = SnapSyncConfig::default();
        invalid_config.max_response_bytes = 0;
        let invalid_stage = SnapSyncStage::new(invalid_config, snap_client.clone());
        assert!(invalid_stage.validate_config().is_err());
    }

    #[test]
    fn test_metrics() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let stage = SnapSyncStage::new(config, snap_client);
        
        let metrics = stage.get_metrics();
        assert_eq!(metrics.ranges_processed, 0);
        assert_eq!(metrics.accounts_downloaded, 0);
        assert_eq!(metrics.failed_requests, 0);
    }

    #[test]
    fn test_stage_id() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let stage = SnapSyncStage::new(config, snap_client);
        assert_eq!(stage.id(), StageId::SnapSync);
    }
}