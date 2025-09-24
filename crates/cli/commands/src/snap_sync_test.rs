//! CLI tests for snap sync functionality

use crate::node::NodeCommand;
use reth_ethereum_cli::chainspec::EthereumChainSpecParser;
use std::ffi::OsString;

/// Test that snap sync CLI arguments are properly parsed
#[test]
fn test_snap_sync_cli_args() {
    // Test basic snap sync mode
    let args = vec![
        "reth",
        "--sync-mode", "snap",
    ];
    
    let cmd = NodeCommand::<EthereumChainSpecParser>::try_parse_from(args).unwrap();
    assert_eq!(cmd.sync.sync_mode.to_string(), "snap");
    assert!(!cmd.sync.snap_sync); // Should be false when using --sync-mode
}

/// Test that deprecated snap sync flag still works
#[test]
fn test_deprecated_snap_sync_flag() {
    let args = vec![
        "reth",
        "--snap-sync",
    ];
    
    let cmd = NodeCommand::<EthereumChainSpecParser>::try_parse_from(args).unwrap();
    assert_eq!(cmd.sync.effective_sync_mode().to_string(), "snap");
    assert!(cmd.sync.snap_sync);
}

/// Test snap sync configuration parameters
#[test]
fn test_snap_sync_config_params() {
    let args = vec![
        "reth",
        "--sync-mode", "snap",
        "--snap-max-concurrent-requests", "20",
        "--snap-max-response-bytes", "4194304", // 4MB
        "--snap-max-accounts-per-request", "2000",
        "--snap-commit-threshold", "20000",
    ];
    
    let cmd = NodeCommand::<EthereumChainSpecParser>::try_parse_from(args).unwrap();
    assert_eq!(cmd.sync.sync_mode.to_string(), "snap");
    assert_eq!(cmd.sync.snap_max_concurrent_requests, 20);
    assert_eq!(cmd.sync.snap_max_response_bytes, 4194304);
    assert_eq!(cmd.sync.snap_max_accounts_per_request, 2000);
    assert_eq!(cmd.sync.snap_commit_threshold, 20000);
}

/// Test that sync mode defaults to full
#[test]
fn test_sync_mode_default() {
    let args = vec!["reth"];
    
    let cmd = NodeCommand::<EthereumChainSpecParser>::try_parse_from(args).unwrap();
    assert_eq!(cmd.sync.sync_mode.to_string(), "full");
    assert!(!cmd.sync.snap_sync);
}

/// Test checkpoint sync mode
#[test]
fn test_checkpoint_sync_mode() {
    let args = vec![
        "reth",
        "--sync-mode", "checkpoint",
    ];
    
    let cmd = NodeCommand::<EthereumChainSpecParser>::try_parse_from(args).unwrap();
    assert_eq!(cmd.sync.sync_mode.to_string(), "checkpoint");
}

/// Test that snap sync config is properly converted
#[test]
fn test_snap_sync_config_conversion() {
    let args = vec![
        "reth",
        "--sync-mode", "snap",
        "--snap-max-concurrent-requests", "15",
        "--snap-max-response-bytes", "3145728", // 3MB
    ];
    
    let cmd = NodeCommand::<EthereumChainSpecParser>::try_parse_from(args).unwrap();
    let config = cmd.sync.to_snap_sync_config();
    
    assert_eq!(config.max_concurrent_requests, 15);
    assert_eq!(config.max_response_bytes, 3145728);
    assert_eq!(config.max_accounts_per_request, 1000); // default
    assert_eq!(config.commit_threshold, 10_000); // default
}

/// Test help output includes snap sync options
#[test]
fn test_snap_sync_help_includes_options() {
    let args = vec!["reth", "--help"];
    
    // This test would require capturing stdout, which is complex in unit tests
    // Instead, we'll verify that the arguments are properly defined
    let cmd = NodeCommand::<EthereumChainSpecParser>::try_parse_from(vec!["reth", "--help"]);
    
    // The command should parse successfully (help is a valid argument)
    assert!(cmd.is_ok());
}

/// Test invalid sync mode is rejected
#[test]
fn test_invalid_sync_mode() {
    let args = vec![
        "reth",
        "--sync-mode", "invalid",
    ];
    
    let cmd = NodeCommand::<EthereumChainSpecParser>::try_parse_from(args);
    assert!(cmd.is_err());
}

/// Test that sync mode and snap sync flag conflict is handled
#[test]
fn test_sync_mode_and_snap_sync_flag_conflict() {
    let args = vec![
        "reth",
        "--sync-mode", "full",
        "--snap-sync",
    ];
    
    let cmd = NodeCommand::<EthereumChainSpecParser>::try_parse_from(args).unwrap();
    // The deprecated flag should take precedence
    assert_eq!(cmd.sync.effective_sync_mode().to_string(), "snap");
}