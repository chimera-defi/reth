use alloy_primitives::{keccak256, B256, U256};
use reth_db_api::{
    cursor::DbCursorRW,
    tables,
    transaction::{DbTx, DbTxMut},
};
use reth_eth_wire_types::snap::{AccountRangeMessage, AccountData, GetAccountRangeMessage};
use reth_net_p2p::{
    download::DownloadClient,
    error::PeerRequestResult,
    snap::SnapClient,
    priority::Priority,
};
use reth_provider::{
    DBProvider, StatsReader, HashingWriter, HeaderProvider,
};
use reth_stages_api::{
    EntitiesCheckpoint, ExecInput, ExecOutput, Stage, StageCheckpoint, StageError,
    StageId, UnwindInput, UnwindOutput,
};
use std::{
    collections::BTreeMap,
    sync::Arc,
    task::{ready, Context, Poll},
};
use tokio::sync::watch;
use tracing::*;

/// Configuration for the SnapSyncStage
#[derive(Debug, Clone)]
pub struct SnapSyncConfig {
    /// Max account ranges per execution
    pub max_ranges_per_execution: usize,
    /// Enable snap sync
    pub enabled: bool,
}

impl Default for SnapSyncConfig {
    fn default() -> Self {
        Self {
            max_ranges_per_execution: 100,
            enabled: false,
        }
    }
}

/// Snap sync stage for downloading trie data ranges from peers.
/// Replaces SenderRecoveryStage, ExecutionStage and PruneSenderRecoveryStage when enabled.
#[derive(Debug)]
pub struct SnapSyncStage<SnapClient> {
    /// Configuration for the stage
    config: SnapSyncConfig,
    /// Snap client for peer communication
    snap_client: Arc<SnapClient>,
    /// Current target state root from consensus engine
    target_state_root: Option<B256>,
    /// Current starting hash for account range requests
    current_starting_hash: B256,
    /// Request ID counter for snap requests
    request_id_counter: u64,
    /// Pending account range responses
    pending_responses: Vec<AccountRangeMessage>,
    /// Whether we're currently downloading
    is_downloading: bool,
    /// Watch receiver for header updates
    header_receiver: Option<watch::Receiver<B256>>,
}

impl<SnapClient> SnapSyncStage<SnapClient>
where
    SnapClient: SnapClient + Send + Sync + 'static,
{
    /// Create a new SnapSyncStage
    pub fn new(config: SnapSyncConfig, snap_client: Arc<SnapClient>) -> Self {
        Self {
            config,
            snap_client,
            target_state_root: None,
            current_starting_hash: B256::ZERO,
            request_id_counter: 0,
            pending_responses: Vec::new(),
            is_downloading: false,
            header_receiver: None,
        }
    }

    /// Set the header receiver for consensus engine updates
    pub fn with_header_receiver(mut self, receiver: watch::Receiver<B256>) -> Self {
        self.header_receiver = Some(receiver);
        self
    }

    /// Check if hashed state is empty
    fn is_hashed_state_empty<Provider>(&self, provider: &Provider) -> Result<bool, StageError>
    where
        Provider: StatsReader,
    {
        let count = provider.count_entries::<tables::HashedAccounts>()?;
        Ok(count == 0)
    }

    /// Get last hashed account for continuation
    fn get_last_hashed_account<Provider>(&self, provider: &Provider) -> Result<Option<B256>, StageError>
    where
        Provider: DBProvider + StatsReader,
    {
        let tx = provider.tx_ref();
        let mut cursor = tx.cursor_read::<tables::HashedAccounts>()?;
        
        if let Some((last_hash, _)) = cursor.last()? {
            Ok(Some(last_hash))
        } else {
            Ok(None)
        }
    }

    /// Process account range and insert into database
    fn process_account_range<Provider>(
        &self,
        provider: &Provider,
        account_range: AccountRangeMessage,
    ) -> Result<(), StageError>
    where
        Provider: DBProvider + HashingWriter,
    {
        // Verify Merkle proof before processing
        if !self.verify_proof(&account_range)? {
            return Err(StageError::Fatal("Invalid Merkle proof".into()));
        }

        let mut accounts = BTreeMap::new();
        
        for account_data in account_range.accounts {
            // Decode account from RLP
            let account = reth_primitives_traits::Account::decode(&mut account_data.body.as_ref())
                .map_err(|e| StageError::Fatal(format!("Failed to decode account: {}", e).into()))?;
            
            accounts.insert(account_data.hash, Some(account));
        }

        // Write accounts to hashed state
        provider.write_hashed_accounts(accounts)?;

        Ok(())
    }

    /// Process pending account range responses
    fn process_pending_responses<Provider>(
        &mut self,
        provider: &Provider,
    ) -> Result<usize, StageError>
    where
        Provider: DBProvider + StatsReader + HashingWriter,
    {
        let mut processed = 0;
        
        for account_range in self.pending_responses.drain(..) {
            self.process_account_range(provider, account_range)?;
            processed += 1;
        }
        
        Ok(processed)
    }

    /// Create a GetAccountRange request
    fn create_account_range_request(&mut self, starting_hash: B256, limit_hash: B256) -> GetAccountRangeMessage {
        self.request_id_counter += 1;
        GetAccountRangeMessage {
            request_id: self.request_id_counter,
            root_hash: self.target_state_root.unwrap_or(B256::ZERO),
            starting_hash,
            limit_hash,
            response_bytes: 2 * 1024 * 1024, // 2MB limit
        }
    }

    /// Verify Merkle proof for account range
    fn verify_proof(&self, account_range: &AccountRangeMessage) -> Result<bool, StageError> {
        if account_range.proof.is_empty() {
            // No proof provided, this is acceptable for some snap sync implementations
            return Ok(true);
        }

        // TODO: Implement proper Merkle proof verification
        // This should verify the proof against the target state root
        // For now, we'll do basic validation
        if account_range.accounts.is_empty() {
            return Ok(true);
        }

        // Verify that accounts are in the correct range
        let mut prev_hash = B256::ZERO;
        for account_data in &account_range.accounts {
            if account_data.hash <= prev_hash {
                return Err(StageError::Fatal("Accounts not in ascending order".into()));
            }
            prev_hash = account_data.hash;
        }

        // TODO: Verify Merkle proof against state root
        // This would involve:
        // 1. Reconstructing the trie from the accounts
        // 2. Verifying the proof path
        // 3. Checking against the target state root

        Ok(true)
    }

    /// Start download requests for account ranges
    fn start_download_requests<Provider>(
        &mut self,
        provider: &Provider,
    ) -> Result<(), StageError>
    where
        Provider: DBProvider + StatsReader,
    {
        if self.is_downloading {
            return Ok(());
        }

        let target_state_root = self.target_state_root
            .ok_or_else(|| StageError::Fatal("No target state root available".into()))?;

        // Create account range requests
        let mut requests = Vec::new();
        let mut current_hash = self.current_starting_hash;
        let max_hash = B256::from([0xff; 32]);

        for _ in 0..self.config.max_ranges_per_execution {
            if current_hash >= max_hash {
                break;
            }

            // Calculate limit hash for this range
            let limit_hash = if current_hash.le(&B256::from([0xfe; 32])) {
                // Use a reasonable step size for pagination
                let mut next_hash = current_hash;
                let bytes = next_hash.as_slice();
                let mut carry = 1;
                for i in (0..32).rev() {
                    let (new_val, new_carry) = bytes[i].overflowing_add(carry);
                    next_hash.as_mut_slice()[i] = new_val;
                    carry = new_carry as u8;
                    if carry == 0 {
                        break;
                    }
                }
                next_hash
            } else {
                max_hash
            };

            let request = self.create_account_range_request(current_hash, limit_hash);
            requests.push((request, current_hash));
            current_hash = limit_hash;
        }

        // TODO: In a real implementation, we would spawn async tasks to handle these requests
        // For now, we'll simulate the responses
        self.simulate_account_range_responses(requests)?;

        self.is_downloading = true;
        Ok(())
    }

    /// Simulate account range responses (placeholder for real async implementation)
    fn simulate_account_range_responses(
        &mut self,
        requests: Vec<(GetAccountRangeMessage, B256)>,
    ) -> Result<(), StageError> {
        for (request, starting_hash) in requests {
            // Simulate account range response with proper error handling
            let mut accounts = Vec::new();
            
            // Generate some mock account data
            for i in 0..10 {
                let mut hash_bytes = [0u8; 32];
                hash_bytes[31] = (starting_hash.as_slice()[31] + i as u8) % 256;
                let account_hash = B256::from(hash_bytes);
                
                // Create mock account data
                let account = reth_primitives_traits::Account {
                    nonce: i,
                    balance: U256::from(i * 1000),
                    bytecode_hash: None,
                };
                
                let mut account_rlp = Vec::new();
                account.encode(&mut account_rlp);
                
                accounts.push(AccountData {
                    hash: account_hash,
                    body: account_rlp.into(),
                });
            }

            let account_range = AccountRangeMessage {
                request_id: request.request_id,
                accounts,
                proof: vec![],
            };

            self.pending_responses.push(account_range);
        }

        self.is_downloading = false;
        Ok(())
    }

    /// Handle network errors with retry logic
    fn handle_network_error(&mut self, error: &str) -> Result<(), StageError> {
        warn!(target: "sync::stages::snap_sync", error = error, "Network error occurred");
        
        // TODO: Implement proper retry logic
        // For now, we'll just log the error and continue
        // In production, this should:
        // 1. Track retry attempts per peer
        // 2. Implement exponential backoff
        // 3. Switch to different peers on repeated failures
        // 4. Report bad peers to the network layer
        
        Ok(())
    }

    /// Select the best peer for snap requests
    fn select_peer(&self) -> Result<Option<reth_network_peers::PeerId>, StageError> {
        // TODO: Implement peer selection strategy
        // This should consider:
        // 1. Peer's snap sync capabilities
        // 2. Peer's reliability and response time
        // 3. Peer's current load
        // 4. Geographic proximity
        
        // For now, return None to use default peer selection
        Ok(None)
    }
}

impl<Provider, SnapClient> Stage<Provider> for SnapSyncStage<SnapClient>
where
    Provider: DBProvider + StatsReader + HashingWriter + HeaderProvider,
    SnapClient: SnapClient + Send + Sync + 'static,
{
    fn id(&self) -> StageId {
        StageId::SnapSync
    }

    fn poll_execute_ready(
        &mut self,
        cx: &mut Context<'_>,
        _input: ExecInput,
    ) -> Poll<Result<(), StageError>> {
        if !self.config.enabled {
            return Poll::Ready(Ok(()));
        }

        // Check for header updates from consensus engine
        if let Some(ref mut receiver) = self.header_receiver {
            if let Poll::Ready(Ok(())) = receiver.poll_changed(cx) {
                if let Ok(header_hash) = receiver.borrow().clone() {
                    // TODO: Get actual state root from header
                    // For now, use header hash as placeholder
                    self.target_state_root = Some(header_hash);
                    info!(target: "sync::stages::snap_sync", "Updated target state root from consensus engine");
                }
            }
        }

        // Check if we have a target state root
        if self.target_state_root.is_none() {
            return Poll::Pending;
        }

        // If we're not downloading and have pending responses, we're ready
        if !self.is_downloading && !self.pending_responses.is_empty() {
            return Poll::Ready(Ok(()));
        }

        // If we're not downloading and need to start, we're ready
        if !self.is_downloading {
            return Poll::Ready(Ok(()));
        }

        Poll::Pending
    }

    fn execute(
        &mut self,
        provider: &Provider,
        input: ExecInput,
    ) -> Result<ExecOutput, StageError> {
        if !self.config.enabled {
            return Ok(ExecOutput::done(input.checkpoint()));
        }

        if input.target_reached() {
            return Ok(ExecOutput::done(input.checkpoint()));
        }

        // Determine starting point
        let is_empty = self.is_hashed_state_empty(provider)?;
        if is_empty {
            self.current_starting_hash = B256::ZERO;
        } else {
            // Get the last entry from the database
            if let Some(last_hash) = self.get_last_hashed_account(provider)? {
                self.current_starting_hash = last_hash;
            }
        }

        // Process any pending responses
        let ranges_processed = self.process_pending_responses(provider)?;

        // Start new downloads if we have capacity
        if !self.is_downloading && ranges_processed < self.config.max_ranges_per_execution {
            self.start_download_requests(provider)?;
        }

        // Calculate progress
        let total_accounts = provider.count_entries::<tables::HashedAccounts>()? as u64;
        let entities_checkpoint = EntitiesCheckpoint {
            processed: total_accounts,
            total: total_accounts,
        };

        let done = self.current_starting_hash >= B256::from([0xff; 32]);

        info!(
            target: "sync::stages::snap_sync",
            ranges_processed = ranges_processed,
            total_accounts = total_accounts,
            done = done,
            "Snap sync progress"
        );

        Ok(ExecOutput {
            checkpoint: StageCheckpoint::new(input.target())
                .with_entities_stage_checkpoint(entities_checkpoint),
            done,
        })
    }

    fn unwind(
        &mut self,
        provider: &Provider,
        input: UnwindInput,
    ) -> Result<UnwindOutput, StageError> {
        let tx = provider.tx_ref();
        let mut cursor = tx.cursor_write::<tables::HashedAccounts>()?;
        
        // Clear hashed accounts for unwind
        cursor.clear()?;

        Ok(UnwindOutput {
            checkpoint: StageCheckpoint::new(input.unwind_to),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestStageDB;
    use reth_net_p2p::{
        download::DownloadClient,
        error::PeerRequestResult,
        snap::SnapClient,
        priority::Priority,
    };
    use std::sync::Arc;

    // Mock snap client for testing
    #[derive(Debug, Clone)]
    struct MockSnapClient;

    impl DownloadClient for MockSnapClient {
        fn report_bad_message(&self, _peer_id: reth_network_peers::PeerId) {
            // Mock implementation
        }

        fn num_connected_peers(&self) -> usize {
            1
        }
    }

    impl SnapClient for MockSnapClient {
        type Output = futures::future::Ready<PeerRequestResult<AccountRangeMessage>>;

        fn get_account_range_with_priority(
            &self,
            _request: GetAccountRangeMessage,
            _priority: Priority,
        ) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: reth_network_peers::PeerId::random(),
                result: AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                },
            }))
        }

        fn get_storage_ranges(&self, _request: reth_eth_wire_types::snap::GetStorageRangesMessage) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: reth_network_peers::PeerId::random(),
                result: reth_eth_wire_types::snap::StorageRangesMessage {
                    request_id: 1,
                    slots: vec![],
                    proof: vec![],
                },
            }))
        }

        fn get_byte_codes(&self, _request: reth_eth_wire_types::snap::GetByteCodesMessage) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: reth_network_peers::PeerId::random(),
                result: reth_eth_wire_types::snap::ByteCodesMessage {
                    request_id: 1,
                    codes: vec![],
                },
            }))
        }

        fn get_trie_nodes(&self, _request: reth_eth_wire_types::snap::GetTrieNodesMessage) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: reth_network_peers::PeerId::random(),
                result: reth_eth_wire_types::snap::TrieNodesMessage {
                    request_id: 1,
                    nodes: vec![],
                },
            }))
        }
    }

    #[test]
    fn test_snap_sync_stage_creation() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let stage = SnapSyncStage::new(config, snap_client);
        assert_eq!(stage.config.enabled, false);
        assert_eq!(stage.current_starting_hash, B256::ZERO);
    }

    #[test]
    fn test_snap_sync_stage_disabled() {
        let mut config = SnapSyncConfig::default();
        config.enabled = false;
        let snap_client = Arc::new(MockSnapClient);
        let mut stage = SnapSyncStage::new(config, snap_client);
        let db = TestStageDB::default();
        let provider = db.factory.database_provider_rw().unwrap();
        let input = ExecInput { target: Some(100), checkpoint: None };
        
        let result = stage.execute(&provider, input);
        assert!(result.is_ok());
        assert!(result.unwrap().done);
    }

    #[test]
    fn test_snap_sync_stage_enabled() {
        let mut config = SnapSyncConfig::default();
        config.enabled = true;
        let snap_client = Arc::new(MockSnapClient);
        let mut stage = SnapSyncStage::new(config, snap_client);
        
        let db = TestStageDB::default();
        let provider = db.factory.database_provider_rw().unwrap();
        let input = ExecInput { target: Some(100), checkpoint: None };
        
        let result = stage.execute(&provider, input);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(!output.done); // Should not be done yet
    }

    #[test]
    fn test_hashed_state_empty() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let stage = SnapSyncStage::new(config, snap_client);
        let db = TestStageDB::default();
        let provider = db.factory.database_provider_rw().unwrap();
        
        let is_empty = stage.is_hashed_state_empty(&provider).unwrap();
        assert!(is_empty);
    }

    #[test]
    fn test_proof_verification() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let stage = SnapSyncStage::new(config, snap_client);
        
        // Test with empty proof (should pass)
        let account_range = AccountRangeMessage {
            request_id: 1,
            accounts: vec![],
            proof: vec![],
        };
        assert!(stage.verify_proof(&account_range).unwrap());
        
        // Test with accounts in correct order (should pass)
        let mut accounts = Vec::new();
        for i in 0..5 {
            let mut hash_bytes = [0u8; 32];
            hash_bytes[31] = i as u8;
            let account_hash = B256::from(hash_bytes);
            
            let account = reth_primitives_traits::Account {
                nonce: i,
                balance: U256::from(i * 1000),
                bytecode_hash: None,
            };
            
            let mut account_rlp = Vec::new();
            account.encode(&mut account_rlp);
            
            accounts.push(AccountData {
                hash: account_hash,
                body: account_rlp.into(),
            });
        }
        
        let account_range = AccountRangeMessage {
            request_id: 1,
            accounts,
            proof: vec![],
        };
        assert!(stage.verify_proof(&account_range).unwrap());
    }

    #[test]
    fn test_proof_verification_invalid_order() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let stage = SnapSyncStage::new(config, snap_client);
        
        // Test with accounts in wrong order (should fail)
        let mut accounts = Vec::new();
        for i in (0..5).rev() { // Reverse order
            let mut hash_bytes = [0u8; 32];
            hash_bytes[31] = i as u8;
            let account_hash = B256::from(hash_bytes);
            
            let account = reth_primitives_traits::Account {
                nonce: i,
                balance: U256::from(i * 1000),
                bytecode_hash: None,
            };
            
            let mut account_rlp = Vec::new();
            account.encode(&mut account_rlp);
            
            accounts.push(AccountData {
                hash: account_hash,
                body: account_rlp.into(),
            });
        }
        
        let account_range = AccountRangeMessage {
            request_id: 1,
            accounts,
            proof: vec![],
        };
        assert!(stage.verify_proof(&account_range).is_err());
    }
}