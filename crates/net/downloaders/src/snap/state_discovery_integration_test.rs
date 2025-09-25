//! Integration test for state root discovery system.

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::snap::test_utils::TestSnapClient;
    use alloy_primitives::B256;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_state_root_discovery_integration() {
        // Create a test client
        let client = Arc::new(TestSnapClient::new());
        
        // Create state discovery with configuration
        let config = StateRootDiscoveryConfig {
            max_peers: 5,
            query_timeout: std::time::Duration::from_secs(10),
            min_age_blocks: 7200,  // 1 day
            max_age_blocks: 50400, // 1 week
        };
        
        let mut discovery = StateRootDiscovery::with_config(client, config);
        
        // Add some test peers
        discovery.add_peer("peer1".to_string());
        discovery.add_peer("peer2".to_string());
        discovery.add_peer("peer3".to_string());
        
        // Verify peers were added
        assert_eq!(discovery.get_peer_count(), 3);
        
        // Query peers for state roots
        let state_roots = discovery.query_peers_for_state_roots().await;
        assert!(state_roots.is_ok());
        
        // Select a recent state root
        let selected = discovery.select_recent_state_root();
        assert!(selected.is_some());
        
        let (state_root, block_number) = selected.unwrap();
        assert!(!state_root.is_zero());
        assert!(block_number > 0);
        
        // Verify state root validation
        let is_valid = discovery.validate_state_root(state_root, block_number);
        assert!(is_valid);
        
        // Check statistics
        let stats = discovery.get_stats();
        assert_eq!(stats.total_peers, 3);
        assert_eq!(stats.max_peers, 5);
    }

    #[tokio::test]
    async fn test_state_root_discovery_peer_management() {
        let client = Arc::new(TestSnapClient::new());
        let mut discovery = StateRootDiscovery::new(client, 3, std::time::Duration::from_secs(5));
        
        // Add peers
        discovery.add_peer("peer1".to_string());
        discovery.add_peer("peer2".to_string());
        discovery.add_peer("peer3".to_string());
        
        assert_eq!(discovery.get_peer_count(), 3);
        
        // Try to add more peers than max
        discovery.add_peer("peer4".to_string());
        assert_eq!(discovery.get_peer_count(), 3); // Should not exceed max
        
        // Remove a peer
        discovery.remove_peer("peer2");
        assert_eq!(discovery.get_peer_count(), 2);
        
        // Clear all peers
        discovery.clear_peers();
        assert_eq!(discovery.get_peer_count(), 0);
    }

    #[tokio::test]
    async fn test_state_root_discovery_state_root_selection() {
        let client = Arc::new(TestSnapClient::new());
        let mut discovery = StateRootDiscovery::new(client, 5, std::time::Duration::from_secs(5));
        
        // Add peers with different state roots
        discovery.add_peer("peer1".to_string());
        discovery.add_peer("peer2".to_string());
        discovery.add_peer("peer3".to_string());
        
        // Update peer state roots with different ages
        let current_block = 18000000u64;
        
        // Peer 1: Very recent (too recent)
        discovery.update_peer_state_root("peer1", B256::from([1u8; 32]), current_block);
        
        // Peer 2: Good age (1 day old)
        let one_day_ago = current_block - 7200; // 1 day in blocks
        discovery.update_peer_state_root("peer2", B256::from([2u8; 32]), one_day_ago);
        
        // Peer 3: Too old (1 week old)
        let one_week_ago = current_block - 50400; // 1 week in blocks
        discovery.update_peer_state_root("peer3", B256::from([3u8; 32]), one_week_ago);
        
        // Should select peer 2 (good age)
        let selected = discovery.select_recent_state_root();
        assert!(selected.is_some());
        
        let (state_root, block_number) = selected.unwrap();
        assert_eq!(state_root, B256::from([2u8; 32]));
        assert_eq!(block_number, one_day_ago);
    }

    #[tokio::test]
    async fn test_state_root_discovery_no_suitable_peers() {
        let client = Arc::new(TestSnapClient::new());
        let mut discovery = StateRootDiscovery::new(client, 5, std::time::Duration::from_secs(5));
        
        // Add peer with very recent state root (too recent)
        discovery.add_peer("peer1".to_string());
        let current_block = 18000000u64;
        discovery.update_peer_state_root("peer1", B256::from([1u8; 32]), current_block);
        
        // Should return None (no suitable peer)
        let selected = discovery.select_recent_state_root();
        assert!(selected.is_none());
    }

    #[tokio::test]
    async fn test_state_root_discovery_configuration() {
        let client = Arc::new(TestSnapClient::new());
        
        let config = StateRootDiscoveryConfig {
            max_peers: 10,
            query_timeout: std::time::Duration::from_secs(30),
            min_age_blocks: 3600,  // 30 minutes
            max_age_blocks: 7200,  // 1 day
        };
        
        let discovery = StateRootDiscovery::with_config(client, config);
        let stats = discovery.get_stats();
        
        assert_eq!(stats.max_peers, 10);
        assert_eq!(stats.query_timeout, std::time::Duration::from_secs(30));
        assert_eq!(stats.total_peers, 0);
        assert_eq!(stats.peers_with_state_roots, 0);
    }
}