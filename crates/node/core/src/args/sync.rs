//! Sync mode arguments for the node

use clap::{Args, ValueEnum};
use reth_config::config::SnapSyncConfig;
use std::fmt;

/// Sync mode for the node
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SyncMode {
    /// Full sync - download and verify all blocks from genesis
    Full,
    /// Snap sync - fast state synchronization using snap protocol
    Snap,
    /// Checkpoint sync - sync from a trusted checkpoint
    Checkpoint,
}

impl fmt::Display for SyncMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncMode::Full => write!(f, "full"),
            SyncMode::Snap => write!(f, "snap"),
            SyncMode::Checkpoint => write!(f, "checkpoint"),
        }
    }
}

/// Arguments for configuring sync behavior
#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Sync mode to use
    #[arg(
        long,
        value_enum,
        default_value = "full",
        help = "Sync mode: full (download all blocks), snap (fast state sync), or checkpoint (sync from trusted checkpoint)"
    )]
    pub sync_mode: SyncMode,

    /// Enable snap sync for fast state synchronization
    #[arg(
        long,
        help = "Enable snap sync for fast state synchronization (deprecated: use --sync-mode snap)"
    )]
    pub snap_sync: bool,

    /// Maximum number of concurrent snap sync requests
    #[arg(
        long,
        default_value = "10",
        help = "Maximum number of concurrent snap sync requests"
    )]
    pub snap_max_concurrent_requests: usize,

    /// Maximum response size for snap sync requests (in bytes)
    #[arg(
        long,
        default_value = "2097152",
        help = "Maximum response size for snap sync requests in bytes (default: 2MB)"
    )]
    pub snap_max_response_bytes: u64,

    /// Maximum number of accounts per snap sync request
    #[arg(
        long,
        default_value = "1000",
        help = "Maximum number of accounts per snap sync request"
    )]
    pub snap_max_accounts_per_request: u64,

    /// Maximum number of storage slots per snap sync request
    #[arg(
        long,
        default_value = "1000",
        help = "Maximum number of storage slots per snap sync request"
    )]
    pub snap_max_storage_slots_per_request: u64,

    /// Maximum number of byte codes per snap sync request
    #[arg(
        long,
        default_value = "100",
        help = "Maximum number of byte codes per snap sync request"
    )]
    pub snap_max_byte_codes_per_request: u64,

    /// Maximum number of trie nodes per snap sync request
    #[arg(
        long,
        default_value = "1000",
        help = "Maximum number of trie nodes per snap sync request"
    )]
    pub snap_max_trie_nodes_per_request: u64,

    /// Commit threshold for snap sync (number of items before committing)
    #[arg(
        long,
        default_value = "10000",
        help = "Number of snap sync items to process before committing progress"
    )]
    pub snap_commit_threshold: u64,
}

impl SyncArgs {
    /// Get the effective sync mode, considering both sync_mode and the deprecated snap_sync flag
    pub fn effective_sync_mode(&self) -> SyncMode {
        if self.snap_sync {
            SyncMode::Snap
        } else {
            self.sync_mode
        }
    }

    /// Convert to SnapSyncConfig
    pub fn to_snap_sync_config(&self) -> SnapSyncConfig {
        SnapSyncConfig {
            max_concurrent_requests: self.snap_max_concurrent_requests,
            max_response_bytes: self.snap_max_response_bytes,
            max_accounts_per_request: self.snap_max_accounts_per_request,
            max_storage_slots_per_request: self.snap_max_storage_slots_per_request,
            max_byte_codes_per_request: self.snap_max_byte_codes_per_request,
            max_trie_nodes_per_request: self.snap_max_trie_nodes_per_request,
            commit_threshold: self.snap_commit_threshold,
        }
    }
}

impl Default for SyncArgs {
    fn default() -> Self {
        Self {
            sync_mode: SyncMode::Full,
            snap_sync: false,
            snap_max_concurrent_requests: 10,
            snap_max_response_bytes: 2 * 1024 * 1024, // 2MB
            snap_max_accounts_per_request: 1000,
            snap_max_storage_slots_per_request: 1000,
            snap_max_byte_codes_per_request: 100,
            snap_max_trie_nodes_per_request: 1000,
            snap_commit_threshold: 10_000,
        }
    }
}