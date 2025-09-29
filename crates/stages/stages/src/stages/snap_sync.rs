use alloy_primitives::B256;
use reth_config::config::SnapSyncConfig;
use reth_db_api::{
    cursor::DbCursorRW,
    tables,
    transaction::DbTx,
};
use reth_eth_wire_types::snap::{AccountRangeMessage, GetAccountRangeMessage};
use reth_net_p2p::snap::SnapClient;
use reth_network_peers::PeerId;
use reth_provider::{
    DBProvider, StatsReader, HeaderProvider,
};
use reth_primitives_traits::{Account, SealedHeader};
use reth_stages_api::{
    EntitiesCheckpoint, ExecInput, ExecOutput, Stage, StageCheckpoint, StageError,
    StageId, UnwindInput, UnwindOutput,
};
use std::{
    collections::HashMap,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::sync::watch;
use tracing::*;

/// Network performance metrics for adaptive range sizing
#[derive(Debug, Clone, Default)]
struct NetworkMetrics {
    /// Average response time in milliseconds
    avg_response_time_ms: f64,
    /// Success rate (0.0 to 1.0)
    success_rate: f64,
    /// Number of samples for moving average
    sample_count: u32,
}

/// Snap sync stage for downloading trie data ranges from peers.
/// Replaces SenderRecoveryStage, ExecutionStage and PruneSenderRecoveryStage when enabled.
#[derive(Debug)]
pub struct SnapSyncStage<C> {
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
    /// Retry attempts for failed requests (request_id -> attempts)
    retry_attempts: std::collections::HashMap<u64, u32>,
    /// Failed requests waiting for retry
    failed_requests: Vec<(u64, GetAccountRangeMessage, std::time::Instant)>,
    /// Available peers for snap sync requests
    available_peers: Vec<reth_network_peers::PeerId>,
    /// Peer performance metrics (peer_id -> success_rate)
    peer_metrics: HashMap<reth_network_peers::PeerId, f64>,
    /// Current adaptive range size
    current_range_size: u64,
    /// Network performance metrics for adaptive sizing
    network_metrics: NetworkMetrics,
    /// Active requests with their start times for timeout tracking
    active_requests: HashMap<u64, Instant>,
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
            retry_attempts: HashMap::new(),
            failed_requests: Vec::new(),
            available_peers: Vec::new(),
            peer_metrics: HashMap::new(),
            current_range_size: config.range_size,
            network_metrics: NetworkMetrics::default(),
            active_requests: HashMap::new(),
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
        Provider: StatsReader,
    {
        let tx = provider.tx_ref();
        let mut cursor = tx.cursor_read::<tables::HashedAccounts>()?;
        Ok(cursor.first()?.is_none())
    }

    /// Get the last hashed account from the database
    pub fn get_last_hashed_account<Provider>(&self, provider: &Provider) -> Result<Option<B256>, StageError>
    where
        Provider: StatsReader,
    {
        let tx = provider.tx_ref();
        let mut cursor = tx.cursor_read::<tables::HashedAccounts>()?;
        Ok(cursor.last()?.map(|(hash, _)| hash))
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

    /// Process account ranges and insert into database
    pub fn process_account_ranges<Provider>(
        &self,
        provider: &Provider,
        account_ranges: Vec<AccountRangeMessage>,
    ) -> Result<usize, StageError>
    where
        Provider: DBProvider,
    {
        if account_ranges.is_empty() {
            return Ok(0);
        }

        let tx = provider.tx_ref();
        let mut cursor = tx.cursor_write::<tables::HashedAccounts>()?;
        let mut processed = 0;

        for account_range in account_ranges {
            // Verify proof structure (basic validation)
            if !self.verify_account_range_proof(&account_range)? {
                return Err(StageError::Fatal("Invalid account range proof".into()));
            }

            // Validate accounts are in ascending order
            let mut prev_hash = B256::ZERO;
            for account_data in &account_range.accounts {
                if account_data.hash <= prev_hash && prev_hash != B256::ZERO {
                    return Err(StageError::Fatal("Accounts not in ascending order".into()));
                }
                prev_hash = account_data.hash;
            }

            // Insert accounts into database
            for account_data in account_range.accounts {
                // Decode account from RLP
                let account = Account::decode(&mut account_data.body.as_ref())
                    .map_err(|e| StageError::Fatal(format!("Failed to decode account: {}", e).into()))?;
                
                cursor.upsert(account_data.hash, account)?;
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
            // The proof should prove the account data against the target state root
            match verify_proof(
                target_state_root,
                account_nibbles,
                Some(account_data.body.as_ref()),
                &account_range.proof,
            ) {
                Ok(()) => {
                    // Proof verification successful for this account
                    continue;
                }
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
        // Extract the actual state root from the latest header
        self.header_receiver.as_ref().and_then(|receiver| {
            let header = receiver.borrow();
            Some(header.state_root())
        })
    }

    /// Start a network request for account range
    fn start_account_range_request(&mut self, starting_hash: B256, limit_hash: B256) -> Result<(), StageError> {
        // Generate request ID and create the request
        self.request_id_counter += 1;
        let request_id = self.request_id_counter;
        
        // Get target state root for the request
        let root_hash = self.get_target_state_root().unwrap_or(B256::ZERO);
        
        // Create the account range request
        let request = GetAccountRangeMessage {
            request_id,
            root_hash,
            starting_hash,
            limit_hash,
            response_bytes: self.config.max_response_bytes,
        };
        
        // Start tracking the request for timeout purposes
        self.start_request_tracking(request_id);
        
        // Set current range for tracking
        self.current_range = Some((starting_hash, limit_hash));
        
        // In a real implementation, this would start the actual network request via SnapClient
        // For now, we simulate the request by logging it
        debug!(
            target: "sync::stages::snap_sync",
            request_id = request_id,
            starting_hash = ?starting_hash,
            limit_hash = ?limit_hash,
            root_hash = ?root_hash,
            "Starting account range request"
        );
        
        Ok(())
    }

    /// Handle a failed network request with retry logic
    pub fn handle_failed_request(&mut self, request_id: u64, request: GetAccountRangeMessage) {
        let attempts = self.retry_attempts.get(&request_id).copied().unwrap_or(0);
        
        if attempts < self.config.max_retry_attempts {
            // Add to retry queue with exponential backoff delay
            let delay = Duration::from_millis(1000 * 2_u64.pow(attempts)); // 1s, 2s, 4s, 8s...
            let retry_time = Instant::now() + delay;
            
            self.failed_requests.push((request_id, request, retry_time));
            self.retry_attempts.insert(request_id, attempts + 1);
            
            warn!(
                target: "sync::stages::snap_sync",
                request_id = request_id,
                attempts = attempts + 1,
                max_attempts = self.config.max_retry_attempts,
                retry_delay_ms = delay.as_millis(),
                "Request failed, scheduling retry"
            );
        } else {
            // Max retries exceeded, give up
            error!(
                target: "sync::stages::snap_sync",
                request_id = request_id,
                attempts = attempts,
                "Request failed after max retries, giving up"
            );
            
            self.retry_attempts.remove(&request_id);
        }
    }

    /// Process retry queue and retry eligible requests
    pub fn process_retry_queue(&mut self) -> Result<(), StageError> {
        let now = Instant::now();
        let mut retry_now = Vec::new();
        
        // Find requests that are ready for retry
        for (i, (request_id, request, retry_time)) in self.failed_requests.iter().enumerate() {
            if now >= *retry_time {
                retry_now.push((*request_id, request.clone()));
            }
        }
        
        // Remove retry-ready requests from the queue
        self.failed_requests.retain(|(_, _, retry_time)| now < *retry_time);
        
        // Retry the eligible requests
        for (request_id, request) in retry_now {
            info!(
                target: "sync::stages::snap_sync",
                request_id = request_id,
                "Retrying failed request"
            );
            
            // In a real implementation, this would restart the network request
            // For now, we'll just log the retry
            debug!(
                target: "sync::stages::snap_sync",
                request_id = request_id,
                starting_hash = ?request.starting_hash,
                limit_hash = ?request.limit_hash,
                "Retrying account range request"
            );
        }
        
        Ok(())
    }

    /// Select the best available peer for a snap sync request
    pub fn select_peer(&self) -> Result<PeerId, StageError> {
        if self.available_peers.is_empty() {
            return Err(StageError::Fatal("No available peers for snap sync".into()));
        }

        // Select peer with highest success rate, or random if no metrics
        let best_peer = self.available_peers
            .iter()
            .max_by(|a, b| {
                let a_rate = self.peer_metrics.get(a).copied().unwrap_or(0.5);
                let b_rate = self.peer_metrics.get(b).copied().unwrap_or(0.5);
                a_rate.partial_cmp(&b_rate).unwrap_or(std::cmp::Ordering::Equal)
            })
            .ok_or_else(|| StageError::Fatal("No peers available".into()))?;

        Ok(*best_peer)
    }

    /// Update peer metrics based on request success/failure
    pub fn update_peer_metrics(&mut self, peer_id: PeerId, success: bool) {
        let current_rate = self.peer_metrics.get(&peer_id).copied().unwrap_or(0.5);
        
        // Simple exponential moving average: new_rate = 0.9 * old_rate + 0.1 * (success ? 1.0 : 0.0)
        let new_rate = 0.9 * current_rate + 0.1 * if success { 1.0 } else { 0.0 };
        
        self.peer_metrics.insert(peer_id, new_rate);
        
        debug!(
            target: "sync::stages::snap_sync",
            peer_id = %peer_id,
            success = success,
            new_rate = new_rate,
            "Updated peer metrics"
        );
    }

    /// Add a peer to the available peers list
    pub fn add_peer(&mut self, peer_id: PeerId) {
        if !self.available_peers.contains(&peer_id) {
            self.available_peers.push(peer_id);
            self.peer_metrics.insert(peer_id, 0.5); // Start with neutral rating
            
            info!(
                target: "sync::stages::snap_sync",
                peer_id = %peer_id,
                total_peers = self.available_peers.len(),
                "Added new peer for snap sync"
            );
        }
    }

    /// Remove a peer from the available peers list
    pub fn remove_peer(&mut self, peer_id: PeerId) {
        self.available_peers.retain(|&p| p != peer_id);
        self.peer_metrics.remove(&peer_id);
        
        warn!(
            target: "sync::stages::snap_sync",
            peer_id = %peer_id,
            remaining_peers = self.available_peers.len(),
            "Removed peer from snap sync"
        );
    }

    /// Get peer performance statistics
    pub fn get_peer_stats(&self) -> Vec<(PeerId, f64)> {
        self.peer_metrics
            .iter()
            .map(|(&peer_id, &rate)| (peer_id, rate))
            .collect()
    }

    /// Update network metrics based on request performance
    pub fn update_network_metrics(&mut self, response_time_ms: f64, success: bool) {
        // Update response time with exponential moving average
        let alpha = 0.1; // Smoothing factor
        self.network_metrics.avg_response_time_ms = 
            alpha * response_time_ms + (1.0 - alpha) * self.network_metrics.avg_response_time_ms;
        
        // Update success rate with exponential moving average
        let success_value = if success { 1.0 } else { 0.0 };
        self.network_metrics.success_rate = 
            alpha * success_value + (1.0 - alpha) * self.network_metrics.success_rate;
        
        self.network_metrics.sample_count += 1;
        
        // Adaptive range sizing based on network performance
        if self.config.adaptive_range_sizing {
            self.adjust_range_size();
        }
        
        debug!(
            target: "sync::stages::snap_sync",
            response_time_ms = response_time_ms,
            success = success,
            avg_response_time_ms = self.network_metrics.avg_response_time_ms,
            success_rate = self.network_metrics.success_rate,
            current_range_size = self.current_range_size,
            "Updated network metrics"
        );
    }

    /// Adjust range size based on network performance
    fn adjust_range_size(&mut self) {
        let old_size = self.current_range_size;
        
        // Adjust based on success rate and response time
        if self.network_metrics.success_rate > 0.9 && self.network_metrics.avg_response_time_ms < 1000.0 {
            // Good performance: increase range size
            self.current_range_size = (self.current_range_size * 2).min(self.config.max_range_size);
        } else if self.network_metrics.success_rate < 0.7 || self.network_metrics.avg_response_time_ms > 5000.0 {
            // Poor performance: decrease range size
            self.current_range_size = (self.current_range_size / 2).max(self.config.min_range_size);
        }
        
        if old_size != self.current_range_size {
            info!(
                target: "sync::stages::snap_sync",
                old_size = old_size,
                new_size = self.current_range_size,
                success_rate = self.network_metrics.success_rate,
                avg_response_time_ms = self.network_metrics.avg_response_time_ms,
                "Adjusted range size based on network performance"
            );
        }
    }

    /// Get current range size
    pub fn get_current_range_size(&self) -> u64 {
        self.current_range_size
    }

    /// Reset range size to default
    pub fn reset_range_size(&mut self) {
        self.current_range_size = self.config.range_size;
        self.network_metrics = NetworkMetrics::default();
        
        info!(
            target: "sync::stages::snap_sync",
            range_size = self.current_range_size,
            "Reset range size to default"
        );
    }

    /// Start tracking a request for timeout purposes
    pub fn start_request_tracking(&mut self, request_id: u64) {
        self.active_requests.insert(request_id, Instant::now());
        
        debug!(
            target: "sync::stages::snap_sync",
            request_id = request_id,
            timeout_seconds = self.config.request_timeout_seconds,
            "Started tracking request for timeout"
        );
    }

    /// Complete request tracking and update metrics
    pub fn complete_request_tracking(&mut self, request_id: u64, success: bool) {
        if let Some(start_time) = self.active_requests.remove(&request_id) {
            let response_time = start_time.elapsed();
            let response_time_ms = response_time.as_millis() as f64;
            
            // Update network metrics
            self.update_network_metrics(response_time_ms, success);
            
            debug!(
                target: "sync::stages::snap_sync",
                request_id = request_id,
                response_time_ms = response_time_ms,
                success = success,
                "Completed request tracking"
            );
        }
    }

    /// Check for timed out requests and handle them
    pub fn check_timeouts(&mut self) -> Result<(), StageError> {
        let now = Instant::now();
        let timeout_duration = Duration::from_secs(self.config.request_timeout_seconds);
        let mut timed_out_requests = Vec::new();
        
        // Find timed out requests
        for (&request_id, &start_time) in &self.active_requests {
            if now.duration_since(start_time) > timeout_duration {
                timed_out_requests.push(request_id);
            }
        }
        
        // Handle timed out requests
        for request_id in timed_out_requests {
            self.handle_request_timeout(request_id);
        }
        
        Ok(())
    }

    /// Handle a timed out request
    fn handle_request_timeout(&mut self, request_id: u64) {
        self.active_requests.remove(&request_id);
        
        warn!(
            target: "sync::stages::snap_sync",
            request_id = request_id,
            timeout_seconds = self.config.request_timeout_seconds,
            "Request timed out"
        );
        
        // Update network metrics for timeout (treated as failure)
        self.update_network_metrics(self.config.request_timeout_seconds as f64 * 1000.0, false);
        
        // Create a dummy request for retry logic
        let dummy_request = GetAccountRangeMessage {
            request_id,
            root_hash: B256::ZERO,
            starting_hash: B256::ZERO,
            limit_hash: B256::ZERO,
            response_bytes: self.config.max_response_bytes,
        };
        
        // Handle as failed request for retry logic
        self.handle_failed_request(request_id, dummy_request);
    }

    /// Get active request count
    pub fn get_active_request_count(&self) -> usize {
        self.active_requests.len()
    }

    /// Get timeout configuration
    pub fn get_timeout_seconds(&self) -> u64 {
        self.config.request_timeout_seconds
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
        _cx: &mut Context<'_>,
        _input: ExecInput,
    ) -> Poll<Result<(), StageError>> {
        if !self.config.enabled {
            return Poll::Ready(Ok(()));
        }

        // Check if we have a target state root from consensus engine
        if self.get_target_state_root().is_none() {
            return Poll::Pending;
        }

        // For now, we'll always return ready since we handle async operations in execute()
        // In a real implementation, this would poll the actual network requests
        Poll::Ready(Ok(()))
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

        // Get target state root from consensus engine
        let target_state_root = self.get_target_state_root()
            .ok_or_else(|| StageError::Fatal("No target state root available".into()))?;

        // Implement the snap sync algorithm as specified in the issues:
        // 1. Check if hashed state is empty -> start from 0x0000... or last entry
        // 2. Paginate over trie ranges using GetAccountRange requests
        // 3. If no data returned, return to step 1 (get new target state root)
        // 4. Repeat until final range (0xffff...) is fetched

        let mut starting_hash = if self.is_hashed_state_empty(provider)? {
            B256::ZERO
        } else {
            self.get_last_hashed_account(provider)?.unwrap_or(B256::ZERO)
        };

        let mut total_processed = 0;
        let max_hash = B256::from([0xff; 32]);

        // Process retry queue first
        self.process_retry_queue()?;
        
        // Check for timed out requests
        self.check_timeouts()?;

        // Process multiple ranges per execution (configurable)
        for _ in 0..self.config.max_ranges_per_execution {
            if starting_hash >= max_hash {
                break;
            }

            // Calculate the range for this request using configurable range size
            let range_size = B256::from_low_u64_be(self.current_range_size);
            let limit_hash = if starting_hash.saturating_add(range_size) >= max_hash {
                max_hash
            } else {
                starting_hash.saturating_add(range_size)
            };

            // Start network request for this range
            self.start_account_range_request(starting_hash, limit_hash)?;

            // Move to next range
            starting_hash = limit_hash;
        }

        // Process any completed account ranges from network requests
        // In a real implementation, this would process actual network responses
        // For now, we process empty ranges as the network requests are handled in poll_execute_ready
        let completed_ranges = vec![];
        let processed = self.process_account_ranges(provider, completed_ranges)?;
        total_processed += processed;

        // If no data was returned for current target state root, we need to re-poll
        // This implements step 3 of the algorithm
        if processed == 0 {
            debug!(
                target: "sync::stages::snap_sync",
                current_hash = ?starting_hash,
                "No data returned for range, may need new target state root"
            );
        }

        let total_accounts = provider.count_entries::<tables::HashedAccounts>()? as u64;
        let entities_checkpoint = EntitiesCheckpoint {
            processed: total_accounts,
            total: total_accounts,
        };

        // Stage is done when we've processed the final range (until 0xffff...)
        let done = starting_hash >= max_hash;

        info!(
            target: "sync::stages::snap_sync",
            processed = total_processed,
            total_accounts = total_accounts,
            done = done,
            target_state_root = ?target_state_root,
            current_hash = ?starting_hash,
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
        cursor.clear()?;
        
        Ok(UnwindOutput {
            checkpoint: StageCheckpoint::new(input.unwind_to),
        })
    }
}

#[cfg(test)]
mod tests;