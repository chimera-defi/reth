//! Snap sync downloader implementation.
//!
//! This module provides the snap sync downloader that can download Ethereum state
//! snapshots using the snap protocol. It supports downloading account ranges,
//! storage ranges, byte codes, and trie nodes.

pub mod downloader;
pub mod task;
pub mod queue;
pub mod server;
pub mod state_manager;
pub mod peer_manager;
pub mod progress_reporter;
pub mod state_discovery;
pub mod state_verifier;
pub mod state_healer;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

#[cfg(test)]
mod tests;

pub use downloader::SnapSyncDownloader;
pub use server::{SnapSyncServer, SnapSyncServerTrait};
pub use state_manager::{SnapSyncStateManager, StateRootManager, SyncProgress, DataType, StateRootInfo};
pub use peer_manager::{SnapSyncPeerManager, PeerManager, PeerSelectionStrategy, PeerStats, SnapSyncPeer, PeerMetrics};
pub use progress_reporter::{SnapSyncProgressReporter, ProgressReporter, ProgressReport, DataProgress, SyncStats};
pub use state_discovery::{StateRootDiscovery, StateRootDiscoveryTrait, StateRootDiscoveryConfig, StateRootDiscoveryStats};
pub use state_verifier::{StateVerifier, StateVerificationTrait, StateVerificationConfig, VerificationResult, VerificationStats, AccountRange, StorageRange, StateData, AccountState};
pub use state_healer::{StateHealer, StateHealingTrait, StateHealingConfig, MissingData, HealingResult, HealingStats};