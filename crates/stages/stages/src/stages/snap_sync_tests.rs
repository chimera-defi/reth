//! Integration tests for the snap sync stage.

use super::SnapSyncStage;
use crate::test_utils::{StorageKind, TestStageDB};
use alloy_primitives::B256;
use reth_config::config::{EtlConfig, SnapSyncConfig};
use reth_network_downloaders::snap::test_utils::TestSnapClient;
use reth_stages_api::{ExecInput, Stage, StageId};
use std::sync::Arc;

#[tokio::test]
async fn test_snap_sync_stage_creation() {
    let test_db = TestStageDB::default();
    let provider = test_db.factory.provider_rw().unwrap();
    let client = Arc::new(TestSnapClient::new());
    let config = SnapSyncConfig::default();
    let etl_config = EtlConfig::default();
    
    let stage = SnapSyncStage::new(provider.clone(), client, config, etl_config);
    
    assert_eq!(stage.id(), StageId::SnapSync);
}

#[tokio::test]
async fn test_snap_sync_stage_execute() {
    let test_db = TestStageDB::default();
    let provider = test_db.factory.provider_rw().unwrap();
    let client = Arc::new(TestSnapClient::new());
    let config = SnapSyncConfig::default();
    let etl_config = EtlConfig::default();
    
    let mut stage = SnapSyncStage::new(provider.clone(), client, config, etl_config);
    
    let input = ExecInput {
        target: Some(100),
        checkpoint: None,
    };
    
    // Execute the stage
    let result = stage.execute(&provider, input);
    
    // The stage should execute without errors
    assert!(result.is_ok());
    
    let output = result.unwrap();
    assert!(output.done);
}

#[tokio::test]
async fn test_snap_sync_stage_unwind() {
    let test_db = TestStageDB::default();
    let provider = test_db.factory.provider_rw().unwrap();
    let client = Arc::new(TestSnapClient::new());
    let config = SnapSyncConfig::default();
    let etl_config = EtlConfig::default();
    
    let mut stage = SnapSyncStage::new(provider.clone(), client, config, etl_config);
    
    let unwind_input = reth_stages_api::UnwindInput {
        checkpoint: reth_stages_api::StageCheckpoint::new(50),
        unwind_to: 25,
        bad_block: None,
    };
    
    // Test unwind
    let result = stage.unwind(&provider, unwind_input);
    
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output.checkpoint.block_number, 25);
}

#[test]
fn test_snap_sync_config_integration() {
    let config = SnapSyncConfig {
        max_concurrent_requests: 5,
        max_response_bytes: 1024 * 1024,
        max_accounts_per_request: 500,
        max_storage_slots_per_request: 500,
        max_byte_codes_per_request: 50,
        max_trie_nodes_per_request: 500,
        commit_threshold: 5000,
    };
    
    // Test that custom config values are preserved
    assert_eq!(config.max_concurrent_requests, 5);
    assert_eq!(config.max_response_bytes, 1024 * 1024);
    assert_eq!(config.commit_threshold, 5000);
}

#[test]
fn test_stage_id_consistency() {
    // Ensure the stage ID is consistent
    assert_eq!(StageId::SnapSync.as_str(), "SnapSync");
    assert!(StageId::SnapSync.is_downloading_stage());
}