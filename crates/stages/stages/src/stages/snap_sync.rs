//! Snap sync stage implementation.

use alloy_primitives::{B256, Bytes};
use futures_util::StreamExt;
use reth_config::config::{EtlConfig, SnapSyncConfig};
use reth_db_api::{
    cursor::{DbCursorRO, DbCursorRW},
    table::Value,
    tables,
    transaction::{DbTx, DbTxMut},
    DbTxUnwindExt, RawKey, RawTable, RawValue,
};
use reth_etl::Collector;
use reth_network_downloaders::snap::downloader::SnapSyncDownloader;
use reth_network_p2p::snap::client::SnapClient;
use reth_primitives_traits::{serde_bincode_compat, NodePrimitives};
use reth_provider::{
    providers::StaticFileWriter, BlockHashReader, DBProvider, HeaderProvider,
    StaticFileProviderFactory,
};
use reth_stages_api::{
    CheckpointBlockRange, EntitiesCheckpoint, ExecInput, ExecOutput, HeadersCheckpoint, Stage,
    StageCheckpoint, StageError, StageId, UnwindInput, UnwindOutput,
};
use reth_static_file_types::StaticFileSegment;
use reth_storage_errors::provider::ProviderError;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::*;

/// The snap sync stage.
///
/// The snap sync stage downloads Ethereum state snapshots using the snap protocol.
/// It downloads account ranges, storage ranges, byte codes, and trie nodes to
/// quickly synchronize the state without processing every block.
#[derive(Debug)]
pub struct SnapSyncStage<Provider, Client: SnapClient> {
    /// Database handle.
    provider: Provider,
    /// Snap sync downloader
    downloader: SnapSyncDownloader<Client, Provider>,
    /// Configuration for snap sync
    config: SnapSyncConfig,
    /// ETL collector for account data
    account_collector: Collector<B256, Bytes>,
    /// ETL collector for storage data
    storage_collector: Collector<B256, Bytes>,
    /// ETL collector for byte codes
    byte_code_collector: Collector<B256, Bytes>,
    /// ETL collector for trie nodes
    trie_node_collector: Collector<B256, Bytes>,
    /// Current state root being synced
    current_state_root: Option<B256>,
    /// Whether the stage is ready to write data
    is_ready_to_write: bool,
}

impl<Provider, Client> SnapSyncStage<Provider, Client>
where
    Client: SnapClient,
    Provider: HeaderProvider + Clone,
{
    /// Create a new snap sync stage
    pub fn new(
        provider: Provider,
        client: Arc<Client>,
        config: SnapSyncConfig,
        etl_config: EtlConfig,
    ) -> Self {
        let downloader = SnapSyncDownloader::new(client, provider.clone(), config.clone());
        
        Self {
            provider,
            downloader,
            config,
            account_collector: Collector::new(etl_config.file_size / 4, etl_config.dir.clone()),
            storage_collector: Collector::new(etl_config.file_size / 4, etl_config.dir.clone()),
            byte_code_collector: Collector::new(etl_config.file_size / 4, etl_config.dir.clone()),
            trie_node_collector: Collector::new(etl_config.file_size / 4, etl_config.dir),
            current_state_root: None,
            is_ready_to_write: false,
        }
    }

    /// Start the snap sync process for the given state root
    async fn start_snap_sync(&mut self, state_root: B256) -> Result<(), StageError> {
        info!(target: "sync::stages::snap_sync", state_root = ?state_root, "Starting snap sync");
        
        self.current_state_root = Some(state_root);
        self.downloader.start_account_range_download(state_root).await
            .map_err(|e| StageError::Fatal(Box::new(e)))?;
        
        Ok(())
    }

    /// Process downloaded snap sync data
    async fn process_snap_data(&mut self) -> Result<usize, StageError> {
        let mut processed_count = 0;
        
        // Process the downloader stream
        let mut stream = Box::pin(&mut self.downloader);
        while let Some(result) = stream.next().await {
            match result {
                Ok(snap_result) => {
                    match snap_result {
                        reth_network_downloaders::snap::downloader::SnapSyncResult::AccountRange(msg) => {
                            // Process account range data
                            for account in msg.accounts {
                                self.account_collector.insert(account.hash, account.body)?;
                                processed_count += 1;
                            }
                            
                            // Store proof data
                            for proof in msg.proof {
                                self.trie_node_collector.insert(B256::random(), proof)?;
                            }
                        }
                        reth_network_downloaders::snap::downloader::SnapSyncResult::StorageRanges(msg) => {
                            // Process storage range data
                            for account_slots in msg.slots {
                                for slot in account_slots {
                                    self.storage_collector.insert(slot.hash, slot.data)?;
                                    processed_count += 1;
                                }
                            }
                            
                            // Store proof data
                            for proof in msg.proof {
                                self.trie_node_collector.insert(B256::random(), proof)?;
                            }
                        }
                        reth_network_downloaders::snap::downloader::SnapSyncResult::ByteCodes(msg) => {
                            // Process byte code data
                            for (i, code) in msg.codes.into_iter().enumerate() {
                                let hash = B256::from_slice(&[i as u8; 32]); // Simplified hash
                                self.byte_code_collector.insert(hash, code)?;
                                processed_count += 1;
                            }
                        }
                        reth_network_downloaders::snap::downloader::SnapSyncResult::TrieNodes(msg) => {
                            // Process trie node data
                            for (i, node) in msg.nodes.into_iter().enumerate() {
                                let hash = B256::from_slice(&[i as u8; 32]); // Simplified hash
                                self.trie_node_collector.insert(hash, node)?;
                                processed_count += 1;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(target: "sync::stages::snap_sync", error = ?e, "Error processing snap sync data");
                    return Err(StageError::Fatal(Box::new(e)));
                }
            }
        }
        
        Ok(processed_count)
    }

    /// Write snap sync data to storage
    fn write_snap_data<P>(&mut self, provider: &P) -> Result<usize, StageError>
    where
        P: DBProvider<Tx: DbTxMut> + StaticFileProviderFactory,
    {
        let mut total_written = 0;
        
        // Write account data
        let account_count = self.account_collector.len();
        if account_count > 0 {
            info!(target: "sync::stages::snap_sync", count = account_count, "Writing account data");
            
            let mut static_file_writer = provider
                .static_file_provider()
                .get_writer(StaticFileSegment::Headers, 0)?;
            
            for (hash, data) in self.account_collector.iter() {
                static_file_writer.append_block(hash, data)?;
                total_written += 1;
            }
            
            static_file_writer.commit()?;
            self.account_collector.clear();
        }
        
        // Write storage data
        let storage_count = self.storage_collector.len();
        if storage_count > 0 {
            info!(target: "sync::stages::snap_sync", count = storage_count, "Writing storage data");
            
            let mut static_file_writer = provider
                .static_file_provider()
                .get_writer(StaticFileSegment::Headers, 1)?;
            
            for (hash, data) in self.storage_collector.iter() {
                static_file_writer.append_block(hash, data)?;
                total_written += 1;
            }
            
            static_file_writer.commit()?;
            self.storage_collector.clear();
        }
        
        // Write byte code data
        let byte_code_count = self.byte_code_collector.len();
        if byte_code_count > 0 {
            info!(target: "sync::stages::snap_sync", count = byte_code_count, "Writing byte code data");
            
            let mut static_file_writer = provider
                .static_file_provider()
                .get_writer(StaticFileSegment::Headers, 2)?;
            
            for (hash, data) in self.byte_code_collector.iter() {
                static_file_writer.append_block(hash, data)?;
                total_written += 1;
            }
            
            static_file_writer.commit()?;
            self.byte_code_collector.clear();
        }
        
        // Write trie node data
        let trie_node_count = self.trie_node_collector.len();
        if trie_node_count > 0 {
            info!(target: "sync::stages::snap_sync", count = trie_node_count, "Writing trie node data");
            
            let mut static_file_writer = provider
                .static_file_provider()
                .get_writer(StaticFileSegment::Headers, 3)?;
            
            for (hash, data) in self.trie_node_collector.iter() {
                static_file_writer.append_block(hash, data)?;
                total_written += 1;
            }
            
            static_file_writer.commit()?;
            self.trie_node_collector.clear();
        }
        
        Ok(total_written)
    }
}

impl<Provider, Client> Stage<Provider> for SnapSyncStage<Provider, Client>
where
    Client: SnapClient + 'static,
    Provider: HeaderProvider + Clone + 'static,
{
    fn id(&self) -> StageId {
        StageId::SnapSync
    }

    fn execute(
        &mut self,
        provider: &Provider,
        input: ExecInput,
    ) -> Result<ExecOutput, StageError> {
        let stage_checkpoint = input.checkpoint();
        let target = input.target();
        
        info!(target: "sync::stages::snap_sync", 
            checkpoint = ?stage_checkpoint,
            target = ?target,
            "Executing snap sync stage"
        );

        // Get the current state root from the latest header
        let state_root = self.downloader.get_current_state_root()
            .map_err(|e| StageError::Fatal(Box::new(e)))?;
        
        // Start snap sync if we haven't already
        if self.current_state_root.is_none() {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(self.start_snap_sync(state_root))?;
        }
        
        // Process downloaded data
        let rt = tokio::runtime::Handle::current();
        let processed_count = rt.block_on(self.process_snap_data())?;
        
        // Write data to storage if we have enough
        let written_count = if processed_count > 0 {
            self.write_snap_data(provider)?
        } else {
            0
        };
        
        info!(target: "sync::stages::snap_sync", 
            processed = processed_count,
            written = written_count,
            "Completed snap sync stage execution"
        );

        Ok(ExecOutput {
            checkpoint: StageCheckpoint::new(target)
                .with_entities_stage_checkpoint(EntitiesCheckpoint {
                    processed: processed_count as u64,
                    total: Some(processed_count as u64),
                }),
            done: written_count > 0,
        })
    }

    fn unwind(
        &mut self,
        provider: &Provider,
        input: UnwindInput,
    ) -> Result<UnwindOutput, StageError> {
        info!(target: "sync::stages::snap_sync", 
            checkpoint = ?input.checkpoint(),
            unwind_to = ?input.unwind_to(),
            "Unwinding snap sync stage"
        );

        // Clear all collectors
        self.account_collector.clear();
        self.storage_collector.clear();
        self.byte_code_collector.clear();
        self.trie_node_collector.clear();
        
        // Reset state
        self.current_state_root = None;
        self.is_ready_to_write = false;

        Ok(UnwindOutput {
            checkpoint: StageCheckpoint::new(input.unwind_to()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reth_network_downloaders::snap::test_utils::TestSnapClient;
    use reth_provider::test_utils::MockProvider;

    #[test]
    fn test_snap_sync_stage_creation() {
        let provider = MockProvider::default();
        let client = Arc::new(TestSnapClient::new());
        let config = SnapSyncConfig::default();
        let etl_config = EtlConfig::default();
        
        let stage = SnapSyncStage::new(provider, client, config, etl_config);
        assert_eq!(stage.id(), StageId::SnapSync);
    }
}

#[cfg(test)]
#[path = "snap_sync_tests.rs"]
mod integration_tests;

#[cfg(test)]
#[path = "snap_sync_e2e_test.rs"]
mod e2e_tests;

// Re-export integration tests when testing
#[cfg(test)]
pub use integration_tests::*;