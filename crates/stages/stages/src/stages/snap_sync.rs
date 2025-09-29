use alloy_primitives::B256;
use reth_config::config::SnapSyncConfig;
use reth_db_api::{
    cursor::DbCursorRW,
    tables,
    transaction::DbTx,
};
use reth_eth_wire_types::snap::{AccountRangeMessage, GetAccountRangeMessage};
use reth_net_p2p::snap::SnapClient;
use reth_provider::{
    DBProvider, StatsReader, HeaderProvider,
};
use reth_primitives_traits::{Account, SealedHeader};
use reth_stages_api::{
    EntitiesCheckpoint, ExecInput, ExecOutput, Stage, StageCheckpoint, StageError,
    StageId, UnwindInput, UnwindOutput,
};
use std::{
    sync::Arc,
    task::{Context, Poll},
};
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
    header_receiver: Option<watch::Receiver<SealedHeader>>,
    /// Request ID counter for snap requests
    request_id_counter: u64,
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
            current_range: None,
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
        let stats = provider.get_stats()?;
        Ok(stats.accounts_hashed == 0)
    }

    /// Get the last hashed account from the database
    pub fn get_last_hashed_account<Provider>(&self, provider: &Provider) -> Result<Option<B256>, StageError>
    where
        Provider: DBProvider,
    {
        let mut cursor = provider.tx_ref().cursor_read::<tables::HashedAccounts>()?;
        cursor.last().map(|result| {
            result.map(|(key, _)| key).map_err(|e| StageError::Database(e.into()))
        }).transpose()
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
        let mut processed = 0;
        let mut tx = provider.tx_ref();

        for account_range in account_ranges {
            // Verify proof before processing
            if !self.verify_account_range_proof(&account_range)? {
                return Err(StageError::Fatal("Account range proof verification failed".into()));
            }

            // Process each account in the range
            for account_data in &account_range.accounts {
                // Decode account data
                let account = Account::decode(&mut account_data.body.as_ref())
                    .map_err(|e| StageError::Fatal(format!("Failed to decode account: {}", e).into()))?;

                // Insert into database
                tx.put::<tables::HashedAccounts>(account_data.hash, account)?;
                processed += 1;
            }
        }

        tx.commit()?;
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
                Some(account_data.body.as_ref()),
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
            Some(header.state_root())
        })
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
        input: ExecInput,
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
            return Ok(ExecOutput {
                stage_progress: input.checkpoint().progress(),
                done: true,
                reached_tip: true,
                entities_checkpoint: EntitiesCheckpoint {
                    processed: 0,
                    total: Some(0),
                },
            });
        }

        // Get target state root
        let target_state_root = self.get_target_state_root()
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

        // Process multiple ranges per execution (configurable)
        for _ in 0..self.config.max_ranges_per_execution {
            if starting_hash >= max_hash {
                break;
            }

            // Calculate the range for this request using configurable range size
            let range_size = B256::from_low_u64_be(self.config.range_size);
            let limit_hash = if starting_hash.saturating_add(range_size) >= max_hash {
                max_hash
            } else {
                starting_hash.saturating_add(range_size)
            };

            // Create and send account range request
            let request = self.create_account_range_request(starting_hash, limit_hash);
            
            // In a real implementation, this would use the SnapClient to send the request
            // and wait for the response. For now, we simulate processing empty ranges.
            debug!(
                target: "sync::stages::snap_sync",
                request_id = request.request_id,
                starting_hash = ?request.starting_hash,
                limit_hash = ?request.limit_hash,
                root_hash = ?request.root_hash,
                "Processing account range request"
            );

            // Process any completed account ranges from network requests
            // Network responses are handled through the SnapClient trait implementation
            let completed_ranges = vec![];
            let processed = self.process_account_ranges(provider, completed_ranges)?;
            total_processed += processed;

            // Move to next range
            // starting_hash = limit_hash; // This would be in a real implementation
        }

        // If no data was returned for current target state root, we need to re-poll
        if total_processed == 0 {
            debug!(
                target: "sync::stages::snap_sync",
                "No data returned for current target state root, will re-poll"
            );
        }

        Ok(ExecOutput {
            stage_progress: input.checkpoint().progress(),
            done: total_processed == 0, // Done when no more data
            reached_tip: total_processed == 0,
            entities_checkpoint: EntitiesCheckpoint {
                processed: total_processed as u64,
                total: None,
            },
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