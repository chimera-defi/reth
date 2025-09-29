use alloy_primitives::B256;
use reth_config::config::SnapSyncConfig;
use reth_db_api::{
    cursor::DbCursorRW,
    tables,
    transaction::{DbTx, DbTxMut},
};
use reth_eth_wire_types::snap::{AccountRangeMessage, AccountData, GetAccountRangeMessage};
use reth_net_p2p::{
    snap::SnapClient,
    priority::Priority,
};
use reth_provider::{
    DBProvider, StatsReader, HeaderProvider,
};
use reth_stages_api::{
    EntitiesCheckpoint, ExecInput, ExecOutput, Stage, StageCheckpoint, StageError,
    StageId, UnwindInput, UnwindOutput,
};
use std::{
    sync::Arc,
    task::{Context, Poll},
};
use tracing::*;

/// Snap sync stage for downloading trie data ranges from peers.
/// Replaces SenderRecoveryStage, ExecutionStage and PruneSenderRecoveryStage when enabled.
#[derive(Debug)]
pub struct SnapSyncStage<C> {
    /// Configuration for the stage
    config: SnapSyncConfig,
    /// Snap client for peer communication
    snap_client: Arc<C>,
}

impl<C> SnapSyncStage<C>
where
    C: SnapClient + Send + Sync + 'static,
{
    /// Create a new SnapSyncStage
    pub const fn new(config: SnapSyncConfig, snap_client: Arc<C>) -> Self {
        Self {
            config,
            snap_client,
        }
    }

    /// Check if hashed state is empty
    fn is_hashed_state_empty<Provider>(&self, provider: &Provider) -> Result<bool, StageError>
    where
        Provider: StatsReader,
    {
        let tx = provider.tx_ref();
        let mut cursor = tx.cursor_read::<tables::HashedAccounts>()?;
        Ok(cursor.first()?.is_none())
    }

    /// Get the last hashed account from the database
    fn get_last_hashed_account<Provider>(&self, provider: &Provider) -> Result<Option<B256>, StageError>
    where
        Provider: StatsReader,
    {
        let tx = provider.tx_ref();
        let mut cursor = tx.cursor_read::<tables::HashedAccounts>()?;
        Ok(cursor.last()?.map(|(hash, _)| hash))
    }

    /// Process account ranges and insert into database
    fn process_account_ranges<Provider>(
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
            // Basic validation - accounts should be in ascending order
            let mut prev_hash = B256::ZERO;
            for account_data in &account_range.accounts {
                if account_data.hash <= prev_hash {
                    return Err(StageError::Fatal("Accounts not in ascending order".into()));
                }
                prev_hash = account_data.hash;
            }

            // Insert accounts into database
            for account_data in account_range.accounts {
                // Decode account from RLP
                let account = reth_primitives_traits::Account::decode(&mut account_data.body.as_ref())
                    .map_err(|e| StageError::Fatal(format!("Failed to decode account: {}", e).into()))?;
                
                cursor.upsert(account_data.hash, account)?;
                processed += 1;
            }
        }

        Ok(processed)
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

        // For now, always ready (real implementation would handle async here)
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

        // Check if we need to start from beginning or continue
        let starting_hash = if self.is_hashed_state_empty(provider)? {
            B256::ZERO
        } else {
            self.get_last_hashed_account(provider)?.unwrap_or(B256::ZERO)
        };

        // Create account range request
        let request = GetAccountRangeMessage {
            request_id: 1, // Simple counter
            starting_hash,
            limit_hash: B256::from([0xff; 32]), // Max hash
            response_bytes: self.config.max_response_bytes,
        };

        // TODO: In real implementation, this would be async
        // For now, we simulate with empty response
        let account_ranges = vec![AccountRangeMessage {
            request_id: 1,
            accounts: vec![],
            proof: vec![],
        }];

        let processed = self.process_account_ranges(provider, account_ranges)?;
        let total_accounts = provider.count_entries::<tables::HashedAccounts>()? as u64;

        let entities_checkpoint = EntitiesCheckpoint {
            processed: total_accounts,
            total: total_accounts,
        };

        // For now, mark as done when we've processed something
        let done = processed > 0 || starting_hash >= B256::from([0xff; 32]);

        info!(
            target: "sync::stages::snap_sync",
            processed = processed,
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
        cursor.clear()?;
        
        Ok(UnwindOutput {
            checkpoint: StageCheckpoint::new(input.unwind_to),
        })
    }
}

#[cfg(test)]
mod tests;