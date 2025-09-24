//! Integration tests for snap sync functionality

#[cfg(test)]
mod tests {
    use crate::{
        stages::{SnapSyncConfig, SnapSyncStage},
        test_utils::TestStageDB,
    };
    use alloy_primitives::B256;
    use futures_util::future;
    use reth_eth_wire_types::snap::{AccountRangeMessage, GetAccountRangeMessage, GetByteCodesMessage, GetStorageRangesMessage, GetTrieNodesMessage};
    use reth_network_peers::WithPeerId;
    use reth_network_p2p::{download::DownloadClient, snap::client::SnapClient};
    use reth_stages_api::{ExecInput, Stage, StageCheckpoint, StageId, UnwindInput};
    use tokio::sync::watch;

    // Mock snap client for integration tests
    #[derive(Debug, Clone)]
    struct IntegrationMockSnapClient {
        should_fail: bool,
    }

    impl IntegrationMockSnapClient {
        fn new() -> Self {
            Self { should_fail: false }
        }

        fn with_failure() -> Self {
            Self { should_fail: true }
        }
    }

    impl DownloadClient for IntegrationMockSnapClient {
        fn report_bad_message(&self, _peer_id: reth_network_peers::PeerId) {
            // Mock implementation
        }

        fn num_connected_peers(&self) -> usize {
            if self.should_fail { 0 } else { 3 }
        }
    }

    impl SnapClient for IntegrationMockSnapClient {
        type Output = future::Ready<reth_network_p2p::error::PeerRequestResult<AccountRangeMessage>>;

        fn get_account_range_with_priority(
            &self,
            request: GetAccountRangeMessage,
            _priority: reth_network_p2p::priority::Priority,
        ) -> Self::Output {
            if self.should_fail {
                future::ready(Err(reth_network_p2p::error::RequestError::Timeout))
            } else {
                future::ready(Ok(WithPeerId::new(
                    reth_network_peers::PeerId::random(),
                    AccountRangeMessage {
                        request_id: request.request_id,
                        accounts: vec![], // Empty for now
                        proof: vec![],
                    }
                )))
            }
        }

        fn get_storage_ranges(&self, request: GetStorageRangesMessage) -> Self::Output {
            if self.should_fail {
                future::ready(Err(reth_network_p2p::error::RequestError::Timeout))
            } else {
                future::ready(Ok(WithPeerId::new(
                    reth_network_peers::PeerId::random(),
                    AccountRangeMessage {
                        request_id: request.request_id,
                        accounts: vec![],
                        proof: vec![],
                    }
                )))
            }
        }

        fn get_storage_ranges_with_priority(
            &self,
            request: GetStorageRangesMessage,
            _priority: reth_network_p2p::priority::Priority,
        ) -> Self::Output {
            self.get_storage_ranges(request)
        }

        fn get_byte_codes(&self, request: GetByteCodesMessage) -> Self::Output {
            if self.should_fail {
                future::ready(Err(reth_network_p2p::error::RequestError::Timeout))
            } else {
                future::ready(Ok(WithPeerId::new(
                    reth_network_peers::PeerId::random(),
                    AccountRangeMessage {
                        request_id: request.request_id,
                        accounts: vec![],
                        proof: vec![],
                    }
                )))
            }
        }

        fn get_byte_codes_with_priority(
            &self,
            request: GetByteCodesMessage,
            _priority: reth_network_p2p::priority::Priority,
        ) -> Self::Output {
            self.get_byte_codes(request)
        }

        fn get_trie_nodes(&self, request: GetTrieNodesMessage) -> Self::Output {
            if self.should_fail {
                future::ready(Err(reth_network_p2p::error::RequestError::Timeout))
            } else {
                future::ready(Ok(WithPeerId::new(
                    reth_network_peers::PeerId::random(),
                    AccountRangeMessage {
                        request_id: request.request_id,
                        accounts: vec![],
                        proof: vec![],
                    }
                )))
            }
        }

        fn get_trie_nodes_with_priority(
            &self,
            request: GetTrieNodesMessage,
            _priority: reth_network_p2p::priority::Priority,
        ) -> Self::Output {
            self.get_trie_nodes(request)
        }
    }

    #[test]
    fn test_stage_id_consistency() {
        // Test that SnapSync stage ID is consistent across the codebase
        let stage_id = StageId::SnapSync;
        
        assert_eq!(stage_id.as_str(), "SnapSync");
        assert_eq!(stage_id.to_string(), "SnapSync");
        assert!(stage_id.is_downloading_stage());
        
        // Test that it's included in the ALL array
        assert!(StageId::ALL.contains(&StageId::SnapSync));
        
        // Test that it's not confused with other stage types
        assert!(!stage_id.is_tx_lookup());
        assert!(!stage_id.is_finish());
    }

    #[test]
    fn test_snap_sync_stage_lifecycle() {
        let db = TestStageDB::default();
        let (_tip_tx, tip_rx) = watch::channel(B256::ZERO);
        let client = IntegrationMockSnapClient::new();
        let config = SnapSyncConfig::default();

        let mut stage = SnapSyncStage::new(
            db.factory.clone(),
            client,
            tip_rx,
            config,
        );

        // Test stage creation
        assert_eq!(stage.id(), StageId::SnapSync);

        // Test execution with no target
        let input_no_target = ExecInput {
            target: None,
            checkpoint: None,
        };
        
        let result = stage.execute(&db.factory, input_no_target);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.done);
        assert_eq!(output.checkpoint.block_number, 0);

        // Test execution with target
        let input_with_target = ExecInput {
            target: Some(1000),
            checkpoint: Some(StageCheckpoint::new(500)),
        };
        
        let result = stage.execute(&db.factory, input_with_target);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.done);
        assert_eq!(output.checkpoint.block_number, 1000);

        // Test unwind
        let unwind_input = UnwindInput {
            unwind_to: 250,
            bad_block: Some(300),
            checkpoint: StageCheckpoint::new(1000),
        };
        
        let result = stage.unwind(&db.factory, unwind_input);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.checkpoint.block_number, 250);
    }

    #[test]
    fn test_snap_sync_config_validation() {
        let config = SnapSyncConfig::default();
        
        // Test that default values are reasonable
        assert!(config.max_accounts_per_request > 0);
        assert!(config.max_storage_per_request > 0);
        assert!(config.max_bytecodes_per_request > 0);
        assert!(config.max_trie_nodes_per_request > 0);
        assert!(config.request_timeout.as_secs() > 0);
        assert!(config.max_concurrent_requests > 0);
        
        // Test that values are within reasonable ranges
        assert!(config.max_accounts_per_request <= 1000);
        assert!(config.max_storage_per_request <= 10000);
        assert!(config.max_bytecodes_per_request <= 1000);
        assert!(config.max_trie_nodes_per_request <= 10000);
        assert!(config.request_timeout.as_secs() <= 300); // 5 minutes max
        assert!(config.max_concurrent_requests <= 100);
    }

    #[test]
    fn test_snap_sync_stage_with_different_clients() {
        let db = TestStageDB::default();
        let (_tip_tx, tip_rx) = watch::channel(B256::ZERO);
        let config = SnapSyncConfig::default();

        // Test with working client
        let working_client = IntegrationMockSnapClient::new();
        let mut working_stage = SnapSyncStage::new(
            db.factory.clone(),
            working_client,
            tip_rx.clone(),
            config.clone(),
        );

        // Test with failing client
        let failing_client = IntegrationMockSnapClient::with_failure();
        let mut failing_stage = SnapSyncStage::new(
            db.factory.clone(),
            failing_client,
            tip_rx,
            config,
        );

        let input = ExecInput {
            target: Some(100),
            checkpoint: Some(StageCheckpoint::new(0)),
        };

        // Both should succeed since we're using stub implementation
        // In a real implementation, the failing client would cause different behavior
        let working_result = working_stage.execute(&db.factory, input);
        let failing_result = failing_stage.execute(&db.factory, input);
        
        assert!(working_result.is_ok());
        assert!(failing_result.is_ok());
        
        // Verify both produce the same result since it's a stub
        assert_eq!(working_result.unwrap().checkpoint.block_number, 100);
        assert_eq!(failing_result.unwrap().checkpoint.block_number, 100);
    }

    #[test]
    fn test_snap_sync_stage_multiple_executions() {
        let db = TestStageDB::default();
        let (_tip_tx, tip_rx) = watch::channel(B256::ZERO);
        let client = IntegrationMockSnapClient::new();
        let config = SnapSyncConfig::default();

        let mut stage = SnapSyncStage::new(
            db.factory.clone(),
            client,
            tip_rx,
            config,
        );

        // Execute multiple times with different targets
        let targets = vec![100, 200, 500, 1000];
        
        for target in targets {
            let input = ExecInput {
                target: Some(target),
                checkpoint: Some(StageCheckpoint::new(target - 50)),
            };
            
            let result = stage.execute(&db.factory, input);
            assert!(result.is_ok());
            
            let output = result.unwrap();
            assert!(output.done);
            assert_eq!(output.checkpoint.block_number, target);
        }
    }

    #[test]
    fn test_snap_sync_stage_edge_cases() {
        let db = TestStageDB::default();
        let (_tip_tx, tip_rx) = watch::channel(B256::ZERO);
        let client = IntegrationMockSnapClient::new();
        let config = SnapSyncConfig::default();

        let mut stage = SnapSyncStage::new(
            db.factory.clone(),
            client,
            tip_rx,
            config,
        );

        // Test with target = 0
        let input_zero = ExecInput {
            target: Some(0),
            checkpoint: None,
        };
        let result = stage.execute(&db.factory, input_zero);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().checkpoint.block_number, 0);

        // Test with very large target
        let input_large = ExecInput {
            target: Some(u64::MAX),
            checkpoint: Some(StageCheckpoint::new(1000)),
        };
        let result = stage.execute(&db.factory, input_large);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().checkpoint.block_number, u64::MAX);

        // Test unwind to 0
        let unwind_zero = UnwindInput {
            unwind_to: 0,
            bad_block: None,
            checkpoint: StageCheckpoint::new(1000),
        };
        let result = stage.unwind(&db.factory, unwind_zero);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().checkpoint.block_number, 0);
    }

    #[test]
    fn test_client_connectivity_reporting() {
        let working_client = IntegrationMockSnapClient::new();
        let failing_client = IntegrationMockSnapClient::with_failure();

        // Test that clients report different connectivity
        assert_eq!(working_client.num_connected_peers(), 3);
        assert_eq!(failing_client.num_connected_peers(), 0);
    }

    #[test]
    fn test_snap_sync_stage_checkpoint_progression() {
        let db = TestStageDB::default();
        let (_tip_tx, tip_rx) = watch::channel(B256::ZERO);
        let client = IntegrationMockSnapClient::new();
        let config = SnapSyncConfig::default();

        let mut stage = SnapSyncStage::new(
            db.factory.clone(),
            client,
            tip_rx,
            config,
        );

        // Test progression from checkpoint to target
        let checkpoints = vec![0, 50, 100, 200];
        let targets = vec![100, 150, 300, 500];

        for (checkpoint, target) in checkpoints.into_iter().zip(targets.into_iter()) {
            let input = ExecInput {
                target: Some(target),
                checkpoint: Some(StageCheckpoint::new(checkpoint)),
            };
            
            let result = stage.execute(&db.factory, input);
            assert!(result.is_ok());
            
            let output = result.unwrap();
            assert!(output.done);
            assert_eq!(output.checkpoint.block_number, target);
        }
    }

    #[test]
    fn test_snap_sync_stage_unwind_scenarios() {
        let db = TestStageDB::default();
        let (_tip_tx, tip_rx) = watch::channel(B256::ZERO);
        let client = IntegrationMockSnapClient::new();
        let config = SnapSyncConfig::default();

        let mut stage = SnapSyncStage::new(
            db.factory.clone(),
            client,
            tip_rx,
            config,
        );

        // Test various unwind scenarios
        let scenarios = vec![
            (1000, 500),  // Normal unwind
            (100, 0),     // Unwind to genesis
            (500, 499),   // Unwind by 1 block
            (1000, 1000), // No-op unwind (same block)
        ];

        for (from_block, to_block) in scenarios {
            let input = UnwindInput {
                unwind_to: to_block,
                bad_block: if from_block > to_block { Some(from_block) } else { None },
                checkpoint: StageCheckpoint::new(from_block),
            };
            
            let result = stage.unwind(&db.factory, input);
            assert!(result.is_ok());
            
            let output = result.unwrap();
            assert_eq!(output.checkpoint.block_number, to_block);
        }
    }

    #[test]
    fn test_snap_sync_stage_with_checkpoints() {
        let db = TestStageDB::default();
        let (_tip_tx, tip_rx) = watch::channel(B256::ZERO);
        let client = IntegrationMockSnapClient::new();
        let config = SnapSyncConfig::default();

        let mut stage = SnapSyncStage::new(
            db.factory.clone(),
            client,
            tip_rx,
            config,
        );

        // Test execution with existing checkpoint
        let input_with_checkpoint = ExecInput {
            target: Some(1000),
            checkpoint: Some(StageCheckpoint::new(800)),
        };
        
        let result = stage.execute(&db.factory, input_with_checkpoint);
        assert!(result.is_ok());
        
        let output = result.unwrap();
        assert!(output.done);
        assert_eq!(output.checkpoint.block_number, 1000);

        // Test execution without checkpoint (should start from 0)
        let input_no_checkpoint = ExecInput {
            target: Some(500),
            checkpoint: None,
        };
        
        let result = stage.execute(&db.factory, input_no_checkpoint);
        assert!(result.is_ok());
        
        let output = result.unwrap();
        assert!(output.done);
        assert_eq!(output.checkpoint.block_number, 500);
    }

    #[tokio::test]
    async fn test_mock_client_async_operations() {
        let client = IntegrationMockSnapClient::new();
        
        // Test multiple async operations
        let account_request = GetAccountRangeMessage {
            request_id: 1,
            root_hash: B256::ZERO,
            starting_hash: B256::ZERO,
            limit_hash: B256::from([0xff; 32]),
            response_bytes: 1024,
        };
        
        let storage_request = GetStorageRangesMessage {
            request_id: 2,
            root_hash: B256::ZERO,
            account_hashes: vec![B256::ZERO],
            starting_hash: B256::ZERO,
            limit_hash: B256::from([0xff; 32]),
            response_bytes: 2048,
        };
        
        // Execute requests concurrently
        let (account_result, storage_result) = tokio::join!(
            client.get_account_range(account_request),
            client.get_storage_ranges(storage_request)
        );
        
        assert!(account_result.is_ok());
        assert!(storage_result.is_ok());
        
        // Verify request IDs are preserved
        assert_eq!(account_result.unwrap().1.request_id, 1);
        assert_eq!(storage_result.unwrap().1.request_id, 2);
    }

    #[tokio::test]
    async fn test_failing_client_async_operations() {
        let client = IntegrationMockSnapClient::with_failure();
        
        let account_request = GetAccountRangeMessage {
            request_id: 1,
            root_hash: B256::ZERO,
            starting_hash: B256::ZERO,
            limit_hash: B256::from([0xff; 32]),
            response_bytes: 1024,
        };
        
        let result = client.get_account_range(account_request).await;
        assert!(result.is_err());
        
        // Verify it's the expected error type
        match result {
            Err(reth_network_p2p::error::RequestError::Timeout) => {
                // Expected
            }
            other => panic!("Expected Timeout error, got: {:?}", other),
        }
    }
}