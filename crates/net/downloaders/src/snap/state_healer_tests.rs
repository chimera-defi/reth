//! Tests for state healing system.

use crate::snap::{
    state_healer::{StateHealer, StateHealingConfig, MissingData, HealingResult, HealingStats},
    state_verifier::{StateData, AccountRange, StorageRange, AccountState},
    test_utils::TestSnapClient,
};
use alloy_primitives::{B256, Bytes, Address, U256};
use std::{sync::Arc, time::Duration};
use tokio;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_healer_creation() {
        let client = Arc::new(TestSnapClient::new());
        let healer = StateHealer::new(client, 5, Duration::from_secs(30));
        
        assert_eq!(healer.max_healing_attempts, 5);
        assert_eq!(healer.healing_timeout, Duration::from_secs(30));
        assert!(healer.healing_results.is_empty());
    }

    #[test]
    fn test_detect_missing_accounts() {
        let client = Arc::new(TestSnapClient::new());
        let mut healer = StateHealer::new(client, 5, Duration::from_secs(30));
        
        // Create test state data with missing accounts
        let state_data = StateData {
            account_ranges: vec![
                AccountRange {
                    accounts: vec![
                        (Address::from([1u8; 20]), AccountState {
                            nonce: U256::from(1),
                            balance: U256::from(1000),
                            code_hash: B256::from([1u8; 32]),
                            storage_root: B256::from([2u8; 32]),
                        }),
                    ],
                    proof: vec![B256::from([5u8; 32])],
                },
            ],
            storage_ranges: vec![],
            byte_codes: vec![],
            trie_nodes: vec![],
        };
        
        let missing_accounts = healer.detect_missing_accounts(&state_data);
        assert!(missing_accounts.is_empty()); // No missing accounts in this test
    }

    #[test]
    fn test_detect_missing_storage() {
        let client = Arc::new(TestSnapClient::new());
        let mut healer = StateHealer::new(client, 5, Duration::from_secs(30));
        
        // Create test state data with missing storage
        let state_data = StateData {
            account_ranges: vec![],
            storage_ranges: vec![
                StorageRange {
                    account: Address::from([1u8; 20]),
                    storage_slots: vec![
                        (B256::from([1u8; 32]), B256::from([2u8; 32])),
                    ],
                    proof: vec![B256::from([6u8; 32])],
                },
            ],
            byte_codes: vec![],
            trie_nodes: vec![],
        };
        
        let missing_storage = healer.detect_missing_storage(&state_data);
        assert!(missing_storage.is_empty()); // No missing storage in this test
    }

    #[test]
    fn test_detect_missing_byte_codes() {
        let client = Arc::new(TestSnapClient::new());
        let mut healer = StateHealer::new(client, 5, Duration::from_secs(30));
        
        // Create test state data with missing byte codes
        let state_data = StateData {
            account_ranges: vec![],
            storage_ranges: vec![],
            byte_codes: vec![
                (B256::from([1u8; 32]), Bytes::from(vec![0x60, 0x60])),
            ],
            trie_nodes: vec![],
        };
        
        let missing_byte_codes = healer.detect_missing_byte_codes(&state_data);
        assert!(missing_byte_codes.is_empty()); // No missing byte codes in this test
    }

    #[test]
    fn test_detect_missing_trie_nodes() {
        let client = Arc::new(TestSnapClient::new());
        let mut healer = StateHealer::new(client, 5, Duration::from_secs(30));
        
        // Create test state data with missing trie nodes
        let state_data = StateData {
            account_ranges: vec![],
            storage_ranges: vec![],
            byte_codes: vec![],
            trie_nodes: vec![
                (B256::from([1u8; 32]), Bytes::from(vec![0x80, 0x01])),
            ],
        };
        
        let missing_trie_nodes = healer.detect_missing_trie_nodes(&state_data);
        assert!(missing_trie_nodes.is_empty()); // No missing trie nodes in this test
    }

    #[test]
    fn test_heal_missing_data() {
        let client = Arc::new(TestSnapClient::new());
        let mut healer = StateHealer::new(client, 5, Duration::from_secs(30));
        
        // Create test missing data
        let missing_data = MissingData {
            missing_accounts: vec![Address::from([1u8; 20])],
            missing_storage: vec![(Address::from([1u8; 20]), B256::from([1u8; 32]))],
            missing_byte_codes: vec![B256::from([1u8; 32])],
            missing_trie_nodes: vec![B256::from([1u8; 32])],
        };
        
        let result = healer.heal_missing_data(missing_data).await;
        assert!(result.is_ok());
        let healing_result = result.unwrap();
        assert!(healing_result.is_successful);
    }

    #[test]
    fn test_verify_state_consistency() {
        let client = Arc::new(TestSnapClient::new());
        let mut healer = StateHealer::new(client, 5, Duration::from_secs(30));
        
        // Create test state data
        let state_data = StateData {
            account_ranges: vec![
                AccountRange {
                    accounts: vec![
                        (Address::from([1u8; 20]), AccountState {
                            nonce: U256::from(1),
                            balance: U256::from(1000),
                            code_hash: B256::from([1u8; 32]),
                            storage_root: B256::from([2u8; 32]),
                        }),
                    ],
                    proof: vec![B256::from([5u8; 32])],
                },
            ],
            storage_ranges: vec![],
            byte_codes: vec![],
            trie_nodes: vec![],
        };
        
        let is_consistent = healer.verify_state_consistency(&state_data);
        assert!(is_consistent);
    }

    #[test]
    fn test_get_healing_stats() {
        let client = Arc::new(TestSnapClient::new());
        let mut healer = StateHealer::new(client, 5, Duration::from_secs(30));
        
        // Add some healing results
        healer.healing_results.insert("test1".to_string(), HealingResult {
            is_successful: true,
            healed_accounts: 5,
            healed_storage_slots: 10,
            healed_byte_codes: 3,
            healed_trie_nodes: 7,
            healing_time: Duration::from_millis(100),
        });
        
        let stats = healer.get_healing_stats();
        assert_eq!(stats.total_healing_attempts, 1);
        assert_eq!(stats.successful_healing_attempts, 1);
        assert_eq!(stats.failed_healing_attempts, 0);
        assert_eq!(stats.total_healed_accounts, 5);
        assert_eq!(stats.total_healed_storage_slots, 10);
        assert_eq!(stats.total_healed_byte_codes, 3);
        assert_eq!(stats.total_healed_trie_nodes, 7);
    }

    #[test]
    fn test_clear_healing_results() {
        let client = Arc::new(TestSnapClient::new());
        let mut healer = StateHealer::new(client, 5, Duration::from_secs(30));
        
        // Add some healing results
        healer.healing_results.insert("test1".to_string(), HealingResult {
            is_successful: true,
            healed_accounts: 5,
            healed_storage_slots: 10,
            healed_byte_codes: 3,
            healed_trie_nodes: 7,
            healing_time: Duration::from_millis(100),
        });
        
        assert_eq!(healer.healing_results.len(), 1);
        
        healer.clear_healing_results();
        
        assert_eq!(healer.healing_results.len(), 0);
    }
}