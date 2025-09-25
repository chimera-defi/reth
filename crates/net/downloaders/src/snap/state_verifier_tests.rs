//! Tests for state verification system.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snap::test_utils::TestSnapClient;
    use alloy_primitives::{B256, Bytes, Address, U256};
    use std::collections::HashMap;

    #[test]
    fn test_state_verifier_creation() {
        let client = Arc::new(TestSnapClient::new());
        let verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        assert_eq!(verifier.max_verification_attempts, 1000);
        assert_eq!(verifier.verification_timeout, Duration::from_secs(30));
        assert!(verifier.verification_results.is_empty());
    }

    #[test]
    fn test_verify_account_range() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Create test account range data
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
        let result = verifier.verify_account_range(account_range, state_root);
        
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(verification_result.is_valid);
        assert_eq!(verification_result.verified_accounts, 2);
    }

    #[test]
    fn test_verify_storage_range() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Create test storage range data
        let storage_range = StorageRange {
            account: Address::from([1u8; 20]),
            storage_slots: vec![
                (B256::from([1u8; 32]), B256::from([2u8; 32])),
                (B256::from([3u8; 32]), B256::from([4u8; 32])),
            ],
            proof: vec![B256::from([5u8; 32]), B256::from([6u8; 32])],
        };
        
        let storage_root = B256::from([7u8; 32]);
        let result = verifier.verify_storage_range(storage_range, storage_root);
        
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(verification_result.is_valid);
        assert_eq!(verification_result.verified_slots, 2);
    }

    #[test]
    fn test_verify_byte_codes() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Create test byte code data
        let byte_codes = vec![
            (B256::from([1u8; 32]), Bytes::from(vec![0x60, 0x60, 0x60, 0x60])), // PUSH1 0x60
            (B256::from([2u8; 32]), Bytes::from(vec![0x60, 0x60, 0x60, 0x60])), // PUSH1 0x60
        ];
        
        let result = verifier.verify_byte_codes(byte_codes);
        
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(verification_result.is_valid);
        assert_eq!(verification_result.verified_codes, 2);
    }

    #[test]
    fn test_verify_trie_nodes() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Create test trie node data
        let trie_nodes = vec![
            (B256::from([1u8; 32]), Bytes::from(vec![0x80, 0x01])), // Extension node
            (B256::from([2u8; 32]), Bytes::from(vec![0x80, 0x02])), // Branch node
        ];
        
        let result = verifier.verify_trie_nodes(trie_nodes);
        
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(verification_result.is_valid);
        assert_eq!(verification_result.verified_nodes, 2);
    }

    #[test]
    fn test_reconstruct_state_trie() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Create test state data
        let account_ranges = vec![
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
        ];
        
        let storage_ranges = vec![
            StorageRange {
                account: Address::from([1u8; 20]),
                storage_slots: vec![
                    (B256::from([1u8; 32]), B256::from([2u8; 32])),
                ],
                proof: vec![B256::from([6u8; 32])],
            },
        ];
        
        let byte_codes = vec![
            (B256::from([1u8; 32]), Bytes::from(vec![0x60, 0x60])),
        ];
        
        let trie_nodes = vec![
            (B256::from([1u8; 32]), Bytes::from(vec![0x80, 0x01])),
        ];
        
        let state_data = StateData {
            account_ranges,
            storage_ranges,
            byte_codes,
            trie_nodes,
        };
        
        let result = verifier.reconstruct_state_trie(state_data);
        
        assert!(result.is_ok());
        let state_trie = result.unwrap();
        assert!(!state_trie.is_empty());
    }

    #[test]
    fn test_verify_state_root() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Create test state trie
        let state_trie = HashMap::from([
            (B256::from([1u8; 32]), Bytes::from(vec![0x80, 0x01])),
            (B256::from([2u8; 32]), Bytes::from(vec![0x80, 0x02])),
        ]);
        
        let target_state_root = B256::from([7u8; 32]);
        let result = verifier.verify_state_root(state_trie, target_state_root);
        
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(verification_result.is_valid);
    }

    #[test]
    fn test_verify_invalid_account_range() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Create invalid account range data (empty proof)
        let account_range = AccountRange {
            accounts: vec![
                (Address::from([1u8; 20]), AccountState {
                    nonce: U256::from(1),
                    balance: U256::from(1000),
                    code_hash: B256::from([1u8; 32]),
                    storage_root: B256::from([2u8; 32]),
                }),
            ],
            proof: vec![], // Empty proof should fail verification
        };
        
        let state_root = B256::from([7u8; 32]);
        let result = verifier.verify_account_range(account_range, state_root);
        
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(!verification_result.is_valid);
        assert_eq!(verification_result.verified_accounts, 0);
    }

    #[test]
    fn test_verify_invalid_storage_range() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Create invalid storage range data (empty proof)
        let storage_range = StorageRange {
            account: Address::from([1u8; 20]),
            storage_slots: vec![
                (B256::from([1u8; 32]), B256::from([2u8; 32])),
            ],
            proof: vec![], // Empty proof should fail verification
        };
        
        let storage_root = B256::from([7u8; 32]);
        let result = verifier.verify_storage_range(storage_range, storage_root);
        
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(!verification_result.is_valid);
        assert_eq!(verification_result.verified_slots, 0);
    }

    #[test]
    fn test_verify_invalid_byte_codes() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Create invalid byte code data (empty byte codes)
        let byte_codes = vec![
            (B256::from([1u8; 32]), Bytes::from(vec![])), // Empty byte code
        ];
        
        let result = verifier.verify_byte_codes(byte_codes);
        
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(!verification_result.is_valid);
        assert_eq!(verification_result.verified_codes, 0);
    }

    #[test]
    fn test_verify_invalid_trie_nodes() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Create invalid trie node data (empty nodes)
        let trie_nodes = vec![
            (B256::from([1u8; 32]), Bytes::from(vec![])), // Empty trie node
        ];
        
        let result = verifier.verify_trie_nodes(trie_nodes);
        
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(!verification_result.is_valid);
        assert_eq!(verification_result.verified_nodes, 0);
    }

    #[test]
    fn test_verify_invalid_state_root() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Create test state trie
        let state_trie = HashMap::from([
            (B256::from([1u8; 32]), Bytes::from(vec![0x80, 0x01])),
        ]);
        
        let target_state_root = B256::from([7u8; 32]);
        let wrong_state_root = B256::from([8u8; 32]); // Wrong state root
        
        let result = verifier.verify_state_root(state_trie, wrong_state_root);
        
        assert!(result.is_ok());
        let verification_result = result.unwrap();
        assert!(!verification_result.is_valid);
    }

    #[test]
    fn test_get_verification_stats() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Add some verification results
        verifier.verification_results.insert("test1".to_string(), VerificationResult {
            is_valid: true,
            verified_accounts: 10,
            verified_slots: 5,
            verified_codes: 3,
            verified_nodes: 7,
            verification_time: Duration::from_millis(100),
        });
        
        let stats = verifier.get_verification_stats();
        assert_eq!(stats.total_verifications, 1);
        assert_eq!(stats.successful_verifications, 1);
        assert_eq!(stats.failed_verifications, 0);
        assert_eq!(stats.total_verified_accounts, 10);
        assert_eq!(stats.total_verified_slots, 5);
        assert_eq!(stats.total_verified_codes, 3);
        assert_eq!(stats.total_verified_nodes, 7);
    }

    #[test]
    fn test_clear_verification_results() {
        let client = Arc::new(TestSnapClient::new());
        let mut verifier = StateVerifier::new(client, 1000, Duration::from_secs(30));
        
        // Add some verification results
        verifier.verification_results.insert("test1".to_string(), VerificationResult {
            is_valid: true,
            verified_accounts: 10,
            verified_slots: 5,
            verified_codes: 3,
            verified_nodes: 7,
            verification_time: Duration::from_millis(100),
        });
        
        assert_eq!(verifier.verification_results.len(), 1);
        
        verifier.clear_verification_results();
        
        assert_eq!(verifier.verification_results.len(), 0);
    }
}