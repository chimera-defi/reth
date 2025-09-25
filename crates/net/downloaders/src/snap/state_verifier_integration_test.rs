//! Integration test for state verification system.

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::snap::test_utils::TestSnapClient;
    use alloy_primitives::{B256, Bytes, Address, U256};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_state_verifier_integration() {
        // Create a test client
        let client = Arc::new(TestSnapClient::new());
        
        // Create state verifier with configuration
        let config = StateVerificationConfig {
            max_verification_attempts: 1000,
            verification_timeout: Duration::from_secs(30),
            enable_detailed_logging: true,
            enable_performance_metrics: true,
        };
        
        let mut verifier = StateVerifier::with_config(client, config);
        
        // Test account range verification
        let account_range = AccountRange {
            accounts: vec![
                (Address::from([1u8; 20]), AccountState {
                    nonce: U256::from(1),
                    balance: U256::from(1000),
                    code_hash: B256::from([1u8; 32]),
                    storage_root: B256::from([2u8; 32]),
                }),
                (Address::from([2u8; 20]), AccountState {
                    nonce: U256::from(2),
                    balance: U256::from(2000),
                    code_hash: B256::from([3u8; 32]),
                    storage_root: B256::from([4u8; 32]),
                }),
            ],
            proof: vec![B256::from([5u8; 32]), B256::from([6u8; 32])],
        };
        
        let state_root = B256::from([7u8; 32]);
        let result = verifier.verify_account_range(account_range, state_root).await;
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(verification_result.is_valid);
        assert_eq!(verification_result.verified_accounts, 2);
        
        // Test storage range verification
        let storage_range = StorageRange {
            account: Address::from([1u8; 20]),
            storage_slots: vec![
                (B256::from([1u8; 32]), B256::from([2u8; 32])),
                (B256::from([3u8; 32]), B256::from([4u8; 32])),
            ],
            proof: vec![B256::from([5u8; 32]), B256::from([6u8; 32])],
        };
        
        let storage_root = B256::from([7u8; 32]);
        let result = verifier.verify_storage_range(storage_range, storage_root).await;
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(verification_result.is_valid);
        assert_eq!(verification_result.verified_slots, 2);
        
        // Test byte code verification
        let byte_codes = vec![
            (B256::from([1u8; 32]), Bytes::from(vec![0x60, 0x60, 0x60, 0x60])),
            (B256::from([2u8; 32]), Bytes::from(vec![0x60, 0x60, 0x60, 0x60])),
        ];
        
        let result = verifier.verify_byte_codes(byte_codes).await;
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(verification_result.is_valid);
        assert_eq!(verification_result.verified_codes, 2);
        
        // Test trie node verification
        let trie_nodes = vec![
            (B256::from([1u8; 32]), Bytes::from(vec![0x80, 0x01])),
            (B256::from([2u8; 32]), Bytes::from(vec![0x80, 0x02])),
        ];
        
        let result = verifier.verify_trie_nodes(trie_nodes).await;
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(verification_result.is_valid);
        assert_eq!(verification_result.verified_nodes, 2);
        
        // Test state trie reconstruction
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
        
        let result = verifier.reconstruct_state_trie(state_data).await;
        assert!(result.is_ok());
        let state_trie = result.unwrap();
        assert!(!state_trie.is_empty());
        
        // Test state root verification
        let target_state_root = B256::from([7u8; 32]);
        let result = verifier.verify_state_root(state_trie, target_state_root).await;
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(verification_result.is_valid);
        
        // Test statistics
        let stats = verifier.get_verification_stats();
        assert!(stats.total_verifications > 0);
        assert!(stats.successful_verifications > 0);
    }

    #[tokio::test]
    async fn test_state_verifier_error_handling() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Test empty account range
        let empty_account_range = AccountRange {
            accounts: vec![],
            proof: vec![],
        };
        
        let state_root = B256::from([7u8; 32]);
        let result = verifier.verify_account_range(empty_account_range, state_root).await;
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(!verification_result.is_valid);
        assert_eq!(verification_result.verified_accounts, 0);
        
        // Test empty storage range
        let empty_storage_range = StorageRange {
            account: Address::from([1u8; 20]),
            storage_slots: vec![],
            proof: vec![],
        };
        
        let storage_root = B256::from([7u8; 32]);
        let result = verifier.verify_storage_range(empty_storage_range, storage_root).await;
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(!verification_result.is_valid);
        assert_eq!(verification_result.verified_slots, 0);
        
        // Test empty byte codes
        let empty_byte_codes = vec![];
        let result = verifier.verify_byte_codes(empty_byte_codes).await;
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(!verification_result.is_valid);
        assert_eq!(verification_result.verified_codes, 0);
        
        // Test empty trie nodes
        let empty_trie_nodes = vec![];
        let result = verifier.verify_trie_nodes(empty_trie_nodes).await;
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(!verification_result.is_valid);
        assert_eq!(verification_result.verified_nodes, 0);
    }

    #[tokio::test]
    async fn test_state_verifier_performance() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Test with large dataset
        let large_account_range = AccountRange {
            accounts: (0..1000).map(|i| {
                let mut address = [0u8; 20];
                address[0..4].copy_from_slice(&i.to_be_bytes());
                (Address::from(address), AccountState {
                    nonce: U256::from(i),
                    balance: U256::from(i * 1000),
                    code_hash: B256::from([i as u8; 32]),
                    storage_root: B256::from([(i + 1) as u8; 32]),
                })
            }).collect(),
            proof: vec![B256::from([5u8; 32]), B256::from([6u8; 32])],
        };
        
        let state_root = B256::from([7u8; 32]);
        let start_time = std::time::Instant::now();
        
        let result = verifier.verify_account_range(large_account_range, state_root).await;
        let verification_time = start_time.elapsed();
        
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(verification_result.is_valid);
        assert_eq!(verification_result.verified_accounts, 1000);
        
        // Verify performance is reasonable (should complete within 1 second for this test)
        assert!(verification_time.as_secs() < 1);
        
        // Test statistics
        let stats = verifier.get_verification_stats();
        assert_eq!(stats.total_verifications, 1);
        assert_eq!(stats.successful_verifications, 1);
        assert_eq!(stats.total_verified_accounts, 1000);
    }

    #[tokio::test]
    async fn test_state_verifier_configuration() {
        let client = Arc::new(TestSnapClient::new());
        
        let config = StateVerificationConfig {
            max_verification_attempts: 500,
            verification_timeout: Duration::from_secs(60),
            enable_detailed_logging: false,
            enable_performance_metrics: false,
        };
        
        let verifier = StateVerifier::with_config(client, config);
        
        // Verify configuration was applied
        assert_eq!(verifier.max_verification_attempts, 500);
        assert_eq!(verifier.verification_timeout, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn test_state_verifier_clear_results() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Add some verification results
        let account_range = AccountRange {
            accounts: vec![
                (Address::from([1u8; 20]), AccountState {
                    nonce: U256::from(1),
                    balance: U256::from(1000),
                    code_hash: B256::from([1u8; 32]),
                    storage_root: B256::from([2u8; 32]),
                }),
            ],
            proof: vec![B256::from([5u8; 32])],
        };
        
        let state_root = B256::from([7u8; 32]);
        let _ = verifier.verify_account_range(account_range, state_root).await;
        
        // Verify results were added
        let stats = verifier.get_verification_stats();
        assert_eq!(stats.total_verifications, 1);
        
        // Clear results
        verifier.clear_verification_results();
        
        // Verify results were cleared
        let stats = verifier.get_verification_stats();
        assert_eq!(stats.total_verifications, 0);
    }
}