use alloy_primitives::B256;
use reth_config::config::SnapSyncConfig;
use reth_db_api::{
    cursor::DbCursorRO,
    tables,
    transaction::DbTx,
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
    ExecInput, ExecOutput, Stage, StageCheckpoint, StageError,
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
/// Replaces SenderRecoveryStage, ExecutionStage and PruneSenderRecoveryStage when enabled.
pub struct SnapSyncStage<C: SnapClient> {
    /// Configuration for the stage
    config: SnapSyncConfig,
    /// Snap client for peer communication
    snap_client: Arc<C>,
    /// Watch receiver for header updates from consensus engine
    header_receiver: Option<watch::Receiver<SealedHeader>>,
    /// Request ID counter for snap requests
    request_id_counter: u64,
    /// Current range being processed
    current_range: Option<(B256, B256)>,
    /// Pending network requests
    pending_requests: HashMap<u64, Pin<Box<dyn Future<Output = reth_network_p2p::error::PeerRequestResult<reth_eth_wire_types::snap::AccountRangeMessage>> + Send + Sync + Unpin>>>,
    /// Request start times for timeout tracking
    request_start_times: HashMap<u64, Instant>,
    /// Completed account ranges ready for processing
    completed_ranges: Vec<AccountRangeMessage>,
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
            .field("current_range", &self.current_range)
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
    /// Create a new SnapSyncStage
    pub fn new(config: SnapSyncConfig, snap_client: Arc<C>) -> Self {
        Self {
            config,
            snap_client,
            header_receiver: None,
            request_id_counter: 0,
            current_range: None,
            pending_requests: HashMap::new(),
            request_start_times: HashMap::new(),
            completed_ranges: Vec::new(),
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

    /// Process account ranges and return processed count
    pub fn process_account_ranges(
        &self,
        account_ranges: Vec<AccountRangeMessage>,
    ) -> Result<usize, StageError> {
        let mut processed = 0;

        for account_range in account_ranges {
            // Verify proof before processing
            if !self.verify_account_range_proof(&account_range)? {
                return Err(StageError::Fatal("Account range proof verification failed".into()));
            }

            // Process each account in the range
            for account_data in &account_range.accounts {
                // Decode account data
                let _trie_account = TrieAccount::decode(&mut account_data.body.as_ref())
                    .map_err(|e| StageError::Fatal(format!("Failed to decode account: {}", e).into()))?;

                // For now, just count the processed accounts
                // In a real implementation, this would insert into the database
                // or return the data for the pipeline to handle
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
                Ok(()) => continue,
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
        self.header_receiver.as_ref().and_then(|receiver| {
            let header = receiver.borrow();
            Some(header.state_root)
        })
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
    fn check_timeouts(&mut self) -> Vec<u64> {
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
    Provider: DBProvider + StatsReader + HeaderProvider,
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


        // Check for timed out requests
        let timed_out_requests = self.check_timeouts();
        for request_id in timed_out_requests {
            self.handle_request_timeout(request_id);
        }

        // Poll any pending SnapClient requests
        let mut completed_requests = Vec::new();
        for (request_id, future) in self.pending_requests.iter_mut() {
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
                Poll::Pending => continue,
            }
        }

        // Remove completed requests
        for request_id in completed_requests {
            self.pending_requests.remove(&request_id);
            self.complete_request_tracking(request_id);
        }

        // Return ready if we have completed ranges to process
        if !self.completed_ranges.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn execute(
        &mut self,
        provider: &Provider,
        input: ExecInput,
    ) -> Result<ExecOutput, StageError> {
        if !self.config.enabled {
            return Ok(ExecOutput {
                checkpoint: input.checkpoint(),
                done: true,
            });
        }

        // Get target state root
        let _target_state_root = self.get_target_state_root()
            .ok_or_else(|| StageError::Fatal("No target state root available".into()))?;

        // Determine starting point
        let starting_hash = if self.is_hashed_state_empty(provider)? {
            B256::ZERO // Start from beginning if empty
        } else {
            self.get_last_hashed_account(provider)?
                .unwrap_or(B256::ZERO)
        };

        let mut total_processed = 0;
        let max_hash = B256::from([0xff; 32]);
        let mut current_starting_hash = starting_hash;

        // Process multiple ranges per execution (configurable)
        for _ in 0..self.config.max_ranges_per_execution {
            if current_starting_hash >= max_hash {
                break;
            }

            // Calculate the range for this request using configurable range size
            // For now, use a simple approach - process one range at a time
            let limit_hash = if current_starting_hash == B256::ZERO {
                // Start with a small range for the first request
                B256::from([0x10; 32]) // 1/16th of the hash space
            } else {
                // For subsequent requests, move to the next range
                // This is a simplified approach - in a real implementation,
                // you would need proper hash arithmetic
                max_hash
            };

            // Create account range request
            let request = self.create_account_range_request(current_starting_hash, limit_hash);
            
            // Create and queue the network request
            debug!(
                target: "sync::stages::snap_sync",
                request_id = request.request_id,
                starting_hash = ?request.starting_hash,
                limit_hash = ?request.limit_hash,
                root_hash = ?request.root_hash,
                "Creating account range request"
            );

            // Create the network request future and queue it for polling
            let future = self.snap_client.get_account_range_with_priority(request.clone(), Priority::Normal);
            self.pending_requests.insert(request.request_id, Box::pin(future));
            self.start_request_tracking(request.request_id);

            // Move to next range
            current_starting_hash = limit_hash;
        }

        // Process any completed account ranges
        if !self.completed_ranges.is_empty() {
            let completed_ranges = std::mem::take(&mut self.completed_ranges);
            let processed = self.process_account_ranges(completed_ranges)?;
            total_processed += processed;
        }

        // If no data was returned for current target state root, we need to re-poll
        if total_processed == 0 {
            debug!(
                target: "sync::stages::snap_sync",
                "No data returned for current target state root, will re-poll"
            );
        }

        Ok(ExecOutput {
            checkpoint: input.checkpoint(),
            done: total_processed == 0, // Done when no more data
        })
    }

    fn unwind(
        &mut self,
        _provider: &Provider,
        _input: UnwindInput,
    ) -> Result<UnwindOutput, StageError> {
        // Snap sync doesn't need unwinding as it's a one-time sync
        Ok(UnwindOutput {
            checkpoint: StageCheckpoint::new(0),
        })
    }
}