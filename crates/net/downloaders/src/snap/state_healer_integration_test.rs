//! Integration tests for state healing system.

use crate::snap::{
    state_healer::{StateHealer, StateHealingConfig, MissingData},
    state_verifier::{StateData, AccountRange, StorageRange, AccountState},
    test_utils::TestSnapClient,
};
use alloy_primitives::{B256, Bytes, Address, U256};
use std::{sync::Arc, time::Duration};
use tokio;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_state_healing_integration() {
        let client = Arc::new(TestSnapClient::new());
        let mut healer = StateHealer::new(client, 3, Duration::from_secs(10));
        
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
            storage_ranges: vec![
                StorageRange {
                    account: Address::from([1u8; 20]),
                    storage_slots: vec![
                        (B256::from([1u8; 32]), B256::from([2u8; 32])),
                    ],
                    proof: vec![B256::from([6u8; 32])],
                },
            ],
            byte_codes: vec![
                (B256::from([1u8; 32]), Bytes::from(vec![0x60, 0x60])),
            ],
            trie_nodes: vec![
                (B256::from([1u8; 32]), Bytes::from(vec![0x80, 0x01])),
            ],
        };
        
        // Test state consistency
        let is_consistent = healer.verify_state_consistency(&state_data);
        assert!(is_consistent);
        
        // Test missing data detection
        let missing_accounts = healer.detect_missing_accounts(&state_data);
        let missing_storage = healer.detect_missing_storage(&state_data);
        let missing_byte_codes = healer.detect_missing_byte_codes(&state_data);
        let missing_trie_nodes = healer.detect_missing_trie_nodes(&state_data);
        
        assert!(missing_accounts.is_empty());
        assert!(missing_storage.is_empty());
        assert!(missing_byte_codes.is_empty());
        assert!(missing_trie_nodes.is_empty());
        
        // Test healing with empty missing data
        let missing_data = MissingData {
            missing_accounts: vec![],
            missing_storage: vec![],
            missing_byte_codes: vec![],
            missing_trie_nodes: vec![],
        };
        
        let result = healer.heal_missing_data(missing_data).await;
        assert!(result.is_ok());
        let healing_result = result.unwrap();
        assert!(healing_result.is_successful);
        assert_eq!(healing_result.healed_accounts, 0);
        assert_eq!(healing_result.healed_storage_slots, 0);
        assert_eq!(healing_result.healed_byte_codes, 0);
        assert_eq!(healing_result.healed_trie_nodes, 0);
        
        // Test healing statistics
        let stats = healer.get_healing_stats();
        assert_eq!(stats.total_healing_attempts, 1);
        assert_eq!(stats.successful_healing_attempts, 1);
        assert_eq!(stats.failed_healing_attempts, 0);
    }

    #[tokio::test]
    async fn test_state_healing_with_missing_data() {
        let client = Arc::new(TestSnapClient::new());
        let mut healer = StateHealer::new(client, 3, Duration::from_secs(10));
        
        // Create missing data
        let missing_data = MissingData {
            missing_accounts: vec![
                Address::from([1u8; 20]),
                Address::from([2u8; 20]),
            ],
            missing_storage: vec![
                (Address::from([1u8; 20]), B256::from([1u8; 32])),
                (Address::from([2u8; 20]), B256::from([2u8; 32])),
            ],
            missing_byte_codes: vec![
                B256::from([1u8; 32]),
                B256::from([2u8; 32]),
            ],
            missing_trie_nodes: vec![
                B256::from([1u8; 32]),
                B256::from([2u8; 32]),
            ],
        };
        
        // Test healing
        let result = healer.heal_missing_data(missing_data).await;
        assert!(result.is_ok());
        let healing_result = result.unwrap();
        assert!(healing_result.is_successful);
        assert_eq!(healing_result.healed_accounts, 2);
        assert_eq!(healing_result.healed_storage_slots, 2);
        assert_eq!(healing_result.healed_byte_codes, 2);
        assert_eq!(healing_result.healed_trie_nodes, 2);
        
        // Test healing statistics
        let stats = healer.get_healing_stats();
        assert_eq!(stats.total_healing_attempts, 1);
        assert_eq!(stats.successful_healing_attempts, 1);
        assert_eq!(stats.failed_healing_attempts, 0);
        assert_eq!(stats.total_healed_accounts, 2);
        assert_eq!(stats.total_healed_storage_slots, 2);
        assert_eq!(stats.total_healed_byte_codes, 2);
        assert_eq!(stats.total_healed_trie_nodes, 2);
    }

    #[tokio::test]
    async fn test_state_healing_configuration() {
        let client = Arc::new(TestSnapClient::new());
        let config = StateHealingConfig {
            max_healing_attempts: 10,
            healing_timeout: Duration::from_secs(60),
            enable_detailed_logging: true,
        };
        
        let healer = StateHealer::with_config(Arc::new(client), config);
        assert_eq!(healer.max_healing_attempts, 10);
        assert_eq!(healer.healing_timeout, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn test_state_healing_clear_results() {
        let client = Arc::new(TestSnapClient::new());
        let mut healer = StateHealer::new(client, 3, Duration::from_secs(10));
        
        // Add some healing results
        let missing_data = MissingData {
            missing_accounts: vec![Address::from([1u8; 20])],
            missing_storage: vec![],
            missing_byte_codes: vec![],
            missing_trie_nodes: vec![],
        };
        
        let _result = healer.heal_missing_data(missing_data).await;
        
        // Verify results exist
        let stats = healer.get_healing_stats();
        assert_eq!(stats.total_healing_attempts, 1);
        
        // Clear results
        healer.clear_healing_results();
        
        // Verify results are cleared
        let stats = healer.get_healing_stats();
        assert_eq!(stats.total_healing_attempts, 0);
    }
}