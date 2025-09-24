//! Tests for the snap sync progress reporter.

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_progress_reporter_creation() {
        let reporter = SnapSyncProgressReporter::new(
            Duration::from_secs(10),
            true,
        );
        
        assert!(reporter.is_complete());
        assert_eq!(reporter.elapsed_time().as_secs(), 0);
    }

    #[test]
    fn test_progress_reporting() {
        let reporter = SnapSyncProgressReporter::new(
            Duration::from_secs(1), // Short interval for testing
            true,
        );
        
        // Set total counts
        reporter.set_total_counts(1000, 2000, 100, 500);
        
        // Update progress
        reporter.update_progress(DataType::Accounts, 100);
        reporter.update_progress(DataType::StorageSlots, 200);
        reporter.update_progress(DataType::ByteCodes, 10);
        reporter.update_progress(DataType::TrieNodes, 50);
        
        let report = reporter.get_progress_report();
        assert!((report.progress_percentage - 10.0).abs() < 0.1);
        assert_eq!(report.accounts_progress.synced, 100);
        assert_eq!(report.accounts_progress.total, 1000);
        assert_eq!(report.storage_progress.synced, 200);
        assert_eq!(report.storage_progress.total, 2000);
    }

    #[test]
    fn test_data_progress() {
        let progress = DataProgress {
            data_type: DataType::Accounts,
            total: 1000,
            synced: 250,
            percentage: 25.0,
            rate: 10.0,
        };
        
        assert_eq!(progress.data_type, DataType::Accounts);
        assert_eq!(progress.total, 1000);
        assert_eq!(progress.synced, 250);
        assert!((progress.percentage - 25.0).abs() < 0.1);
        assert!((progress.rate - 10.0).abs() < 0.1);
    }

    #[test]
    fn test_sync_stats() {
        let stats = SyncStats {
            total_items: 3600,
            synced_items: 360,
            progress_percentage: 10.0,
            total_bytes_downloaded: 1000000,
            average_sync_rate: 10.0,
            current_sync_rate: 12.0,
            elapsed_time: Duration::from_secs(36),
            estimated_time_remaining: Some(Duration::from_secs(324)),
        };
        
        assert_eq!(stats.total_items, 3600);
        assert_eq!(stats.synced_items, 360);
        assert!((stats.progress_percentage - 10.0).abs() < 0.1);
        assert_eq!(stats.total_bytes_downloaded, 1000000);
        assert!((stats.average_sync_rate - 10.0).abs() < 0.1);
        assert!((stats.current_sync_rate - 12.0).abs() < 0.1);
        assert_eq!(stats.elapsed_time.as_secs(), 36);
        assert_eq!(stats.estimated_time_remaining.unwrap().as_secs(), 324);
    }

    #[test]
    fn test_progress_summary() {
        let reporter = SnapSyncProgressReporter::new(
            Duration::from_secs(10),
            true,
        );
        
        reporter.set_total_counts(1000, 2000, 100, 500);
        reporter.update_progress(DataType::Accounts, 100);
        reporter.update_progress(DataType::StorageSlots, 200);
        
        let summary = reporter.get_summary();
        assert!(summary.contains("Snap Sync Progress"));
        assert!(summary.contains("Accounts:"));
        assert!(summary.contains("Storage:"));
        assert!(summary.contains("Byte Codes:"));
        assert!(summary.contains("Trie Nodes:"));
    }

    #[test]
    fn test_disabled_progress_reporter() {
        let mut reporter = SnapSyncProgressReporter::new(
            Duration::from_secs(10),
            false, // Disabled
        );
        
        // Should not report progress when disabled
        reporter.set_total_counts(1000, 2000, 100, 500);
        reporter.update_progress(DataType::Accounts, 100);
        
        // Should still be able to get progress report
        let report = reporter.get_progress_report();
        assert_eq!(report.accounts_progress.synced, 100);
    }

    #[test]
    fn test_progress_completion() {
        let reporter = SnapSyncProgressReporter::new(
            Duration::from_secs(10),
            true,
        );
        
        reporter.set_total_counts(100, 200, 50, 100);
        
        // Update to completion
        reporter.update_progress(DataType::Accounts, 100);
        reporter.update_progress(DataType::StorageSlots, 200);
        reporter.update_progress(DataType::ByteCodes, 50);
        reporter.update_progress(DataType::TrieNodes, 100);
        
        assert!(reporter.is_complete());
        
        let report = reporter.get_progress_report();
        assert!((report.progress_percentage - 100.0).abs() < 0.1);
    }
}