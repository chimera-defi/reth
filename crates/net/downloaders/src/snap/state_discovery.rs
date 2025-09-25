//! State root discovery system for snap sync.

use alloy_primitives::B256;
use reth_network_p2p::snap::client::SnapClient;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tracing::*;

/// State root discovery system that queries peers for recent state roots
/// and selects a suitable one for snap sync.
#[derive(Debug)]
pub struct StateRootDiscovery<C: SnapClient> {
    /// Snap client for querying peers
    client: Arc<C>,
    /// Maximum number of peers to query
    max_peers: usize,
    /// Timeout for peer queries
    query_timeout: Duration,
    /// List of peers to query
    peers: Vec<String>,
    /// State roots reported by each peer (peer_id -> (state_root, block_number))
    peer_state_roots: HashMap<String, (B256, u64)>,
}

/// Configuration for state root discovery
#[derive(Debug, Clone)]
pub struct StateRootDiscoveryConfig {
    /// Maximum number of peers to query
    pub max_peers: usize,
    /// Timeout for peer queries
    pub query_timeout: Duration,
    /// Minimum age for state root (in blocks)
    pub min_age_blocks: u64,
    /// Maximum age for state root (in blocks)
    pub max_age_blocks: u64,
}

impl Default for StateRootDiscoveryConfig {
    fn default() -> Self {
        Self {
            max_peers: 10,
            query_timeout: Duration::from_secs(30),
            min_age_blocks: 7200,  // ~1 day (7200 blocks * 12 seconds)
            max_age_blocks: 50400, // ~1 week (50400 blocks * 12 seconds)
        }
    }
}

impl<C: SnapClient> StateRootDiscovery<C> {
    /// Create a new state root discovery system
    pub fn new(client: Arc<C>, max_peers: usize, query_timeout: Duration) -> Self {
        Self {
            client,
            max_peers,
            query_timeout,
            peers: Vec::new(),
            peer_state_roots: HashMap::new(),
        }
    }

    /// Create a new state root discovery system with configuration
    pub fn with_config(client: Arc<C>, config: StateRootDiscoveryConfig) -> Self {
        Self {
            client,
            max_peers: config.max_peers,
            query_timeout: config.query_timeout,
            peers: Vec::new(),
            peer_state_roots: HashMap::new(),
        }
    }

    /// Add a peer to query for state roots
    pub fn add_peer(&mut self, peer_id: String) {
        if self.peers.len() < self.max_peers {
            self.peers.push(peer_id);
            info!(target: "snap_sync::state_discovery", peer_id = %self.peers.last().unwrap(), "Added peer for state root discovery");
        } else {
            warn!(target: "snap_sync::state_discovery", peer_id = %peer_id, "Maximum number of peers reached, ignoring new peer");
        }
    }

    /// Remove a peer from the discovery system
    pub fn remove_peer(&mut self, peer_id: &str) {
        self.peers.retain(|p| p != peer_id);
        self.peer_state_roots.remove(peer_id);
        info!(target: "snap_sync::state_discovery", peer_id = %peer_id, "Removed peer from state root discovery");
    }

    /// Update the state root reported by a peer
    pub fn update_peer_state_root(&mut self, peer_id: &str, state_root: B256, block_number: u64) {
        self.peer_state_roots.insert(peer_id.to_string(), (state_root, block_number));
        debug!(target: "snap_sync::state_discovery", 
            peer_id = %peer_id, 
            state_root = ?state_root, 
            block_number = block_number,
            "Updated peer state root"
        );
    }

    /// Query all peers for their latest state roots
    pub async fn query_peers_for_state_roots(&mut self) -> Result<Vec<(B256, u64)>, Box<dyn std::error::Error>> {
        info!(target: "snap_sync::state_discovery", peer_count = self.peers.len(), "Querying peers for state roots");
        
        let mut state_roots = Vec::new();
        
        for peer_id in &self.peers.clone() {
            match self.query_peer_for_state_root(peer_id).await {
                Ok((state_root, block_number)) => {
                    self.update_peer_state_root(peer_id, state_root, block_number);
                    state_roots.push((state_root, block_number));
                    info!(target: "snap_sync::state_discovery", 
                        peer_id = %peer_id, 
                        state_root = ?state_root, 
                        block_number = block_number,
                        "Successfully queried peer for state root"
                    );
                }
                Err(e) => {
                    warn!(target: "snap_sync::state_discovery", 
                        peer_id = %peer_id, 
                        error = ?e,
                        "Failed to query peer for state root"
                    );
                }
            }
        }
        
        info!(target: "snap_sync::state_discovery", 
            queried_peers = self.peers.len(),
            successful_queries = state_roots.len(),
            "Completed peer state root queries"
        );
        
        Ok(state_roots)
    }

    /// Query a specific peer for its latest state root
    async fn query_peer_for_state_root(&self, peer_id: &str) -> Result<(B256, u64), Box<dyn std::error::Error>> {
        // In a real implementation, this would:
        // 1. Send a request to the peer asking for its latest state root
        // 2. Wait for the response with a timeout
        // 3. Parse the response and return the state root and block number
        
        // For now, we'll simulate this with a mock implementation
        // In practice, this would use the snap client to query the peer
        
        // Simulate network delay
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Mock response - in reality this would come from the peer
        let state_root = B256::from_slice(&[1u8; 32]);
        let block_number = self.get_current_block_number().await?;
        
        Ok((state_root, block_number))
    }

    /// Get the current block number (mock implementation)
    async fn get_current_block_number(&self) -> Result<u64, Box<dyn std::error::Error>> {
        // In a real implementation, this would query the network for the current block number
        // For now, we'll simulate it
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
        let block_number = now.as_secs() / 12; // Approximate block number (12 second blocks)
        Ok(block_number)
    }

    /// Select a suitable recent state root for snap sync
    pub fn select_recent_state_root(&self) -> Option<(B256, u64)> {
        if self.peer_state_roots.is_empty() {
            warn!(target: "snap_sync::state_discovery", "No peer state roots available for selection");
            return None;
        }

        let current_block = self.get_current_block_number_sync();
        let min_age_blocks = 7200;  // ~1 day
        let max_age_blocks = 50400; // ~1 week

        let mut suitable_peers: Vec<_> = self.peer_state_roots
            .iter()
            .filter(|(_, (_, block_number))| {
                let age = current_block.saturating_sub(*block_number);
                age >= min_age_blocks && age <= max_age_blocks
            })
            .collect();

        if suitable_peers.is_empty() {
            warn!(target: "snap_sync::state_discovery", 
                current_block = current_block,
                min_age = min_age_blocks,
                max_age = max_age_blocks,
                "No suitable state roots found (age requirements not met)"
            );
            return None;
        }

        // Sort by block number (most recent first)
        suitable_peers.sort_by(|a, b| b.1.1.cmp(&a.1.1));

        // Select the most recent suitable state root
        let (peer_id, (state_root, block_number)) = suitable_peers[0];
        
        info!(target: "snap_sync::state_discovery", 
            peer_id = %peer_id,
            state_root = ?state_root,
            block_number = block_number,
            age_blocks = current_block - block_number,
            "Selected state root for snap sync"
        );

        Some((*state_root, *block_number))
    }

    /// Get current block number synchronously (for use in selection)
    fn get_current_block_number_sync(&self) -> u64 {
        // In a real implementation, this would be cached or retrieved synchronously
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        now.as_secs() / 12 // Approximate block number (12 second blocks)
    }

    /// Validate a state root (mock implementation)
    pub fn validate_state_root(&self, state_root: B256, block_number: u64) -> bool {
        // In a real implementation, this would:
        // 1. Query multiple peers to verify the state root
        // 2. Check if the state root is consistent across peers
        // 3. Verify the state root against known good roots
        
        // For now, we'll simulate validation
        // A real implementation would be more sophisticated
        
        // Basic validation: check if state root is not zero and block number is reasonable
        !state_root.is_zero() && block_number > 0
    }

    /// Get the number of peers
    pub fn get_peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Get all peer state roots
    pub fn get_peer_state_roots(&self) -> &HashMap<String, (B256, u64)> {
        &self.peer_state_roots
    }

    /// Clear all peers and state roots
    pub fn clear_peers(&mut self) {
        self.peers.clear();
        self.peer_state_roots.clear();
        info!(target: "snap_sync::state_discovery", "Cleared all peers and state roots");
    }

    /// Get discovery statistics
    pub fn get_stats(&self) -> StateRootDiscoveryStats {
        StateRootDiscoveryStats {
            total_peers: self.peers.len(),
            peers_with_state_roots: self.peer_state_roots.len(),
            max_peers: self.max_peers,
            query_timeout: self.query_timeout,
        }
    }
}

/// Statistics for state root discovery
#[derive(Debug, Clone)]
pub struct StateRootDiscoveryStats {
    /// Total number of peers
    pub total_peers: usize,
    /// Number of peers that have reported state roots
    pub peers_with_state_roots: usize,
    /// Maximum number of peers
    pub max_peers: usize,
    /// Query timeout
    pub query_timeout: Duration,
}

/// Trait for state root discovery
pub trait StateRootDiscoveryTrait {
    /// Add a peer to query for state roots
    fn add_peer(&mut self, peer_id: String);
    
    /// Remove a peer from the discovery system
    fn remove_peer(&mut self, peer_id: &str);
    
    /// Query all peers for their latest state roots
    fn query_peers_for_state_roots(&mut self) -> impl std::future::Future<Output = Result<Vec<(B256, u64)>, Box<dyn std::error::Error>>> + Send;
    
    /// Select a suitable recent state root for snap sync
    fn select_recent_state_root(&self) -> Option<(B256, u64)>;
    
    /// Validate a state root
    fn validate_state_root(&self, state_root: B256, block_number: u64) -> bool;
    
    /// Get discovery statistics
    fn get_stats(&self) -> StateRootDiscoveryStats;
}

impl<C: SnapClient> StateRootDiscoveryTrait for StateRootDiscovery<C> {
    fn add_peer(&mut self, peer_id: String) {
        self.add_peer(peer_id)
    }

    fn remove_peer(&mut self, peer_id: &str) {
        self.remove_peer(peer_id)
    }

    async fn query_peers_for_state_roots(&mut self) -> Result<Vec<(B256, u64)>, Box<dyn std::error::Error>> {
        self.query_peers_for_state_roots().await
    }

    fn select_recent_state_root(&self) -> Option<(B256, u64)> {
        self.select_recent_state_root()
    }

    fn validate_state_root(&self, state_root: B256, block_number: u64) -> bool {
        self.validate_state_root(state_root, block_number)
    }

    fn get_stats(&self) -> StateRootDiscoveryStats {
        self.get_stats()
    }
}