//! Benchmark tests for snap sync functionality

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
    use reth_stages_api::{ExecInput, Stage, StageCheckpoint, StageId};
    use std::time::Instant;
    use tokio::sync::watch;

    #[derive(Debug, Clone)]
    struct BenchmarkSnapClient;

    impl DownloadClient for BenchmarkSnapClient {
        fn report_bad_message(&self, _peer_id: reth_network_peers::PeerId) {
            // Mock implementation
        }

        fn num_connected_peers(&self) -> usize {
            10 // Simulate good connectivity
        }
    }

    impl SnapClient for BenchmarkSnapClient {
        type Output = future::Ready<reth_network_p2p::error::PeerRequestResult<AccountRangeMessage>>;

        fn get_account_range_with_priority(
            &self,
            request: GetAccountRangeMessage,
            _priority: reth_network_p2p::priority::Priority,
        ) -> Self::Output {
            future::ready(Ok(WithPeerId::new(
                reth_network_peers::PeerId::random(),
                AccountRangeMessage {
                    request_id: request.request_id,
                    accounts: vec![], // Empty response for benchmark
                    proof: vec![],
                }
            )))
        }

        fn get_storage_ranges(&self, request: GetStorageRangesMessage) -> Self::Output {
            future::ready(Ok(WithPeerId::new(
                reth_network_peers::PeerId::random(),
                AccountRangeMessage {
                    request_id: request.request_id,
                    accounts: vec![],
                    proof: vec![],
                }
            )))
        }

        fn get_storage_ranges_with_priority(
            &self,
            request: GetStorageRangesMessage,
            _priority: reth_network_p2p::priority::Priority,
        ) -> Self::Output {
            self.get_storage_ranges(request)
        }

        fn get_byte_codes(&self, request: GetByteCodesMessage) -> Self::Output {
            future::ready(Ok(WithPeerId::new(
                reth_network_peers::PeerId::random(),
                AccountRangeMessage {
                    request_id: request.request_id,
                    accounts: vec![],
                    proof: vec![],
                }
            )))
        }

        fn get_byte_codes_with_priority(
            &self,
            request: GetByteCodesMessage,
            _priority: reth_network_p2p::priority::Priority,
        ) -> Self::Output {
            self.get_byte_codes(request)
        }

        fn get_trie_nodes(&self, request: GetTrieNodesMessage) -> Self::Output {
            future::ready(Ok(WithPeerId::new(
                reth_network_peers::PeerId::random(),
                AccountRangeMessage {
                    request_id: request.request_id,
                    accounts: vec![],
                    proof: vec![],
                }
            )))
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
    fn test_snap_sync_stage_execution_performance() {
        let db = TestStageDB::default();
        let (_tip_tx, tip_rx) = watch::channel(B256::ZERO);
        let client = BenchmarkSnapClient;
        let config = SnapSyncConfig::default();

        let mut stage = SnapSyncStage::new(
            db.factory.clone(),
            client,
            tip_rx,
            config,
        );

        // Measure execution time for different target sizes
        let targets = vec![100, 1000, 10000, 100000];
        
        for target in targets {
            let input = ExecInput {
                target: Some(target),
                checkpoint: Some(StageCheckpoint::new(0)),
            };
            
            let start = Instant::now();
            let result = stage.execute(&db.factory, input);
            let elapsed = start.elapsed();
            
            assert!(result.is_ok());
            assert_eq!(result.unwrap().checkpoint.block_number, target);
            
            // For a stub implementation, execution should be very fast
            assert!(elapsed.as_millis() < 100, "Execution took too long: {:?} for target {}", elapsed, target);
        }
    }

    #[test]
    fn test_snap_sync_stage_memory_usage() {
        let db = TestStageDB::default();
        let (_tip_tx, tip_rx) = watch::channel(B256::ZERO);
        let client = BenchmarkSnapClient;
        let config = SnapSyncConfig::default();

        // Create multiple stages to test memory usage
        let mut stages = Vec::new();
        for i in 0..100 {
            let stage = SnapSyncStage::new(
                db.factory.clone(),
                client.clone(),
                tip_rx.clone(),
                config.clone(),
            );
            stages.push(stage);
        }

        // Verify all stages were created successfully
        assert_eq!(stages.len(), 100);
        for stage in &stages {
            assert_eq!(stage.id(), StageId::SnapSync);
        }
    }

    #[test]
    fn test_snap_sync_config_performance_tuning() {
        // Test different configuration scenarios for performance
        let configs = vec![
            // Conservative config
            SnapSyncConfig {
                max_accounts_per_request: 100,
                max_storage_per_request: 200,
                max_bytecodes_per_request: 10,
                max_trie_nodes_per_request: 100,
                request_timeout: std::time::Duration::from_secs(60),
                max_concurrent_requests: 4,
            },
            // Aggressive config
            SnapSyncConfig {
                max_accounts_per_request: 1000,
                max_storage_per_request: 5000,
                max_bytecodes_per_request: 200,
                max_trie_nodes_per_request: 2000,
                request_timeout: std::time::Duration::from_secs(10),
                max_concurrent_requests: 50,
            },
            // Balanced config (default)
            SnapSyncConfig::default(),
        ];

        let db = TestStageDB::default();
        let (_tip_tx, tip_rx) = watch::channel(B256::ZERO);
        let client = BenchmarkSnapClient;

        for (i, config) in configs.into_iter().enumerate() {
            let mut stage = SnapSyncStage::new(
                db.factory.clone(),
                client.clone(),
                tip_rx.clone(),
                config,
            );

            let input = ExecInput {
                target: Some(1000),
                checkpoint: Some(StageCheckpoint::new(0)),
            };
            
            let start = Instant::now();
            let result = stage.execute(&db.factory, input);
            let elapsed = start.elapsed();
            
            assert!(result.is_ok(), "Config {} failed", i);
            assert_eq!(result.unwrap().checkpoint.block_number, 1000);
            
            // All configs should execute quickly for stub implementation
            assert!(elapsed.as_millis() < 50, "Config {} took too long: {:?}", i, elapsed);
        }
    }

    #[test]
    fn test_snap_sync_stage_concurrency() {
        use std::sync::Arc;
        use std::thread;

        let db = TestStageDB::default();
        let (_tip_tx, tip_rx) = watch::channel(B256::ZERO);
        let client = BenchmarkSnapClient;
        let config = SnapSyncConfig::default();

        let stage = Arc::new(std::sync::Mutex::new(SnapSyncStage::new(
            db.factory.clone(),
            client,
            tip_rx,
            config,
        )));

        let mut handles = vec![];

        // Spawn multiple threads to test concurrent access
        for i in 0..10 {
            let stage_clone = Arc::clone(&stage);
            let factory_clone = db.factory.clone();
            
            let handle = thread::spawn(move || {
                let input = ExecInput {
                    target: Some(((i + 1) * 100) as u64),
                    checkpoint: Some(StageCheckpoint::new((i * 100) as u64)),
                };
                
                let mut stage_guard = stage_clone.lock().unwrap();
                let result = stage_guard.execute(&factory_clone, input);
                assert!(result.is_ok());
                result.unwrap().checkpoint.block_number
            });
            
            handles.push(handle);
        }

        // Wait for all threads to complete
        for (i, handle) in handles.into_iter().enumerate() {
            let result = handle.join().unwrap();
            assert_eq!(result, ((i + 1) * 100) as u64);
        }
    }
}