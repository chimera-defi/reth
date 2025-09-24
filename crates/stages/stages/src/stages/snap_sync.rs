use futures_util::TryStreamExt;
use reth_db_api::{
    cursor::DbCursorRO,
    tables,
    transaction::{DbTx, DbTxMut},
};
use reth_network::downloaders::snap::{SnapSyncClient, SnapSyncDownloader};
use reth_provider::{
    providers::StaticFileWriter, DBProvider, ProviderError, StaticFileProviderFactory,
};
use reth_stages_api::{
    EntitiesCheckpoint, ExecInput, ExecOutput, Stage, StageCheckpoint, StageError, StageId,
    UnwindInput, UnwindOutput,
};
use reth_storage_errors::provider::ProviderResult;
use std::{task::ready, task::Context, task::Poll};
use tracing::*;

/// The snap sync stage downloads state data using the snap protocol.
///
/// This stage downloads the complete Ethereum state for a given block using
/// the SNAP (State Network Access Protocol) which allows for faster state
/// synchronization by downloading state snapshots instead of computing the
/// state from block execution.
///
/// # Tables
///
/// The state data is processed and inserted into these tables:
///
/// - [`PlainAccountState`][reth_db_api::tables::PlainAccountState]
/// - [`PlainStorageState`][reth_db_api::tables::PlainStorageState]
/// - [`Bytecodes`][reth_db_api::tables::Bytecodes]
///
/// # Genesis
///
/// This stage expects that the genesis has been inserted into the appropriate tables.
#[derive(Debug)]
pub struct SnapSyncStage<D: SnapSyncClient> {
    /// The snap sync downloader.
    downloader: SnapSyncDownloader<D>,
}

impl<D: SnapSyncClient> SnapSyncStage<D> {
    /// Create new snap sync stage from downloader.
    pub fn new(downloader: SnapSyncDownloader<D>) -> Self {
        Self { downloader }
    }
}

#[derive(Debug)]
pub struct SnapSyncStageInput {
    /// The block hash to sync state for
    pub block_hash: alloy_primitives::B256,
    /// The expected state root
    pub state_root: alloy_primitives::B256,
}

impl<D: SnapSyncClient> Stage<SnapSyncStageInput> for SnapSyncStage<D> {
    /// The ID of the stage
    fn id(&self) -> StageId {
        StageId::new("SnapSync")
    }

    /// Execute the stage
    async fn execute(
        &mut self,
        input: ExecInput<SnapSyncStageInput>,
    ) -> Result<ExecOutput, StageError> {
        let (input, mut cursor) = input.into_parts();

        let block_hash = input.block_hash;
        let state_root = input.state_root;

        info!(
            target: "reth::stages::snap_sync",
            block_hash = %block_hash,
            state_root = %state_root,
            "Starting snap sync stage"
        );

        // Download the complete state using snap sync
        let stats = self
            .downloader
            .download_state(block_hash)
            .await
            .map_err(|e| StageError::Fatal(e.to_string()))?;

        info!(
            target: "reth::stages::snap_sync",
            accounts = stats.total_accounts,
            storage_slots = stats.total_storage_slots,
            bytecode = stats.total_bytecode,
            "Snap sync stage completed"
        );

        // Update the checkpoint to indicate completion
        let checkpoint = StageCheckpoint::new(state_root)
            .with_entities_checkpoint(EntitiesCheckpoint {
                processed: stats.total_accounts as u64,
                total: stats.total_accounts as u64,
            });

        Ok(ExecOutput {
            checkpoint,
            done: true,
        })
    }

    /// Unwind the stage
    async fn unwind(
        &mut self,
        input: UnwindInput,
    ) -> Result<UnwindOutput, StageError> {
        let (input, mut cursor) = input.into_parts();

        info!(
            target: "reth::stages::snap_sync",
            unwind_to = input.unwind_to,
            "Unwinding snap sync stage"
        );

        // TODO: Implement unwinding logic
        // For now, we'll just mark as completed
        let checkpoint = StageCheckpoint::new(input.unwind_to);

        Ok(UnwindOutput {
            checkpoint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::B256;

    // TODO: Add tests for snap sync stage
}