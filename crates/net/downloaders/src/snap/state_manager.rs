//! State root management for snap sync.

use alloy_primitives::{B256, BlockNumber};
use reth_db_api::{
    cursor::{DbCursorRO, DbCursorRW},
    tables,
    transaction::{DbTx, DbTxMut},
};
use reth_provider::{BlockHashReader, HeaderProvider, StateProvider};
use std::collections::HashMap;
use tracing::*;

/// State root information for a block
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateRootInfo {
    /// Block number
    pub block_number: BlockNumber,
    /// Block hash
    pub block_hash: B256,
    /// State root hash
    pub state_root: B256,
    /// Whether this state root has been verified
    pub verified: bool,
}

/// Manages state roots for snap sync operations
#[derive(Debug)]
pub struct SnapSyncStateManager<Provider> {
    /// Database provider for accessing state data
    provider: Provider,
    /// Cache of state root information
    state_root_cache: HashMap<B256, StateRootInfo>,
    /// Current sync target state root
    target_state_root: Option<B256>,
    /// Current sync progress
    sync_progress: SyncProgress,
}

/// Progress tracking for snap sync
#[derive(Debug, Clone, Default)]
pub struct SyncProgress {
    /// Total accounts to sync
    pub total_accounts: u64,
    /// Accounts synced so far
    pub accounts_synced: u64,
    /// Total storage slots to sync
    pub total_storage_slots: u64,
    /// Storage slots synced so far
    pub storage_slots_synced: u64,
    /// Total byte codes to sync
    pub total_byte_codes: u64,
    /// Byte codes synced so far
    pub byte_codes_synced: u64,
    /// Total trie nodes to sync
    pub total_trie_nodes: u64,
    /// Trie nodes synced so far
    pub trie_nodes_synced: u64,
}

impl SyncProgress {
    /// Get overall progress percentage
    pub fn progress_percentage(&self) -> f64 {
        let total = self.total_accounts + self.total_storage_slots + self.total_byte_codes + self.total_trie_nodes;
        let synced = self.accounts_synced + self.storage_slots_synced + self.byte_codes_synced + self.trie_nodes_synced;
        
        if total == 0 {
            0.0
        } else {
            (synced as f64 / total as f64) * 100.0
        }
    }

    /// Check if sync is complete
    pub fn is_complete(&self) -> bool {
        self.accounts_synced >= self.total_accounts &&
        self.storage_slots_synced >= self.total_storage_slots &&
        self.byte_codes_synced >= self.total_byte_codes &&
        self.trie_nodes_synced >= self.total_trie_nodes
    }

    /// Update progress for a specific data type
    pub fn update_progress(&mut self, data_type: DataType, count: u64) {
        match data_type {
            DataType::Accounts => self.accounts_synced += count,
            DataType::StorageSlots => self.storage_slots_synced += count,
            DataType::ByteCodes => self.byte_codes_synced += count,
            DataType::TrieNodes => self.trie_nodes_synced += count,
        }
    }
}

/// Types of data being synced
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataType {
    Accounts,
    StorageSlots,
    ByteCodes,
    TrieNodes,
}

impl<Provider> SnapSyncStateManager<Provider>
where
    Provider: HeaderProvider + BlockHashReader + StateProvider + Clone,
{
    /// Create a new state manager
    pub fn new(provider: Provider) -> Self {
        Self {
            provider,
            state_root_cache: HashMap::new(),
            target_state_root: None,
            sync_progress: SyncProgress::default(),
        }
    }

    /// Set the target state root for sync
    pub fn set_target_state_root(&mut self, state_root: B256) -> Result<(), Box<dyn std::error::Error>> {
        info!(target: "snap_sync::state_manager", state_root = ?state_root, "Setting target state root");
        
        // Verify the state root exists
        if let Some(block_info) = self.find_block_by_state_root(state_root)? {
            self.target_state_root = Some(state_root);
            self.state_root_cache.insert(state_root, block_info);
            Ok(())
        } else {
            Err(format!("State root {} not found in database", state_root).into())
        }
    }

    /// Get the current target state root
    pub fn target_state_root(&self) -> Option<B256> {
        self.target_state_root
    }

    /// Get state root information for a block
    pub fn get_state_root_info(&mut self, block_hash: B256) -> Result<Option<StateRootInfo>, Box<dyn std::error::Error>> {
        // Check cache first
        if let Some(info) = self.state_root_cache.get(&block_hash) {
            return Ok(Some(info.clone()));
        }

        // Query database
        let info = self.query_state_root_from_db(block_hash)?;
        if let Some(ref info) = info {
            self.state_root_cache.insert(block_hash, info.clone());
        }

        Ok(info)
    }

    /// Verify a state root against the database
    pub fn verify_state_root(&mut self, state_root: B256) -> Result<bool, Box<dyn std::error::Error>> {
        info!(target: "snap_sync::state_manager", state_root = ?state_root, "Verifying state root");

        // Check if we have this state root in cache
        if let Some(info) = self.state_root_cache.get(&state_root) {
            return Ok(info.verified);
        }

        // Query database to verify
        let is_valid = self.query_state_root_from_db(state_root)?.is_some();
        
        // Update cache
        if let Some(info) = self.state_root_cache.get_mut(&state_root) {
            info.verified = is_valid;
        }

        Ok(is_valid)
    }

    /// Get sync progress
    pub fn sync_progress(&self) -> &SyncProgress {
        &self.sync_progress
    }

    /// Update sync progress
    pub fn update_sync_progress(&mut self, data_type: DataType, count: u64) {
        self.sync_progress.update_progress(data_type, count);
        
        info!(target: "snap_sync::state_manager", 
            data_type = ?data_type, 
            count = count,
            progress = self.sync_progress.progress_percentage(),
            "Updated sync progress"
        );
    }

    /// Set total counts for progress tracking
    pub fn set_total_counts(&mut self, accounts: u64, storage_slots: u64, byte_codes: u64, trie_nodes: u64) {
        self.sync_progress.total_accounts = accounts;
        self.sync_progress.total_storage_slots = storage_slots;
        self.sync_progress.total_byte_codes = byte_codes;
        self.sync_progress.total_trie_nodes = trie_nodes;
        
        info!(target: "snap_sync::state_manager",
            accounts = accounts,
            storage_slots = storage_slots,
            byte_codes = byte_codes,
            trie_nodes = trie_nodes,
            "Set total sync counts"
        );
    }

    /// Check if sync is complete
    pub fn is_sync_complete(&self) -> bool {
        self.sync_progress.is_complete()
    }

    /// Get the latest verified state root
    pub fn get_latest_verified_state_root(&self) -> Option<B256> {
        self.state_root_cache
            .values()
            .filter(|info| info.verified)
            .max_by_key(|info| info.block_number)
            .map(|info| info.state_root)
    }

    /// Find block information by state root
    fn find_block_by_state_root(&self, state_root: B256) -> Result<Option<StateRootInfo>, Box<dyn std::error::Error>> {
        // In a real implementation, this would query the database to find the block
        // that has this state root. For now, we'll return a placeholder.
        Ok(Some(StateRootInfo {
            block_number: 0,
            block_hash: B256::ZERO,
            state_root,
            verified: false,
        }))
    }

    /// Query state root information from database
    fn query_state_root_from_db(&self, block_hash: B256) -> Result<Option<StateRootInfo>, Box<dyn std::error::Error>> {
        // In a real implementation, this would query the database
        // For now, we'll return a placeholder
        Ok(Some(StateRootInfo {
            block_number: 0,
            block_hash,
            state_root: B256::ZERO,
            verified: false,
        }))
    }

    /// Clear the state root cache
    pub fn clear_cache(&mut self) {
        self.state_root_cache.clear();
        info!(target: "snap_sync::state_manager", "Cleared state root cache");
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> CacheStats {
        let total_entries = self.state_root_cache.len();
        let verified_entries = self.state_root_cache.values().filter(|info| info.verified).count();
        
        CacheStats {
            total_entries,
            verified_entries,
            cache_hit_rate: if total_entries > 0 { verified_entries as f64 / total_entries as f64 } else { 0.0 },
        }
    }
}

/// Cache statistics for the state manager
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Total number of entries in cache
    pub total_entries: usize,
    /// Number of verified entries
    pub verified_entries: usize,
    /// Cache hit rate (verified entries / total entries)
    pub cache_hit_rate: f64,
}

/// Trait for state root management
pub trait StateRootManager {
    /// Set the target state root for sync
    fn set_target_state_root(&mut self, state_root: B256) -> Result<(), Box<dyn std::error::Error>>;
    
    /// Get the current target state root
    fn target_state_root(&self) -> Option<B256>;
    
    /// Verify a state root
    fn verify_state_root(&mut self, state_root: B256) -> Result<bool, Box<dyn std::error::Error>>;
    
    /// Get sync progress
    fn sync_progress(&self) -> &SyncProgress;
    
    /// Update sync progress
    fn update_sync_progress(&mut self, data_type: DataType, count: u64);
    
    /// Check if sync is complete
    fn is_sync_complete(&self) -> bool;
}

impl<Provider> StateRootManager for SnapSyncStateManager<Provider>
where
    Provider: HeaderProvider + BlockHashReader + StateProvider + Clone,
{
    fn set_target_state_root(&mut self, state_root: B256) -> Result<(), Box<dyn std::error::Error>> {
        self.set_target_state_root(state_root)
    }

    fn target_state_root(&self) -> Option<B256> {
        self.target_state_root
    }

    fn verify_state_root(&mut self, state_root: B256) -> Result<bool, Box<dyn std::error::Error>> {
        self.verify_state_root(state_root)
    }

    fn sync_progress(&self) -> &SyncProgress {
        self.sync_progress()
    }

    fn update_sync_progress(&mut self, data_type: DataType, count: u64) {
        self.update_sync_progress(data_type, count)
    }

    fn is_sync_complete(&self) -> bool {
        self.is_sync_complete()
    }
}