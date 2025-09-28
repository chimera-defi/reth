use alloy_primitives::{keccak256, B256, U256};
use reth_db_api::{
    cursor::DbCursorRW,
    tables,
    transaction::{DbTx, DbTxMut},
};
use reth_eth_wire_types::snap::{AccountRangeMessage, AccountData};
use reth_provider::{
    DBProvider, StatsReader, HashingWriter,
};
use reth_stages_api::{
    EntitiesCheckpoint, ExecInput, ExecOutput, Stage, StageCheckpoint, StageError,
    StageId, UnwindInput, UnwindOutput,
};
use std::{
    collections::BTreeMap,
    task::{ready, Context, Poll},
};
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
pub struct SnapSyncStage {
    /// Configuration for the stage
    config: SnapSyncConfig,
    /// Current target state root
    target_state_root: Option<B256>,
    /// Current starting hash for account range requests
    current_starting_hash: B256,
    /// Request ID counter for snap requests
    request_id_counter: u64,
}

impl SnapSyncStage {
    /// Create a new SnapSyncStage
    pub fn new(config: SnapSyncConfig) -> Self {
        Self {
            config,
            target_state_root: None,
            current_starting_hash: B256::ZERO,
            request_id_counter: 0,
        }
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
        let mut accounts = BTreeMap::new();
        
        for account_data in account_range.accounts {
            // Decode account from RLP
            let account = reth_primitives_traits::Account::decode(&mut account_data.body.as_ref())
                .map_err(|e| StageError::Fatal(format!("Failed to decode account: {}", e).into()))?;
            
            accounts.insert(account_data.hash, Some(account));
        }

        // Write accounts to hashed state
        provider.write_hashed_accounts(accounts)?;

        // TODO: Verify proofs
        Ok(())
    }

    /// Simulate downloading account ranges (placeholder for real implementation)
    fn download_account_ranges<Provider>(
        &mut self,
        provider: &Provider,
    ) -> Result<usize, StageError>
    where
        Provider: DBProvider + StatsReader + HashingWriter,
    {
        let mut ranges_processed = 0;
        let mut current_hash = self.current_starting_hash;
        let max_hash = B256::from([0xff; 32]);

        while current_hash < max_hash && ranges_processed < self.config.max_ranges_per_execution {
            // Simulate account range response
            let mut accounts = Vec::new();
            
            // Generate some mock account data
            for i in 0..10 {
                let mut hash_bytes = [0u8; 32];
                hash_bytes[31] = (current_hash.as_slice()[31] + i as u8) % 256;
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
                request_id: self.request_id_counter,
                accounts,
                proof: vec![],
            };

            self.request_id_counter += 1;

            // Process the account range
            self.process_account_range(provider, account_range)?;
            
            // Update current hash
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
            current_hash = next_hash;
            ranges_processed += 1;
        }

        self.current_starting_hash = current_hash;
        Ok(ranges_processed)
    }
}

impl<Provider> Stage<Provider> for SnapSyncStage
where
    Provider: DBProvider + StatsReader + HashingWriter,
{
    fn id(&self) -> StageId {
        StageId::SnapSync
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

        // Download account ranges
        let ranges_processed = self.download_account_ranges(provider)?;

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

    #[test]
    fn test_snap_sync_stage_creation() {
        let config = SnapSyncConfig::default();
        let stage = SnapSyncStage::new(config);
        assert_eq!(stage.config.enabled, false);
        assert_eq!(stage.current_starting_hash, B256::ZERO);
    }

    #[test]
    fn test_snap_sync_stage_disabled() {
        let mut config = SnapSyncConfig::default();
        config.enabled = false;
        let mut stage = SnapSyncStage::new(config);
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
        let mut stage = SnapSyncStage::new(config);
        
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
        let stage = SnapSyncStage::new(config);
        let db = TestStageDB::default();
        let provider = db.factory.database_provider_rw().unwrap();
        
        let is_empty = stage.is_hashed_state_empty(&provider).unwrap();
        assert!(is_empty);
    }
}