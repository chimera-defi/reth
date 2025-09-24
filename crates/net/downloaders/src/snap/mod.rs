//! Snap sync downloader implementation
//!
//! This module provides functionality for downloading Ethereum state data
//! using the SNAP (State Network Access Protocol) which allows for faster
//! state synchronization by downloading state snapshots instead of computing
//! the state from block execution.

pub mod client;
pub mod downloader;
pub mod example;

pub use client::{SnapSyncClient, SnapSyncError};
pub use downloader::{DownloadStats, SnapSyncConfig, SnapSyncDownloader};
pub use example::{example_snap_sync_usage, ExampleSnapSyncClient};