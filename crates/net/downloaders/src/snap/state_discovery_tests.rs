//! Tests for state root discovery system.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snap::test_utils::TestSnapClient;
    use alloy_primitives::B256;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn test_state_root_discovery_creation() {
        let client = Arc::new(TestSnapClient::new());
        let discovery = StateRootDiscovery::new(client, 5, Duration::from_secs(30));
        
        assert_eq!(discovery.max_peers, 5);
        assert_eq!(discovery.query_timeout, Duration::from_secs(30));
        assert!(discovery.peer_state_roots.is_empty());
    }

    #[test]
    fn test_add_peer() {
        let client = Arc::new(TestSnapClient::new());
        let mut discovery = StateRootDiscovery::new(client, 5, Duration::from_secs(30));
        
        discovery.add_peer("peer1".to_string());
        discovery.add_peer("peer2".to_string());
        
        assert_eq!(discovery.peers.len(), 2);
        assert!(discovery.peers.contains(&"peer1".to_string()));
        assert!(discovery.peers.contains(&"peer2".to_string()));
    }

    #[test]
    fn test_remove_peer() {
        let client = Arc::new(TestSnapClient::new());
        let mut discovery = StateRootDiscovery::new(client, 5, Duration::from_secs(30));
        
        discovery.add_peer("peer1".to_string());
        discovery.add_peer("peer2".to_string());
        discovery.remove_peer("peer1");
        
        assert_eq!(discovery.peers.len(), 1);
        assert!(!discovery.peers.contains(&"peer1".to_string()));
        assert!(discovery.peers.contains(&"peer2".to_string()));
    }

    #[test]
    fn test_update_peer_state_root() {
        let client = Arc::new(TestSnapClient::new());
        let mut discovery = StateRootDiscovery::new(client, 5, Duration::from_secs(30));
        
        discovery.add_peer("peer1".to_string());
        let state_root = B256::from([1u8; 32]);
        let block_number = 18000000u64;
        
        discovery.update_peer_state_root("peer1", state_root, block_number);
        
        assert_eq!(discovery.peer_state_roots.get("peer1"), Some(&(state_root, block_number)));
    }

    #[test]
    fn test_select_recent_state_root() {
        let client = Arc::new(TestSnapClient::new());
        let mut discovery = StateRootDiscovery::new(client, 5, Duration::from_secs(30));
        
        // Add peers with different state roots
        discovery.add_peer("peer1".to_string());
        discovery.add_peer("peer2".to_string());
        discovery.add_peer("peer3".to_string());
        
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let block_number = now / 12; // Approximate block number (12 second blocks)
        
        // Peer 1: Very recent (too recent for snap sync)
        discovery.update_peer_state_root("peer1", B256::from([1u8; 32]), block_number);
        
        // Peer 2: Good age (1 day old)
        let one_day_ago = block_number - (24 * 60 * 60 / 12); // 1 day in blocks
        discovery.update_peer_state_root("peer2", B256::from([2u8; 32]), one_day_ago);
        
        // Peer 3: Too old (1 week old)
        let one_week_ago = block_number - (7 * 24 * 60 * 60 / 12); // 1 week in blocks
        discovery.update_peer_state_root("peer3", B256::from([3u8; 32]), one_week_ago);
        
        // Should select peer 2 (good age)
        let selected = discovery.select_recent_state_root();
        assert!(selected.is_some());
        let (state_root, block_number) = selected.unwrap();
        assert_eq!(state_root, B256::from([2u8; 32]));
        assert_eq!(block_number, one_day_ago);
    }

    #[test]
    fn test_select_recent_state_root_no_suitable_peers() {
        let client = Arc::new(TestSnapClient::new());
        let mut discovery = StateRootDiscovery::new(client, 5, Duration::from_secs(30));
        
        // Add peer with very recent state root (too recent)
        discovery.add_peer("peer1".to_string());
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let block_number = now / 12; // Very recent
        discovery.update_peer_state_root("peer1", B256::from([1u8; 32]), block_number);
        
        // Should return None (no suitable peer)
        let selected = discovery.select_recent_state_root();
        assert!(selected.is_none());
    }

    #[test]
    fn test_select_recent_state_root_no_peers() {
        let client = Arc::new(TestSnapClient::new());
        let discovery = StateRootDiscovery::new(client, 5, Duration::from_secs(30));
        
        // No peers added
        let selected = discovery.select_recent_state_root();
        assert!(selected.is_none());
    }

    #[test]
    fn test_validate_state_root() {
        let client = Arc::new(TestSnapClient::new());
        let mut discovery = StateRootDiscovery::new(client, 5, Duration::from_secs(30));
        
        let state_root = B256::from([1u8; 32]);
        let block_number = 18000000u64;
        
        // Should validate successfully (mock implementation)
        let is_valid = discovery.validate_state_root(state_root, block_number);
        assert!(is_valid);
    }

    #[tokio::test]
    async fn test_query_peers_for_state_roots() {
        let client = Arc::new(TestSnapClient::new());
        let mut discovery = StateRootDiscovery::new(client, 5, Duration::from_secs(30));
        
        discovery.add_peer("peer1".to_string());
        discovery.add_peer("peer2".to_string());
        
        // Mock the client to return state roots
        // In a real implementation, this would query the network
        let state_roots = discovery.query_peers_for_state_roots().await;
        assert!(state_roots.is_ok());
    }

    #[test]
    fn test_get_peer_count() {
        let client = Arc::new(TestSnapClient::new());
        let mut discovery = StateRootDiscovery::new(client, 5, Duration::from_secs(30));
        
        assert_eq!(discovery.get_peer_count(), 0);
        
        discovery.add_peer("peer1".to_string());
        assert_eq!(discovery.get_peer_count(), 1);
        
        discovery.add_peer("peer2".to_string());
        assert_eq!(discovery.get_peer_count(), 2);
    }

    #[test]
    fn test_get_peer_state_roots() {
        let client = Arc::new(TestSnapClient::new());
        let mut discovery = StateRootDiscovery::new(client, 5, Duration::from_secs(30));
        
        discovery.add_peer("peer1".to_string());
        discovery.add_peer("peer2".to_string());
        
        let state_root1 = B256::from([1u8; 32]);
        let state_root2 = B256::from([2u8; 32]);
        let block_number1 = 18000000u64;
        let block_number2 = 18000001u64;
        
        discovery.update_peer_state_root("peer1", state_root1, block_number1);
        discovery.update_peer_state_root("peer2", state_root2, block_number2);
        
        let peer_state_roots = discovery.get_peer_state_roots();
        assert_eq!(peer_state_roots.len(), 2);
        assert_eq!(peer_state_roots.get("peer1"), Some(&(state_root1, block_number1)));
        assert_eq!(peer_state_roots.get("peer2"), Some(&(state_root2, block_number2)));
    }

    #[test]
    fn test_clear_peers() {
        let client = Arc::new(TestSnapClient::new());
        let mut discovery = StateRootDiscovery::new(client, 5, Duration::from_secs(30));
        
        discovery.add_peer("peer1".to_string());
        discovery.add_peer("peer2".to_string());
        discovery.update_peer_state_root("peer1", B256::from([1u8; 32]), 18000000u64);
        
        assert_eq!(discovery.get_peer_count(), 2);
        assert_eq!(discovery.peer_state_roots.len(), 1);
        
        discovery.clear_peers();
        
        assert_eq!(discovery.get_peer_count(), 0);
        assert_eq!(discovery.peer_state_roots.len(), 0);
    }
}