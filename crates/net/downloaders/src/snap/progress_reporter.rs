//! Progress reporting for snap sync operations.

use crate::snap::state_manager::{SyncProgress, DataType};
use crate::snap::peer_manager::PeerStats;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tracing::*;

/// Progress reporter for snap sync operations
#[derive(Debug, Clone)]
pub struct SnapSyncProgressReporter {
    /// Current sync progress
    progress: Arc<Mutex<SyncProgress>>,
    /// Start time of sync operation
    start_time: Instant,
    /// Last update time
    last_update: Arc<Mutex<Instant>>,
    /// Update interval for progress reporting
    update_interval: Duration,
    /// Whether progress reporting is enabled
    enabled: bool,
}

/// Progress report data
#[derive(Debug, Clone)]
pub struct ProgressReport {
    /// Current progress percentage
    pub progress_percentage: f64,
    /// Accounts progress
    pub accounts_progress: DataProgress,
    /// Storage slots progress
    pub storage_progress: DataProgress,
    /// Byte codes progress
    pub byte_codes_progress: DataProgress,
    /// Trie nodes progress
    pub trie_nodes_progress: DataProgress,
    /// Overall sync statistics
    pub sync_stats: SyncStats,
    /// Estimated time remaining
    pub estimated_time_remaining: Option<Duration>,
    /// Current sync rate (items per second)
    pub sync_rate: f64,
}

/// Progress for a specific data type
#[derive(Debug, Clone)]
pub struct DataProgress {
    /// Data type
    pub data_type: DataType,
    /// Total items to sync
    pub total: u64,
    /// Items synced so far
    pub synced: u64,
    /// Progress percentage
    pub percentage: f64,
    /// Sync rate (items per second)
    pub rate: f64,
}

/// Overall sync statistics
#[derive(Debug, Clone)]
pub struct SyncStats {
    /// Total items to sync
    pub total_items: u64,
    /// Items synced so far
    pub synced_items: u64,
    /// Overall progress percentage
    pub progress_percentage: f64,
    /// Total bytes downloaded
    pub total_bytes_downloaded: u64,
    /// Average sync rate (items per second)
    pub average_sync_rate: f64,
    /// Current sync rate (items per second)
    pub current_sync_rate: f64,
    /// Elapsed time
    pub elapsed_time: Duration,
    /// Estimated time remaining
    pub estimated_time_remaining: Option<Duration>,
}

impl SnapSyncProgressReporter {
    /// Create a new progress reporter
    pub fn new(update_interval: Duration, enabled: bool) -> Self {
        let now = Instant::now();
        Self {
            progress: Arc::new(Mutex::new(SyncProgress::default())),
            start_time: now,
            last_update: Arc::new(Mutex::new(now)),
            update_interval,
            enabled,
        }
    }

    /// Update progress for a specific data type
    pub fn update_progress(&self, data_type: DataType, count: u64) {
        if !self.enabled {
            return;
        }

        let mut progress = self.progress.lock().unwrap();
        progress.update_progress(data_type, count);

        // Check if we should report progress
        let now = Instant::now();
        let mut last_update = self.last_update.lock().unwrap();
        
        if now.duration_since(*last_update) >= self.update_interval {
            self.report_progress(&progress, now);
            *last_update = now;
        }
    }

    /// Set total counts for progress tracking
    pub fn set_total_counts(&self, accounts: u64, storage_slots: u64, byte_codes: u64, trie_nodes: u64) {
        if !self.enabled {
            return;
        }

        let mut progress = self.progress.lock().unwrap();
        progress.total_accounts = accounts;
        progress.total_storage_slots = storage_slots;
        progress.total_byte_codes = byte_codes;
        progress.total_trie_nodes = trie_nodes;

        info!(target: "snap_sync::progress",
            accounts = accounts,
            storage_slots = storage_slots,
            byte_codes = byte_codes,
            trie_nodes = trie_nodes,
            "Set total sync counts"
        );
    }

    /// Get current progress report
    pub fn get_progress_report(&self) -> ProgressReport {
        let progress = self.progress.lock().unwrap();
        let now = Instant::now();
        let elapsed_time = now.duration_since(self.start_time);

        // Calculate sync rates
        let total_items = progress.total_accounts + progress.total_storage_slots + 
                         progress.total_byte_codes + progress.total_trie_nodes;
        let synced_items = progress.accounts_synced + progress.storage_slots_synced + 
                          progress.byte_codes_synced + progress.trie_nodes_synced;

        let average_sync_rate = if elapsed_time.as_secs() > 0 {
            synced_items as f64 / elapsed_time.as_secs() as f64
        } else {
            0.0
        };

        // Estimate time remaining
        let estimated_time_remaining = if average_sync_rate > 0.0 && synced_items < total_items {
            let remaining_items = total_items - synced_items;
            Some(Duration::from_secs((remaining_items as f64 / average_sync_rate) as u64))
        } else {
            None
        };

        ProgressReport {
            progress_percentage: progress.progress_percentage(),
            accounts_progress: DataProgress {
                data_type: DataType::Accounts,
                total: progress.total_accounts,
                synced: progress.accounts_synced,
                percentage: if progress.total_accounts > 0 { 
                    (progress.accounts_synced as f64 / progress.total_accounts as f64) * 100.0 
                } else { 0.0 },
                rate: if elapsed_time.as_secs() > 0 { 
                    progress.accounts_synced as f64 / elapsed_time.as_secs() as f64 
                } else { 0.0 },
            },
            storage_progress: DataProgress {
                data_type: DataType::StorageSlots,
                total: progress.total_storage_slots,
                synced: progress.storage_slots_synced,
                percentage: if progress.total_storage_slots > 0 { 
                    (progress.storage_slots_synced as f64 / progress.total_storage_slots as f64) * 100.0 
                } else { 0.0 },
                rate: if elapsed_time.as_secs() > 0 { 
                    progress.storage_slots_synced as f64 / elapsed_time.as_secs() as f64 
                } else { 0.0 },
            },
            byte_codes_progress: DataProgress {
                data_type: DataType::ByteCodes,
                total: progress.total_byte_codes,
                synced: progress.byte_codes_synced,
                percentage: if progress.total_byte_codes > 0 { 
                    (progress.byte_codes_synced as f64 / progress.total_byte_codes as f64) * 100.0 
                } else { 0.0 },
                rate: if elapsed_time.as_secs() > 0 { 
                    progress.byte_codes_synced as f64 / elapsed_time.as_secs() as f64 
                } else { 0.0 },
            },
            trie_nodes_progress: DataProgress {
                data_type: DataType::TrieNodes,
                total: progress.total_trie_nodes,
                synced: progress.trie_nodes_synced,
                percentage: if progress.total_trie_nodes > 0 { 
                    (progress.trie_nodes_synced as f64 / progress.total_trie_nodes as f64) * 100.0 
                } else { 0.0 },
                rate: if elapsed_time.as_secs() > 0 { 
                    progress.trie_nodes_synced as f64 / elapsed_time.as_secs() as f64 
                } else { 0.0 },
            },
            sync_stats: SyncStats {
                total_items,
                synced_items,
                progress_percentage: progress.progress_percentage(),
                total_bytes_downloaded: 0, // This would be updated from peer stats
                average_sync_rate,
                current_sync_rate: average_sync_rate, // Simplified for now
                elapsed_time,
                estimated_time_remaining,
            },
            estimated_time_remaining,
            sync_rate: average_sync_rate,
        }
    }

    /// Report progress to logs
    fn report_progress(&self, progress: &SyncProgress, now: Instant) {
        let elapsed_time = now.duration_since(self.start_time);
        let progress_percentage = progress.progress_percentage();

        info!(target: "snap_sync::progress",
            progress = progress_percentage,
            elapsed_seconds = elapsed_time.as_secs(),
            accounts = format!("{}/{}", progress.accounts_synced, progress.total_accounts),
            storage_slots = format!("{}/{}", progress.storage_slots_synced, progress.total_storage_slots),
            byte_codes = format!("{}/{}", progress.byte_codes_synced, progress.total_byte_codes),
            trie_nodes = format!("{}/{}", progress.trie_nodes_synced, progress.total_trie_nodes),
            "Snap sync progress update"
        );

        // Log detailed progress for each data type
        if progress.total_accounts > 0 {
            let accounts_percentage = (progress.accounts_synced as f64 / progress.total_accounts as f64) * 100.0;
            debug!(target: "snap_sync::progress",
                data_type = "accounts",
                synced = progress.accounts_synced,
                total = progress.total_accounts,
                percentage = accounts_percentage,
                "Account sync progress"
            );
        }

        if progress.total_storage_slots > 0 {
            let storage_percentage = (progress.storage_slots_synced as f64 / progress.total_storage_slots as f64) * 100.0;
            debug!(target: "snap_sync::progress",
                data_type = "storage_slots",
                synced = progress.storage_slots_synced,
                total = progress.total_storage_slots,
                percentage = storage_percentage,
                "Storage sync progress"
            );
        }

        if progress.total_byte_codes > 0 {
            let byte_codes_percentage = (progress.byte_codes_synced as f64 / progress.total_byte_codes as f64) * 100.0;
            debug!(target: "snap_sync::progress",
                data_type = "byte_codes",
                synced = progress.byte_codes_synced,
                total = progress.total_byte_codes,
                percentage = byte_codes_percentage,
                "Byte code sync progress"
            );
        }

        if progress.total_trie_nodes > 0 {
            let trie_nodes_percentage = (progress.trie_nodes_synced as f64 / progress.total_trie_nodes as f64) * 100.0;
            debug!(target: "snap_sync::progress",
                data_type = "trie_nodes",
                synced = progress.trie_nodes_synced,
                total = progress.total_trie_nodes,
                percentage = trie_nodes_percentage,
                "Trie node sync progress"
            );
        }
    }

    /// Check if sync is complete
    pub fn is_complete(&self) -> bool {
        let progress = self.progress.lock().unwrap();
        progress.is_complete()
    }

    /// Get elapsed time
    pub fn elapsed_time(&self) -> Duration {
        Instant::now().duration_since(self.start_time)
    }

    /// Enable or disable progress reporting
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Update the update interval
    pub fn set_update_interval(&mut self, interval: Duration) {
        self.update_interval = interval;
    }

    /// Force a progress report
    pub fn force_report(&self) {
        if !self.enabled {
            return;
        }

        let progress = self.progress.lock().unwrap();
        let now = Instant::now();
        self.report_progress(&progress, now);
    }

    /// Get a summary of current progress
    pub fn get_summary(&self) -> String {
        let report = self.get_progress_report();
        let elapsed = self.elapsed_time();
        
        format!(
            "Snap Sync Progress: {:.1}% complete\n\
             Accounts: {}/{} ({:.1}%)\n\
             Storage: {}/{} ({:.1}%)\n\
             Byte Codes: {}/{} ({:.1}%)\n\
             Trie Nodes: {}/{} ({:.1}%)\n\
             Elapsed: {}s, Rate: {:.1} items/s",
            report.progress_percentage,
            report.accounts_progress.synced, report.accounts_progress.total, report.accounts_progress.percentage,
            report.storage_progress.synced, report.storage_progress.total, report.storage_progress.percentage,
            report.byte_codes_progress.synced, report.byte_codes_progress.total, report.byte_codes_progress.percentage,
            report.trie_nodes_progress.synced, report.trie_nodes_progress.total, report.trie_nodes_progress.percentage,
            elapsed.as_secs(),
            report.sync_rate
        )
    }
}

/// Trait for progress reporting
pub trait ProgressReporter {
    /// Update progress for a specific data type
    fn update_progress(&self, data_type: DataType, count: u64);
    
    /// Set total counts for progress tracking
    fn set_total_counts(&self, accounts: u64, storage_slots: u64, byte_codes: u64, trie_nodes: u64);
    
    /// Get current progress report
    fn get_progress_report(&self) -> ProgressReport;
    
    /// Check if sync is complete
    fn is_complete(&self) -> bool;
    
    /// Get elapsed time
    fn elapsed_time(&self) -> Duration;
}

impl ProgressReporter for SnapSyncProgressReporter {
    fn update_progress(&self, data_type: DataType, count: u64) {
        self.update_progress(data_type, count)
    }

    fn set_total_counts(&self, accounts: u64, storage_slots: u64, byte_codes: u64, trie_nodes: u64) {
        self.set_total_counts(accounts, storage_slots, byte_codes, trie_nodes)
    }

    fn get_progress_report(&self) -> ProgressReport {
        self.get_progress_report()
    }

    fn is_complete(&self) -> bool {
        self.is_complete()
    }

    fn elapsed_time(&self) -> Duration {
        self.elapsed_time()
    }
}