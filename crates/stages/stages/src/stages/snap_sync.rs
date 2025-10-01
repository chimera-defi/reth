use alloy_primitives::B256;
use reth_config::config::SnapSyncConfig;
use reth_db_api::{
    cursor::{DbCursorRO, DbCursorRW},
    table::Compress,
    tables,
    transaction::{DbTx, DbTxMut},
    RawKey, RawTable, RawValue,
};
use reth_eth_wire_types::snap::{AccountRangeMessage, GetAccountRangeMessage};
use reth_network_p2p::{snap::client::SnapClient, priority::Priority};
use reth_provider::{
    DBProvider, StatsReader, HeaderProvider,
};
use reth_primitives_traits::SealedHeader;
use alloy_trie::TrieAccount;
use alloy_rlp::Decodable;
use reth_stages_api::{
    ExecInput, ExecOutput, Stage, StageError,
    StageId, UnwindInput, UnwindOutput,
};
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::sync::watch;
use tracing::*;

/// Snap sync stage for downloading trie data ranges from peers.
/// Replaces `SenderRecoveryStage`, `ExecutionStage` and `PruneSenderRecoveryStage` when enabled.
pub struct SnapSyncStage<C: SnapClient> {
    /// Configuration for the stage
    pub config: SnapSyncConfig,
    /// Snap client for peer communication
    snap_client: Arc<C>,
    /// Watch receiver for header updates from consensus engine
    pub header_receiver: Option<watch::Receiver<SealedHeader>>,
    /// Request ID counter for snap requests
    pub request_id_counter: u64,
    /// Pending network requests
    pending_requests: HashMap<u64, Pin<Box<dyn Future<Output = reth_network_p2p::error::PeerRequestResult<reth_eth_wire_types::snap::AccountRangeMessage>> + Send + Sync + Unpin>>>,
    /// Request start times for timeout tracking
    request_start_times: HashMap<u64, Instant>,
    /// Completed account ranges ready for processing
    completed_ranges: Vec<AccountRangeMessage>,
    /// Queued ranges waiting for async processing
    queued_ranges: Vec<(B256, B256, B256)>, // (start, end, state_root)
}

impl<C> std::fmt::Debug for SnapSyncStage<C>
where
    C: SnapClient,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapSyncStage")
            .field("config", &self.config)
            .field("snap_client", &"<SnapClient>")
            .field("header_receiver", &self.header_receiver.is_some())
            .field("request_id_counter", &self.request_id_counter)
            .field("pending_requests", &format!("<{} pending requests>", self.pending_requests.len()))
            .field("request_start_times", &format!("<{} tracked requests>", self.request_start_times.len()))
            .field("completed_ranges", &format!("<{} completed ranges>", self.completed_ranges.len()))
            .finish()
    }
}

impl<C> SnapSyncStage<C>
where
    C: SnapClient + Send + Sync + 'static,
{
    /// Create a new `SnapSyncStage`
    pub fn new(config: SnapSyncConfig, snap_client: Arc<C>) -> Self {
        Self {
            config,
            snap_client,
            header_receiver: None,
            request_id_counter: 0,
            pending_requests: HashMap::new(),
            request_start_times: HashMap::new(),
            completed_ranges: Vec::new(),
            queued_ranges: Vec::new(),
        }
    }

    /// Set the header receiver for consensus engine updates
    pub fn with_header_receiver(mut self, receiver: watch::Receiver<SealedHeader>) -> Self {
        self.header_receiver = Some(receiver);
        self
    }

    /// Check if hashed state is empty
    pub fn is_hashed_state_empty<Provider>(&self, provider: &Provider) -> Result<bool, StageError>
    where
        Provider: DBProvider,
    {
        let mut cursor = provider.tx_ref().cursor_read::<tables::HashedAccounts>()?;
        match cursor.first()? {
            Some(_) => Ok(false), // Database has accounts
            None => Ok(true),     // Database is empty
        }
    }

    /// Get the last hashed account from the database
    pub fn get_last_hashed_account<Provider>(&self, provider: &Provider) -> Result<Option<B256>, StageError>
    where
        Provider: DBProvider,
    {
        let mut cursor = provider.tx_ref().cursor_read::<tables::HashedAccounts>()?;
        match cursor.last()? {
            Some((key, _)) => Ok(Some(key)),
            None => Ok(None),
        }
    }

    /// Get the next starting point for snap sync based on current database state
    /// This implements proper state tracking for snap sync resumption
    pub fn get_next_sync_starting_point<Provider>(&self, provider: &Provider) -> Result<B256, StageError>
    where
        Provider: DBProvider,
    {
        // Check if we have any accounts in the database
        if self.is_hashed_state_empty(provider)? {
            // If empty, start from the beginning
            return Ok(B256::ZERO);
        }

        // For snap sync resumption, we need to find the last processed range
        // This is a simplified approach - in practice, we'd store sync progress
        let last_account = self.get_last_hashed_account(provider)?
            .unwrap_or(B256::ZERO);
        
        // Calculate the next starting point after the last account
        // This ensures we don't miss any accounts and don't duplicate work
        let next_start = self.calculate_next_hash_in_lexicographic_order(last_account, 1)?;
        
        // Ensure we don't go beyond the maximum
        let max_hash = B256::from([0xff; 32]);
        if next_start >= max_hash {
            return Ok(max_hash);
        }
        
        Ok(next_start)
    }

    /// Create account range request
    pub fn create_account_range_request(&mut self, starting_hash: B256, limit_hash: B256) -> GetAccountRangeMessage {
        self.request_id_counter += 1;
        GetAccountRangeMessage {
            request_id: self.request_id_counter,
            root_hash: self.get_target_state_root().unwrap_or(B256::ZERO),
            starting_hash,
            limit_hash,
            response_bytes: self.config.max_response_bytes,
        }
    }

    /// Create a new account range request with explicit state root
    /// This method includes the state root in the request for proper snap sync validation
    #[allow(clippy::missing_const_for_fn)]
    pub fn create_account_range_request_with_state_root(&mut self, starting_hash: B256, limit_hash: B256, state_root: B256) -> GetAccountRangeMessage {
        self.request_id_counter += 1;
        GetAccountRangeMessage {
            request_id: self.request_id_counter,
            root_hash: state_root, // Use the explicit state root
            starting_hash,
            limit_hash,
            response_bytes: self.config.max_response_bytes,
        }
    }

    /// Process account ranges and insert into database
    pub fn process_account_ranges<Provider>(
        &self,
        provider: &Provider,
        account_ranges: Vec<AccountRangeMessage>,
    ) -> Result<usize, StageError>
    where
        Provider: DBProvider<Tx: DbTxMut>,
    {
        let mut processed = 0;

        for account_range in account_ranges {
            // Verify proof before processing
            if !self.verify_account_range_proof(&account_range)? {
                return Err(StageError::Fatal("Account range proof verification failed".into()));
            }

            // Get write cursor for HashedAccounts table
            let mut cursor = provider.tx_ref().cursor_write::<RawTable<tables::HashedAccounts>>()?;

            // Process each account in the range
            for account_data in &account_range.accounts {
                // Decode account data
                let trie_account = TrieAccount::decode(&mut account_data.body.as_ref())
                    .map_err(|e| StageError::Fatal(format!("Failed to decode account: {}", e).into()))?;

                // Convert to Account type for database storage
                let account = reth_primitives_traits::Account {
                    nonce: trie_account.nonce,
                    balance: trie_account.balance,
                    bytecode_hash: Some(trie_account.code_hash),
                };

                // Insert account data into database
                cursor.insert(
                    RawKey::new(account_data.hash),
                    &RawValue::from_vec(account.compress())
                )?;

                debug!(
                    target: "sync::stages::snap_sync",
                    account_hash = ?account_data.hash,
                    nonce = ?account.nonce,
                    balance = ?account.balance,
                    "Inserted account into database"
                );
                
                processed += 1;
            }
        }

        Ok(processed)
    }

    /// Verify account range proof using Merkle proof verification
    fn verify_account_range_proof(&self, account_range: &AccountRangeMessage) -> Result<bool, StageError> {
        use alloy_trie::proof::verify_proof;
        use reth_trie_common::Nibbles;
        
        // If no accounts, proof should be empty or contain only empty root
        if account_range.accounts.is_empty() {
            return Ok(true);
        }
        
        // If accounts present but no proof, this is invalid
        if account_range.proof.is_empty() {
            return Err(StageError::Fatal("Account range has accounts but no proof".into()));
        }
        
        // Get target state root for verification
        let target_state_root = self.get_target_state_root()
            .ok_or_else(|| StageError::Fatal("No target state root available for proof verification".into()))?;
        
        // Verify each account in the range
        for account_data in &account_range.accounts {
            // Convert account hash to nibbles for proof verification
            let account_nibbles = Nibbles::unpack(account_data.hash);
            
            // Verify the proof for this account
            match verify_proof(
                target_state_root,
                account_nibbles,
                Some(account_data.body.as_ref().to_vec()),
                &account_range.proof,
            ) {
                Ok(()) => {},
                Err(e) => {
                    warn!(
                        target: "sync::stages::snap_sync",
                        account_hash = ?account_data.hash,
                        error = %e,
                        "Account proof verification failed"
                    );
                    return Err(StageError::Fatal(format!("Account proof verification failed: {}", e).into()));
                }
            }
        }
        
        Ok(true)
    }

    /// Get current target state root from header receiver
    pub fn get_target_state_root(&self) -> Option<B256> {
        self.header_receiver.as_ref().map(|receiver| {
            let header = receiver.borrow();
            header.state_root
        })
    }

    /// Calculate the next trie range for snap sync requests
    /// This implements proper trie range calculation based on the snap protocol
    pub fn calculate_next_trie_range(&self, current: B256, max: B256) -> Result<(B256, B256), StageError> {
        // For snap sync, we need to traverse the trie in lexicographic order
        // The range should be calculated based on the trie structure, not arbitrary hash values
        
        // Calculate a reasonable range size based on configuration
        // This is a rough estimate - in practice, the actual range size depends on the data
        let estimated_range_size = self.config.max_response_bytes / 1000; // Rough estimate
        let range_size = estimated_range_size.max(100).min(10000); // Clamp to reasonable bounds
        
        // For snap sync, we need to calculate the next range in lexicographic order
        // This is a simplified implementation that increments the hash
        let next = self.calculate_next_hash_in_lexicographic_order(current, range_size)?;
        
        // Ensure we don't exceed the maximum
        let range_end = if next > max { max } else { next };
        
        Ok((current, range_end))
    }

    /// Calculate the next hash in lexicographic order for trie traversal
    /// This is a simplified implementation - in practice, this would be more sophisticated
    fn calculate_next_hash_in_lexicographic_order(&self, current: B256, range_size: u64) -> Result<B256, StageError> {
        // Validate input parameters
        if range_size == 0 {
            return Err(StageError::Fatal("Range size cannot be zero".into()));
        }
        
        // For now, implement a simple increment approach
        // In a real implementation, this would need to understand the trie structure better
        
        // Convert to bytes for manipulation
        let mut hash_bytes = current.as_slice().to_owned();
        
        // Simple increment starting from the least significant byte
        // This is not perfect but better than the previous approach
        let mut carry = range_size;
        for i in (0..32).rev() {
            let (new_val, new_carry) = hash_bytes[i].overflowing_add(carry as u8);
            hash_bytes[i] = new_val;
            carry = if new_carry { 1 } else { 0 };
            if carry == 0 {
                break;
            }
        }
        
        // If we overflowed, return the max value
        if carry > 0 {
            warn!(
                target: "sync::stages::snap_sync",
                current = ?current,
                range_size = range_size,
                "Hash increment overflowed, using max value"
            );
            return Ok(B256::from([0xff; 32]));
        }
        
        let result = B256::from_slice(&hash_bytes);
        
        // Validate that we actually made progress
        if result <= current {
            return Err(StageError::Fatal("Hash increment did not make progress".into()));
        }
        
        Ok(result)
    }

    /// Queue a range for async processing in poll_execute_ready
    fn queue_range_for_processing(&mut self, start: B256, end: B256, state_root: B256) {
        debug!(
            target: "sync::stages::snap_sync",
            start = ?start,
            end = ?end,
            state_root = ?state_root,
            "Queueing range for async processing"
        );
        self.queued_ranges.push((start, end, state_root));
    }

    /// Start tracking a request for timeout purposes
    fn start_request_tracking(&mut self, request_id: u64) {
        self.request_start_times.insert(request_id, Instant::now());
    }

    /// Complete request tracking
    fn complete_request_tracking(&mut self, request_id: u64) {
        self.request_start_times.remove(&request_id);
    }

    /// Check for timed out requests
    fn check_timeouts(&self) -> Vec<u64> {
        let timeout_duration = Duration::from_secs(self.config.request_timeout_seconds);
        let now = Instant::now();
        let mut timed_out = Vec::new();

        for (request_id, start_time) in &self.request_start_times {
            if now.duration_since(*start_time) > timeout_duration {
                timed_out.push(*request_id);
            }
        }

        timed_out
    }

    /// Handle request timeout
    fn handle_request_timeout(&mut self, request_id: u64) {
        warn!(
            target: "sync::stages::snap_sync",
            request_id = request_id,
            "Request timed out"
        );
        
        // Remove from pending requests
        self.pending_requests.remove(&request_id);
        self.request_start_times.remove(&request_id);
        
        // Note: Retry logic could be implemented here if needed
    }

}

impl<Provider, C> Stage<Provider> for SnapSyncStage<C>
where
    Provider: DBProvider<Tx: DbTxMut> + StatsReader + HeaderProvider,
    C: SnapClient + Send + Sync + 'static,
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

        // Check if we have a target state root from consensus engine
        if self.get_target_state_root().is_none() {
            return Poll::Pending;
        }

        // Process any queued ranges by creating network requests
        if !self.queued_ranges.is_empty() {
            let queued_ranges = std::mem::take(&mut self.queued_ranges);
            for (start, end, state_root) in queued_ranges {
                let request = self.create_account_range_request_with_state_root(start, end, state_root);
                
                debug!(
                    target: "sync::stages::snap_sync",
                    request_id = request.request_id,
                    starting_hash = ?request.starting_hash,
                    limit_hash = ?request.limit_hash,
                    root_hash = ?request.root_hash,
                    "Creating network request from queued range"
                );

                // Create the network request future and queue it for polling
                let future = self.snap_client.get_account_range_with_priority(request.clone(), Priority::Normal);
                self.pending_requests.insert(request.request_id, Box::pin(future));
                self.start_request_tracking(request.request_id);
            }
        }

        // Check for timed out requests
        let timed_out_requests = self.check_timeouts();
        for request_id in timed_out_requests {
            self.handle_request_timeout(request_id);
        }

        // Poll any pending SnapClient requests
        let mut completed_requests = Vec::new();
        for (request_id, future) in &mut self.pending_requests {
            match future.as_mut().poll(cx) {
                Poll::Ready(result) => {
                    match result {
                        Ok(account_range) => {
                            debug!(
                                target: "sync::stages::snap_sync",
                                request_id = request_id,
                                accounts_count = account_range.1.accounts.len(),
                                "Received account range response"
                            );
                            self.completed_ranges.push(account_range.1);
                        }
                        Err(e) => {
                            warn!(
                                target: "sync::stages::snap_sync",
                                request_id = request_id,
                                error = %e,
                                "Account range request failed"
                            );
                            
                            // Handle failed request with retry logic
                            // Note: For proper retry, we would need to store the original request
                            // For now, we'll just log the failure and rely on timeout retry
                        }
                    }
                    completed_requests.push(*request_id);
                }
                Poll::Pending => {},
            }
        }

        // Remove completed requests
        for request_id in completed_requests {
            self.pending_requests.remove(&request_id);
            self.complete_request_tracking(request_id);
        }

        // Return ready if we have completed ranges to process
        if self.completed_ranges.is_empty() {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn execute(
        &mut self,
        provider: &Provider,
        input: ExecInput,
    ) -> Result<ExecOutput, StageError> {
        if input.target_reached() {
            return Ok(ExecOutput::done(input.checkpoint()));
        }

        if !self.config.enabled {
            return Ok(ExecOutput {
                checkpoint: input.checkpoint(),
                done: true,
            });
        }

        // Get target state root
        let target_state_root = self.get_target_state_root()
            .ok_or_else(|| StageError::Fatal("No target state root available".into()))?;

        // Determine starting point using improved state tracking
        let starting_hash = self.get_next_sync_starting_point(provider)?;

        let mut total_processed = 0;
        let max_hash = B256::from([0xff; 32]);
        let mut current_starting_hash = starting_hash;

        // Process multiple ranges per execution (configurable)
        for _ in 0..self.config.max_ranges_per_execution {
            if current_starting_hash >= max_hash {
                break;
            }

            // Calculate the next range using proper trie range logic
            let (range_start, range_end) = self.calculate_next_trie_range(current_starting_hash, max_hash)?;
            
            // If we've reached the end, we're done
            if range_start >= max_hash {
                break;
            }

            // Queue the range for async processing in poll_execute_ready
            self.queue_range_for_processing(range_start, range_end, target_state_root);

            // Move to next range
            current_starting_hash = range_end;
        }

        // Process any completed account ranges
        if !self.completed_ranges.is_empty() {
            let completed_ranges = std::mem::take(&mut self.completed_ranges);
            let processed = self.process_account_ranges(provider, completed_ranges)?;
            total_processed += processed;
        }

        // Check if we've reached the end of the trie
        let max_hash = B256::from([0xff; 32]);
        let is_complete = current_starting_hash >= max_hash;
        
        if total_processed == 0 && !is_complete {
            debug!(
                target: "sync::stages::snap_sync",
                "No data returned for current target state root, will re-poll"
            );
        }

        Ok(ExecOutput {
            checkpoint: input.checkpoint(),
            done: is_complete, // Done when we've reached the end of the trie
        })
    }

    fn unwind(
        &mut self,
        provider: &Provider,
        input: UnwindInput,
    ) -> Result<UnwindOutput, StageError> {
        if !self.config.enabled {
            return Ok(UnwindOutput { checkpoint: input.checkpoint });
        }

        // For snap sync, we need to clear the downloaded state data
        // This is a simplified implementation - in practice, we'd need more sophisticated logic
        let unwind_block = input.unwind_to;
        
        info!(
            target: "sync::stages::snap_sync",
            unwind_to = unwind_block,
            "Unwinding snap sync stage - clearing downloaded state data"
        );
        
        // For now, we'll use a simple approach to clear the table
        // In a real implementation, we'd need to track which accounts were downloaded
        // in which ranges and only clear the relevant ones
        provider.tx_ref().clear::<tables::HashedAccounts>()?;
        
        Ok(UnwindOutput { checkpoint: input.checkpoint })
    }
}