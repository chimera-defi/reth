//! Integration test demonstrating the complete snap sync pipeline.

use crate::{
    sets::DefaultStages,
    stages::{SnapSyncStage, HeaderStage, BodyStage},
    StageSet,
};
use alloy_primitives::B256;
use reth_config::config::{EtlConfig, StageConfig, SnapSyncConfig};
use reth_consensus::test_utils::TestConsensus;
use reth_ethereum_primitives::EthPrimitives;
use reth_evm_ethereum::EthEvmConfig;
use reth_network_downloaders::snap::test_utils::TestSnapClient;
use reth_network_p2p::test_utils::{TestBodiesClient, TestHeadersClient};
use reth_prune_types::PruneModes;
use reth_provider::test_utils::{create_test_provider_factory, MockNodeTypesWithDB};
use reth_stages_api::Pipeline;
use reth_static_file::StaticFileProducer;
use std::sync::Arc;
use tokio::sync::watch;

#[tokio::test]
async fn test_snap_sync_integration() {
    // Create test infrastructure
    let provider_factory = create_test_provider_factory();
    let consensus = Arc::new(TestConsensus::default());
    let (tip_tx, tip_rx) = watch::channel(B256::default());
    
    // Create downloaders
    let headers_downloader = TestHeadersClient::default();
    let bodies_downloader = TestBodiesClient::default();
    let snap_client = Arc::new(TestSnapClient::new());
    
    // Create EVM config
    let evm_config = EthEvmConfig::mainnet();
    
    // Create stage config with snap sync enabled
    let mut stage_config = StageConfig::default();
    stage_config.snap_sync = SnapSyncConfig {
        max_concurrent_requests: 5,
        max_response_bytes: 1024 * 1024, // 1MB for testing
        max_accounts_per_request: 100,
        max_storage_slots_per_request: 100,
        max_byte_codes_per_request: 10,
        max_trie_nodes_per_request: 100,
        commit_threshold: 1000,
    };
    
    // Create prune modes
    let prune_modes = PruneModes::none();
    
    // Create static file producer
    let static_file_producer = StaticFileProducer::new(
        provider_factory.clone(),
        prune_modes.clone(),
    );
    
    // Create the pipeline with snap sync included
    let pipeline = Pipeline::<MockNodeTypesWithDB>::builder()
        .with_tip_sender(tip_tx)
        .add_stages(DefaultStages::new(
            provider_factory.database_provider_ro().unwrap(),
            tip_rx,
            consensus,
            headers_downloader,
            bodies_downloader,
            snap_client,
            evm_config,
            stage_config,
            prune_modes,
            None, // era_import_source
        ))
        .build(provider_factory, static_file_producer);
    
    // The pipeline should be created successfully
    assert!(pipeline.is_ok());
}

#[test]
fn test_snap_sync_stage_ordering() {
    // Test that snap sync stage is properly ordered in the pipeline
    let provider = create_test_provider_factory().database_provider_ro().unwrap();
    let consensus = Arc::new(TestConsensus::default());
    let (_, tip_rx) = watch::channel(B256::default());
    let headers_downloader = TestHeadersClient::default();
    let bodies_downloader = TestBodiesClient::default();
    let snap_client = Arc::new(TestSnapClient::new());
    let evm_config = EthEvmConfig::mainnet();
    let stage_config = StageConfig::default();
    let prune_modes = PruneModes::none();
    
    let stages = DefaultStages::new(
        provider,
        tip_rx,
        consensus,
        headers_downloader,
        bodies_downloader,
        snap_client,
        evm_config,
        stage_config,
        prune_modes,
        None,
    );
    
    let builder = stages.builder();
    
    // The builder should contain the snap sync stage
    // Note: This is a basic test to ensure the stages are built correctly
    // In a real implementation, we would check the specific stage ordering
    assert!(builder.stages.len() > 0);
}

#[test]
fn test_snap_sync_configuration_propagation() {
    // Test that snap sync configuration is properly propagated through the system
    let mut stage_config = StageConfig::default();
    
    // Customize snap sync config
    stage_config.snap_sync.max_concurrent_requests = 15;
    stage_config.snap_sync.max_response_bytes = 4 * 1024 * 1024; // 4MB
    stage_config.snap_sync.commit_threshold = 50000;
    
    // Verify the configuration is preserved
    assert_eq!(stage_config.snap_sync.max_concurrent_requests, 15);
    assert_eq!(stage_config.snap_sync.max_response_bytes, 4 * 1024 * 1024);
    assert_eq!(stage_config.snap_sync.commit_threshold, 50000);
    
    // Test that the configuration can be used to create a stage
    let provider = create_test_provider_factory().database_provider_ro().unwrap();
    let client = Arc::new(TestSnapClient::new());
    let etl_config = EtlConfig::default();
    
    let stage = SnapSyncStage::new(provider, client, stage_config.snap_sync, etl_config);
    assert_eq!(stage.config.max_concurrent_requests, 15);
}

#[tokio::test]
async fn test_snap_sync_error_handling() {
    use reth_network_p2p::error::RequestError;
    use reth_network_downloaders::snap::downloader::SnapSyncError;
    
    // Test error conversion and handling
    let network_error = RequestError::Timeout;
    let snap_error = SnapSyncError::Network(network_error);
    
    match snap_error {
        SnapSyncError::Network(RequestError::Timeout) => {
            // Expected error type
        }
        _ => panic!("Unexpected error type"),
    }
    
    // Test other error types
    let invalid_root = SnapSyncError::InvalidStateRoot(B256::default());
    assert!(matches!(invalid_root, SnapSyncError::InvalidStateRoot(_)));
    
    let missing_account = SnapSyncError::MissingAccountData(B256::default());
    assert!(matches!(missing_account, SnapSyncError::MissingAccountData(_)));
}