//! Essential integration tests for the snap sync stage.

use super::SnapSyncStage;
use crate::test_utils::TestStageDB;
use reth_config::config::{EtlConfig, SnapSyncConfig};
use reth_network_downloaders::snap::test_utils::TestSnapClient;
use reth_stages_api::{ExecInput, Stage, StageId};
use std::sync::Arc;

#[tokio::test]
async fn test_snap_sync_stage_basic() {
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
    
    let result = stage.execute(&provider, input);
    assert!(result.is_ok());
    assert!(result.unwrap().done);
}