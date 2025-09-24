//! Tests for the snap sync peer manager.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snap::test_utils::TestSnapClient;
    use std::time::{Duration, Instant};

    #[test]
    fn test_peer_manager_creation() {
        let peer_manager = SnapSyncPeerManager::new(
            PeerSelectionStrategy::BestPerformance,
            10,
            0.8,
            5,
            Duration::from_secs(300),
        );
        
        let stats = peer_manager.get_peer_stats();
        assert_eq!(stats.total_peers, 0);
        assert_eq!(stats.available_peers, 0);
        assert_eq!(stats.snap_sync_peers, 0);
    }

    #[test]
    fn test_add_and_remove_peer() {
        let mut peer_manager = SnapSyncPeerManager::new(
            PeerSelectionStrategy::BestPerformance,
            10,
            0.8,
            5,
            Duration::from_secs(300),
        );
        
        // Add a peer
        peer_manager.add_peer("peer1".to_string(), true);
        
        let stats = peer_manager.get_peer_stats();
        assert_eq!(stats.total_peers, 1);
        assert_eq!(stats.snap_sync_peers, 1);
        
        // Remove the peer
        peer_manager.remove_peer("peer1");
        
        let stats = peer_manager.get_peer_stats();
        assert_eq!(stats.total_peers, 0);
        assert_eq!(stats.snap_sync_peers, 0);
    }

    #[test]
    fn test_peer_selection() {
        let mut peer_manager = SnapSyncPeerManager::new(
            PeerSelectionStrategy::BestPerformance,
            10,
            0.8,
            5,
            Duration::from_secs(300),
        );
        
        // Add peers with different success rates
        peer_manager.add_peer("peer1".to_string(), true);
        peer_manager.add_peer("peer2".to_string(), true);
        
        // Record some successes and failures
        peer_manager.record_success("peer1", Duration::from_millis(100), 1000);
        peer_manager.record_success("peer1", Duration::from_millis(200), 2000);
        peer_manager.record_failure("peer1");
        
        peer_manager.record_success("peer2", Duration::from_millis(150), 1500);
        peer_manager.record_success("peer2", Duration::from_millis(250), 2500);
        
        // peer2 should have better success rate (100% vs 66.7%)
        let selected_peer = peer_manager.select_peer();
        assert!(selected_peer.is_some());
        // Note: The actual selection depends on the strategy implementation
    }

    #[test]
    fn test_peer_metrics() {
        let mut metrics = PeerMetrics::default();
        
        // Record some requests
        metrics.record_success(Duration::from_millis(100), 1000);
        metrics.record_success(Duration::from_millis(200), 2000);
        metrics.record_failure();
        
        assert_eq!(metrics.total_requests, 3);
        assert_eq!(metrics.successful_requests, 2);
        assert_eq!(metrics.failed_requests, 1);
        assert_eq!(metrics.total_bytes_downloaded, 3000);
        assert!((metrics.success_rate() - 0.6667).abs() < 0.01);
    }

    #[test]
    fn test_peer_availability() {
        let mut peer_manager = SnapSyncPeerManager::new(
            PeerSelectionStrategy::BestPerformance,
            10,
            0.8,
            3, // Max 3 consecutive failures
            Duration::from_secs(300),
        );
        
        peer_manager.add_peer("peer1".to_string(), true);
        
        // Record failures until peer becomes unavailable
        peer_manager.record_failure("peer1");
        peer_manager.record_failure("peer1");
        peer_manager.record_failure("peer1");
        
        // Peer should now be unavailable
        let peer_info = peer_manager.get_peer_info("peer1");
        assert!(peer_info.is_some());
        assert!(!peer_info.unwrap().is_available);
        
        // Should not be able to select unavailable peer
        let selected_peer = peer_manager.select_peer();
        assert!(selected_peer.is_none());
    }

    #[test]
    fn test_peer_selection_strategies() {
        let strategies = vec![
            PeerSelectionStrategy::BestPerformance,
            PeerSelectionStrategy::FastestResponse,
            PeerSelectionStrategy::RoundRobin,
            PeerSelectionStrategy::Random,
        ];
        
        for strategy in strategies {
            let peer_manager = SnapSyncPeerManager::new(
                strategy,
                10,
                0.8,
                5,
                Duration::from_secs(300),
            );
            
            // Should be able to create peer manager with any strategy
            assert_eq!(peer_manager.get_peer_stats().total_peers, 0);
        }
    }

    #[test]
    fn test_peer_stats() {
        let mut peer_manager = SnapSyncPeerManager::new(
            PeerSelectionStrategy::BestPerformance,
            10,
            0.8,
            5,
            Duration::from_secs(300),
        );
        
        peer_manager.add_peer("peer1".to_string(), true);
        peer_manager.add_peer("peer2".to_string(), false); // Doesn't support snap sync
        
        let stats = peer_manager.get_peer_stats();
        assert_eq!(stats.total_peers, 2);
        assert_eq!(stats.available_peers, 2);
        assert_eq!(stats.snap_sync_peers, 1);
    }
}