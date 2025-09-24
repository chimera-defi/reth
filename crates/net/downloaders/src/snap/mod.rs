//! Snap sync downloader implementation.
//!
//! This module provides the snap sync downloader that can download Ethereum state
//! snapshots using the snap protocol. It supports downloading account ranges,
//! storage ranges, byte codes, and trie nodes.

pub mod downloader;
pub mod task;
pub mod queue;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

#[cfg(test)]
mod tests;

pub use downloader::SnapSyncDownloader;