//! Tests for the snap sync state manager.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snap::test_utils::MockProvider;
    use std::time::Duration;

    #[test]
    fn test_state_manager_creation() {
        let provider = MockProvider::default();
        let state_manager = SnapSyncStateManager::new(provider);
        
        assert!(state_manager.target_state_root().is_none());
        assert_eq!(state_manager.sync_progress().progress_percentage(), 0.0);
        assert!(!state_manager.is_sync_complete());
    }

    #[test]
    fn test_sync_progress_calculation() {
        let mut progress = SyncProgress::default();
        
        // Set total counts
        progress.total_accounts = 1000;
        progress.total_storage_slots = 2000;
        progress.total_byte_codes = 100;
        progress.total_trie_nodes = 500;
        
        // Update progress
        progress.update_progress(DataType::Accounts, 100);
        progress.update_progress(DataType::StorageSlots, 200);
        progress.update_progress(DataType::ByteCodes, 10);
        progress.update_progress(DataType::TrieNodes, 50);
        
        // Check progress calculation
        assert_eq!(progress.accounts_synced, 100);
        assert_eq!(progress.storage_slots_synced, 200);
        assert_eq!(progress.byte_codes_synced, 10);
        assert_eq!(progress.trie_nodes_synced, 50);
        
        // Total progress should be (360 / 3600) * 100 = 10%
        assert!((progress.progress_percentage() - 10.0).abs() < 0.1);
        assert!(!progress.is_complete());
    }

    #[test]
    fn test_sync_progress_completion() {
        let mut progress = SyncProgress::default();
        
        // Set total counts
        progress.total_accounts = 100;
        progress.total_storage_slots = 200;
        progress.total_byte_codes = 50;
        progress.total_trie_nodes = 100;
        
        // Update progress to completion
        progress.update_progress(DataType::Accounts, 100);
        progress.update_progress(DataType::StorageSlots, 200);
        progress.update_progress(DataType::ByteCodes, 50);
        progress.update_progress(DataType::TrieNodes, 100);
        
        assert!(progress.is_complete());
        assert!((progress.progress_percentage() - 100.0).abs() < 0.1);
    }

    #[test]
    fn test_data_type_enum() {
        assert_eq!(DataType::Accounts, DataType::Accounts);
        assert_eq!(DataType::StorageSlots, DataType::StorageSlots);
        assert_eq!(DataType::ByteCodes, DataType::ByteCodes);
        assert_eq!(DataType::TrieNodes, DataType::TrieNodes);
    }

    #[test]
    fn test_state_root_info() {
        let info = StateRootInfo {
            block_number: 1000,
            block_hash: B256::from([1u8; 32]),
            state_root: B256::from([2u8; 32]),
            verified: true,
        };
        
        assert_eq!(info.block_number, 1000);
        assert_eq!(info.block_hash, B256::from([1u8; 32]));
        assert_eq!(info.state_root, B256::from([2u8; 32]));
        assert!(info.verified);
    }

    #[test]
    fn test_cache_stats() {
        let stats = CacheStats {
            total_entries: 100,
            verified_entries: 80,
            cache_hit_rate: 0.8,
        };
        
        assert_eq!(stats.total_entries, 100);
        assert_eq!(stats.verified_entries, 80);
        assert!((stats.cache_hit_rate - 0.8).abs() < 0.01);
    }
}