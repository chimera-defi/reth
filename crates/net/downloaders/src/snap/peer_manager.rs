//! Peer management for snap sync operations.

use alloy_primitives::B256;
use reth_network_p2p::{
    error::PeerRequestResult,
    snap::client::SnapClient,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::*;

/// Information about a snap sync peer
#[derive(Debug, Clone)]
pub struct SnapSyncPeer {
    /// Peer ID
    pub peer_id: String,
    /// Whether the peer supports snap sync
    pub supports_snap_sync: bool,
    /// Peer's current state root
    pub state_root: Option<B256>,
    /// Peer's block number
    pub block_number: Option<u64>,
    /// Peer's performance metrics
    pub metrics: PeerMetrics,
    /// Last successful request time
    pub last_success: Option<Instant>,
    /// Last failure time
    pub last_failure: Option<Instant>,
    /// Number of consecutive failures
    pub consecutive_failures: u32,
    /// Whether the peer is currently available
    pub is_available: bool,
}

/// Performance metrics for a peer
#[derive(Debug, Clone, Default)]
pub struct PeerMetrics {
    /// Total requests made to this peer
    pub total_requests: u64,
    /// Successful requests
    pub successful_requests: u64,
    /// Failed requests
    pub failed_requests: u64,
    /// Average response time
    pub avg_response_time: Duration,
    /// Total bytes downloaded
    pub total_bytes_downloaded: u64,
    /// Last request time
    pub last_request_time: Option<Instant>,
}

impl PeerMetrics {
    /// Calculate success rate
    pub fn success_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.successful_requests as f64 / self.total_requests as f64
        }
    }

    /// Update metrics after a successful request
    pub fn record_success(&mut self, response_time: Duration, bytes_downloaded: u64) {
        self.total_requests += 1;
        self.successful_requests += 1;
        self.total_bytes_downloaded += bytes_downloaded;
        self.last_request_time = Some(Instant::now());
        
        // Update average response time
        if self.avg_response_time.is_zero() {
            self.avg_response_time = response_time;
        } else {
            // Simple moving average
            self.avg_response_time = Duration::from_millis(
                (self.avg_response_time.as_millis() + response_time.as_millis()) / 2
            );
        }
    }

    /// Update metrics after a failed request
    pub fn record_failure(&mut self) {
        self.total_requests += 1;
        self.failed_requests += 1;
        self.last_request_time = Some(Instant::now());
    }
}

/// Manages snap sync peers
#[derive(Debug)]
pub struct SnapSyncPeerManager<C: SnapClient> {
    /// Available snap sync peers
    peers: HashMap<String, SnapSyncPeer>,
    /// Peer selection strategy
    selection_strategy: PeerSelectionStrategy,
    /// Maximum number of peers to use
    max_peers: usize,
    /// Minimum success rate for peer selection
    min_success_rate: f64,
    /// Maximum consecutive failures before marking peer as unavailable
    max_consecutive_failures: u32,
    /// Timeout for peer availability
    peer_timeout: Duration,
}

/// Strategy for selecting peers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerSelectionStrategy {
    /// Select peer with highest success rate
    BestPerformance,
    /// Select peer with lowest response time
    FastestResponse,
    /// Round-robin selection
    RoundRobin,
    /// Random selection
    Random,
}

impl<C: SnapClient> SnapSyncPeerManager<C> {
    /// Create a new peer manager
    pub fn new(
        selection_strategy: PeerSelectionStrategy,
        max_peers: usize,
        min_success_rate: f64,
        max_consecutive_failures: u32,
        peer_timeout: Duration,
    ) -> Self {
        Self {
            peers: HashMap::new(),
            selection_strategy,
            max_peers,
            min_success_rate,
            max_consecutive_failures,
            peer_timeout,
        }
    }

    /// Add a new peer
    pub fn add_peer(&mut self, peer_id: String, supports_snap_sync: bool) {
        info!(target: "snap_sync::peer_manager", peer_id = %peer_id, "Adding snap sync peer");
        
        let peer = SnapSyncPeer {
            peer_id: peer_id.clone(),
            supports_snap_sync,
            state_root: None,
            block_number: None,
            metrics: PeerMetrics::default(),
            last_success: None,
            last_failure: None,
            consecutive_failures: 0,
            is_available: true,
        };

        self.peers.insert(peer_id, peer);
    }

    /// Remove a peer
    pub fn remove_peer(&mut self, peer_id: &str) {
        info!(target: "snap_sync::peer_manager", peer_id = %peer_id, "Removing snap sync peer");
        self.peers.remove(peer_id);
    }

    /// Update peer information
    pub fn update_peer_info(&mut self, peer_id: &str, state_root: Option<B256>, block_number: Option<u64>) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.state_root = state_root;
            peer.block_number = block_number;
            
            info!(target: "snap_sync::peer_manager", 
                peer_id = %peer_id, 
                state_root = ?state_root,
                block_number = ?block_number,
                "Updated peer information"
            );
        }
    }

    /// Record a successful request to a peer
    pub fn record_success(&mut self, peer_id: &str, response_time: Duration, bytes_downloaded: u64) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.metrics.record_success(response_time, bytes_downloaded);
            peer.last_success = Some(Instant::now());
            peer.consecutive_failures = 0;
            peer.is_available = true;
            
            debug!(target: "snap_sync::peer_manager",
                peer_id = %peer_id,
                response_time_ms = response_time.as_millis(),
                bytes_downloaded = bytes_downloaded,
                success_rate = peer.metrics.success_rate(),
                "Recorded successful request"
            );
        }
    }

    /// Record a failed request to a peer
    pub fn record_failure(&mut self, peer_id: &str) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.metrics.record_failure();
            peer.last_failure = Some(Instant::now());
            peer.consecutive_failures += 1;
            
            // Mark peer as unavailable if too many consecutive failures
            if peer.consecutive_failures >= self.max_consecutive_failures {
                peer.is_available = false;
                warn!(target: "snap_sync::peer_manager",
                    peer_id = %peer_id,
                    consecutive_failures = peer.consecutive_failures,
                    "Marked peer as unavailable due to consecutive failures"
                );
            }
            
            debug!(target: "snap_sync::peer_manager",
                peer_id = %peer_id,
                consecutive_failures = peer.consecutive_failures,
                success_rate = peer.metrics.success_rate(),
                "Recorded failed request"
            );
        }
    }

    /// Select the best peer for a request
    pub fn select_peer(&self) -> Option<String> {
        let available_peers: Vec<_> = self.peers
            .values()
            .filter(|peer| {
                peer.is_available &&
                peer.supports_snap_sync &&
                peer.metrics.success_rate() >= self.min_success_rate
            })
            .collect();

        if available_peers.is_empty() {
            warn!(target: "snap_sync::peer_manager", "No available snap sync peers");
            return None;
        }

        let selected_peer = match self.selection_strategy {
            PeerSelectionStrategy::BestPerformance => {
                available_peers.iter()
                    .max_by(|a, b| a.metrics.success_rate().partial_cmp(&b.metrics.success_rate()).unwrap())
            }
            PeerSelectionStrategy::FastestResponse => {
                available_peers.iter()
                    .min_by(|a, b| a.metrics.avg_response_time.cmp(&b.metrics.avg_response_time))
            }
            PeerSelectionStrategy::RoundRobin => {
                // Simple round-robin based on peer ID hash
                available_peers.iter()
                    .min_by(|a, b| a.peer_id.cmp(&b.peer_id))
            }
            PeerSelectionStrategy::Random => {
                // For simplicity, just pick the first one
                available_peers.first()
            }
        };

        selected_peer.map(|peer| peer.peer_id.clone())
    }

    /// Get all available peers
    pub fn available_peers(&self) -> Vec<String> {
        self.peers
            .values()
            .filter(|peer| peer.is_available && peer.supports_snap_sync)
            .map(|peer| peer.peer_id.clone())
            .collect()
    }

    /// Get peer information
    pub fn get_peer_info(&self, peer_id: &str) -> Option<&SnapSyncPeer> {
        self.peers.get(peer_id)
    }

    /// Get peer statistics
    pub fn get_peer_stats(&self) -> PeerStats {
        let total_peers = self.peers.len();
        let available_peers = self.peers.values().filter(|peer| peer.is_available).count();
        let snap_sync_peers = self.peers.values().filter(|peer| peer.supports_snap_sync).count();
        
        let total_requests: u64 = self.peers.values().map(|peer| peer.metrics.total_requests).sum();
        let successful_requests: u64 = self.peers.values().map(|peer| peer.metrics.successful_requests).sum();
        let total_bytes: u64 = self.peers.values().map(|peer| peer.metrics.total_bytes_downloaded).sum();

        PeerStats {
            total_peers,
            available_peers,
            snap_sync_peers,
            total_requests,
            successful_requests,
            total_bytes_downloaded: total_bytes,
            overall_success_rate: if total_requests > 0 { successful_requests as f64 / total_requests as f64 } else { 0.0 },
        }
    }

    /// Clean up old peer data
    pub fn cleanup(&mut self) {
        let now = Instant::now();
        let mut to_remove = Vec::new();

        for (peer_id, peer) in &self.peers {
            // Remove peers that haven't been seen for too long
            if let Some(last_request) = peer.metrics.last_request_time {
                if now.duration_since(last_request) > self.peer_timeout {
                    to_remove.push(peer_id.clone());
                }
            }
        }

        for peer_id in to_remove {
            self.remove_peer(&peer_id);
        }
    }

    /// Reset peer availability (for retry logic)
    pub fn reset_peer_availability(&mut self) {
        for peer in self.peers.values_mut() {
            if !peer.is_available && peer.consecutive_failures > 0 {
                // Reset after some time
                if let Some(last_failure) = peer.last_failure {
                    if Instant::now().duration_since(last_failure) > Duration::from_secs(60) {
                        peer.is_available = true;
                        peer.consecutive_failures = 0;
                        info!(target: "snap_sync::peer_manager", peer_id = %peer.peer_id, "Reset peer availability");
                    }
                }
            }
        }
    }
}

/// Statistics for all peers
#[derive(Debug, Clone)]
pub struct PeerStats {
    /// Total number of peers
    pub total_peers: usize,
    /// Number of available peers
    pub available_peers: usize,
    /// Number of peers supporting snap sync
    pub snap_sync_peers: usize,
    /// Total requests made
    pub total_requests: u64,
    /// Total successful requests
    pub successful_requests: u64,
    /// Total bytes downloaded
    pub total_bytes_downloaded: u64,
    /// Overall success rate
    pub overall_success_rate: f64,
}

/// Trait for peer management
pub trait PeerManager {
    /// Add a new peer
    fn add_peer(&mut self, peer_id: String, supports_snap_sync: bool);
    
    /// Remove a peer
    fn remove_peer(&mut self, peer_id: &str);
    
    /// Select the best peer for a request
    fn select_peer(&self) -> Option<String>;
    
    /// Record a successful request
    fn record_success(&mut self, peer_id: &str, response_time: Duration, bytes_downloaded: u64);
    
    /// Record a failed request
    fn record_failure(&mut self, peer_id: &str);
    
    /// Get peer statistics
    fn get_peer_stats(&self) -> PeerStats;
}

impl<C: SnapClient> PeerManager for SnapSyncPeerManager<C> {
    fn add_peer(&mut self, peer_id: String, supports_snap_sync: bool) {
        self.add_peer(peer_id, supports_snap_sync)
    }

    fn remove_peer(&mut self, peer_id: &str) {
        self.remove_peer(peer_id)
    }

    fn select_peer(&self) -> Option<String> {
        self.select_peer()
    }

    fn record_success(&mut self, peer_id: &str, response_time: Duration, bytes_downloaded: u64) {
        self.record_success(peer_id, response_time, bytes_downloaded)
    }

    fn record_failure(&mut self, peer_id: &str) {
        self.record_failure(peer_id)
    }

    fn get_peer_stats(&self) -> PeerStats {
        self.get_peer_stats()
    }
}