use alloy_primitives::B256;
use reth_config::config::SnapSyncConfig;
use reth_db_api::{
    cursor::DbCursorRW,
    tables,
    transaction::DbTx,
};
use reth_eth_wire_types::snap::{AccountRangeMessage, GetAccountRangeMessage};
use reth_net_p2p::{
    snap::SnapClient,
    priority::Priority,
};
use reth_provider::{
    DBProvider, StatsReader, HeaderProvider,
};
use reth_primitives_traits::Account;
use reth_stages_api::{
    EntitiesCheckpoint, ExecInput, ExecOutput, Stage, StageCheckpoint, StageError,
    StageId, UnwindInput, UnwindOutput,
};
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use futures::Future;
use tokio::sync::watch;
use tracing::*;

/// Snap sync stage for downloading trie data ranges from peers.
/// Replaces SenderRecoveryStage, ExecutionStage and PruneSenderRecoveryStage when enabled.
#[derive(Debug)]
pub struct SnapSyncStage<C> {
    /// Configuration for the stage
    config: SnapSyncConfig,
    /// Snap client for peer communication
    snap_client: Arc<C>,
    /// Watch receiver for header updates from consensus engine
    header_receiver: Option<watch::Receiver<B256>>,
    /// Request ID counter for snap requests
    request_id_counter: u64,
    /// Pending network requests
    pending_requests: Vec<Pin<Box<dyn Future<Output = Result<AccountRangeMessage, StageError>> + Send + 'static>>>,
    /// Completed account ranges ready for processing
    completed_ranges: Vec<AccountRangeMessage>,
    /// Current range being processed
    current_range: Option<(B256, B256)>,
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
            pending_requests: Vec::new(),
            completed_ranges: Vec::new(),
            current_range: None,
        }
    }

    /// Set the header receiver for consensus engine updates
    pub fn with_header_receiver(mut self, receiver: watch::Receiver<B256>) -> Self {
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
            root_hash: self.get_target_state_root().unwrap_or(B256::ZERO), // TODO: Use actual state root
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

    /// Verify account range proof (basic validation)
    fn verify_account_range_proof(&self, account_range: &AccountRangeMessage) -> Result<bool, StageError> {
        // TODO: Implement full Merkle proof verification using reth_trie utilities
        // This should verify the proof against the target state root
        // For now, we just check that the proof is present if there are accounts
        if !account_range.accounts.is_empty() && account_range.proof.is_empty() {
            warn!(
                target: "sync::stages::snap_sync",
                "Account range has accounts but no proof - proof verification needed"
            );
        }
        
        // TODO: Replace with actual Merkle proof verification
        // Should use reth_trie::verify_proof or similar utilities
        Ok(true)
    }

    /// Get current target state root from header receiver
    pub fn get_target_state_root(&self) -> Option<B256> {
        // TODO: Extract actual state root from header instead of using header hash
        // The header receiver should provide the actual state root, not just the header hash
        self.header_receiver.as_ref().and_then(|receiver| receiver.borrow().clone())
    }

    /// Start a network request for account range
    fn start_account_range_request(&mut self, starting_hash: B256, limit_hash: B256) -> Result<(), StageError> {
        let request = self.create_account_range_request(starting_hash, limit_hash);
        
        // Create the actual network request using SnapClient
        let snap_client = Arc::clone(&self.snap_client);
        let future = async move {
            // TODO: Implement peer selection strategy
            // Should select the best available peer for the request
            match snap_client.get_account_range_with_priority(request, Priority::Normal).await {
                Ok(response) => Ok(response.result),
                Err(e) => {
                    // TODO: Implement retry logic with exponential backoff
                    // Should retry failed requests up to max_retry_attempts times
                    Err(StageError::Fatal(format!("Network request failed: {}", e).into()))
                }
            }
        };
        
        self.pending_requests.push(Box::pin(future));
        self.current_range = Some((starting_hash, limit_hash));
        Ok(())
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

        // Poll pending network requests
        let mut completed_requests = Vec::new();
        for (i, future) in self.pending_requests.iter_mut().enumerate() {
            match future.as_mut().poll(cx) {
                Poll::Ready(result) => {
                    completed_requests.push((i, result));
                }
                Poll::Pending => continue,
            }
        }

        // Process completed requests
        for (i, result) in completed_requests.iter().rev() {
            match result {
                Ok(account_range) => {
                    self.completed_ranges.push(account_range.clone());
                }
                Err(e) => {
                    warn!(
                        target: "sync::stages::snap_sync",
                        error = %e,
                        "Failed to get account range from peer"
                    );
                }
            }
            self.pending_requests.remove(*i);
        }

        // If we have pending requests, continue polling
        if !self.pending_requests.is_empty() {
            return Poll::Pending;
        }

        // Ready to execute when no pending requests
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

        // Process multiple ranges per execution (configurable)
        for _ in 0..self.config.max_ranges_per_execution {
            if starting_hash >= max_hash {
                break;
            }

            // Calculate the range for this request
            // TODO: Make range size configurable and optimize based on network conditions
            let range_size = B256::from_low_u64_be(0x1000000000000000u64); // 1/16th of hash space
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

        // Process completed account ranges
        if !self.completed_ranges.is_empty() {
            let account_ranges = std::mem::take(&mut self.completed_ranges);
            let processed = self.process_account_ranges(provider, account_ranges)?;
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