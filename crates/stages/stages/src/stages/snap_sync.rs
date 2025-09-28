use alloy_primitives::{B256, U256};
use futures::{Stream, StreamExt};
use reth_db_api::{
    cursor::DbCursorRW,
    table::Value,
    tables,
    transaction::{DbTx, DbTxMut},
};
use reth_eth_wire_types::snap::{GetAccountRangeMessage, AccountRangeMessage, AccountData};
use reth_net_p2p::{
    download::DownloadClient,
    error::PeerRequestResult,
    snap::SnapClient,
};
use reth_provider::{
    BlockReader, DBProvider, HeaderProvider, ProviderError, StatsReader,
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
use tokio::sync::mpsc;
use tracing::*;

/// Configuration for the SnapSyncStage
#[derive(Debug, Clone)]
pub struct SnapSyncConfig {
    /// Max account ranges per execution
    pub max_ranges_per_execution: usize,
    /// Max response bytes per request
    pub max_response_bytes: u64,
    /// Enable snap sync
    pub enabled: bool,
}

impl Default for SnapSyncConfig {
    fn default() -> Self {
        Self {
            max_ranges_per_execution: 100,
            max_response_bytes: 2 * 1024 * 1024, // 2MB
            enabled: false,
        }
    }
}

/// Snap sync stage for querying peers for trie data ranges.
/// Replaces SenderRecoveryStage, ExecutionStage and PruneSenderRecoveryStage when enabled.
#[derive(Debug)]
pub struct SnapSyncStage<C, S> {
    /// Configuration for the stage
    config: SnapSyncConfig,
    /// Snap client for communicating with peers
    snap_client: Arc<C>,
    /// Stream of head headers from the consensus engine
    header_stream: S,
    /// Current target state root
    target_state_root: Option<B256>,
    /// Current starting hash for account range requests
    current_starting_hash: B256,
    /// Request ID counter for snap requests
    request_id_counter: u64,
    /// Channel for receiving header updates
    header_receiver: Option<mpsc::UnboundedReceiver<B256>>,
}

impl<C, S> SnapSyncStage<C, S>
where
    C: SnapClient + Send + Sync + 'static,
    S: Stream<Item = B256> + Send + Unpin + 'static,
{
    /// Create a new SnapSyncStage
    pub fn new(config: SnapSyncConfig, snap_client: Arc<C>, header_stream: S) -> Self {
        Self {
            config,
            snap_client,
            header_stream,
            target_state_root: None,
            current_starting_hash: B256::ZERO,
            request_id_counter: 0,
            header_receiver: None,
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

    /// Create a GetAccountRange request
    fn create_account_range_request(&mut self, starting_hash: B256, limit_hash: B256) -> GetAccountRangeMessage {
        self.request_id_counter += 1;
        GetAccountRangeMessage {
            request_id: self.request_id_counter,
            root_hash: self.target_state_root.unwrap_or(B256::ZERO),
            starting_hash,
            limit_hash,
            response_bytes: self.config.max_response_bytes,
        }
    }

    /// Process account range and insert into database
    fn process_account_range<Provider>(
        &self,
        provider: &Provider,
        account_range: AccountRangeMessage,
    ) -> Result<(), StageError>
    where
        Provider: DBProvider,
    {
        let tx = provider.tx_ref();
        let mut cursor = tx.cursor_write::<tables::HashedAccounts>()?;

        for account_data in account_range.accounts {
            // Placeholder: proper account deserialization needed
            let account = reth_primitives_traits::Account {
                nonce: 0,
                balance: U256::ZERO,
                bytecode_hash: None,
            };

            cursor.upsert(account_data.hash, &account)?;
        }

        // Placeholder: proof verification needed
        Ok(())
    }

    /// Download account ranges using GetAccountRange requests
    async fn download_account_ranges<Provider>(
        &mut self,
        provider: &Provider,
    ) -> Result<usize, StageError>
    where
        Provider: DBProvider + StatsReader,
    {
        let mut ranges_processed = 0;
        let mut current_hash = self.current_starting_hash;
        let max_hash = B256::from([0xff; 32]); // 0xffff...

        while current_hash < max_hash && ranges_processed < self.config.max_ranges_per_execution {
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
            
            // Send the request to peers
            let response = self.snap_client.get_account_range(request).await
                .map_err(|e| StageError::Fatal(format!("Snap client error: {:?}", e).into()))?;

            match response {
                Ok(peer_response) => {
                    let account_range = peer_response.result;
                    if account_range.accounts.is_empty() {
                        // No data returned, we might have reached the end or need to update target
                        break;
                    }

                    // Process the account range
                    self.process_account_range(provider, account_range)?;
                    
                    // Update current hash to continue from where we left off
                    if let Some(last_account) = account_range.accounts.last() {
                        current_hash = last_account.hash;
                    } else {
                        break;
                    }
                    
                    ranges_processed += 1;
                }
                Err(e) => {
                    warn!(target: "sync::stages::snap_sync", error = ?e, "Failed to get account range");
                    // Continue with next range
                    current_hash = limit_hash;
                }
            }
        }

        self.current_starting_hash = current_hash;
        Ok(ranges_processed)
    }

    /// Update target state root from header stream
    fn poll_header_updates(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if let Some(ref mut receiver) = self.header_receiver {
            while let Poll::Ready(Some(header_hash)) = receiver.poll_recv(cx) {
                // TODO: Get state root from header
                // For now, we'll use the header hash as a placeholder
                self.target_state_root = Some(header_hash);
            }
        }
        Poll::Pending
    }
}

impl<C, S> Stage<impl DBProvider + StatsReader + HeaderProvider> for SnapSyncStage<C, S>
where
    C: SnapClient + Send + Sync + 'static,
    S: Stream<Item = B256> + Send + Unpin + 'static,
{
    fn id(&self) -> StageId {
        StageId::SnapSync
    }

    fn poll_execute_ready(
        &mut self,
        cx: &mut Context<'_>,
        _input: ExecInput,
    ) -> Poll<Result<(), StageError>> {
        // Check if snap sync is enabled
        if !self.config.enabled {
            return Poll::Ready(Ok(()));
        }

        // Poll for header updates
        self.poll_header_updates(cx);

        // Check if we have a target state root
        if self.target_state_root.is_none() {
            return Poll::Pending;
        }

        Poll::Ready(Ok(()))
    }

    fn execute(
        &mut self,
        provider: &impl DBProvider + StatsReader + HeaderProvider,
        input: ExecInput,
    ) -> Result<ExecOutput, StageError> {
        if !self.config.enabled {
            return Ok(ExecOutput::done(input.checkpoint()));
        }

        if input.target_reached() {
            return Ok(ExecOutput::done(input.checkpoint()));
        }

        // Check if we have a target state root
        let target_state_root = self.target_state_root
            .ok_or_else(|| StageError::Fatal("No target state root available".into()))?;

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

        // Placeholder: async execution needs proper handling
        let ranges_processed = 0;

        // Calculate progress
        let total_accounts = provider.count_entries::<tables::HashedAccounts>()? as u64;
        let entities_checkpoint = EntitiesCheckpoint {
            processed: total_accounts,
            total: total_accounts, // We don't know the total until we're done
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
        provider: &impl DBProvider + StatsReader + HeaderProvider,
        input: UnwindInput,
    ) -> Result<UnwindOutput, StageError> {
        let tx = provider.tx_ref();
        let mut cursor = tx.cursor_write::<tables::HashedAccounts>()?;
        
        // Placeholder: proper unwind logic needed
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
    use reth_provider::test_utils::MockNodeTypesWithDB;
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
            _priority: reth_net_p2p::priority::Priority,
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

        fn get_storage_ranges_with_priority(
            &self,
            _request: reth_eth_wire_types::snap::GetStorageRangesMessage,
            _priority: reth_net_p2p::priority::Priority,
        ) -> Self::Output {
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

        fn get_byte_codes_with_priority(
            &self,
            _request: reth_eth_wire_types::snap::GetByteCodesMessage,
            _priority: reth_net_p2p::priority::Priority,
        ) -> Self::Output {
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

        fn get_trie_nodes_with_priority(
            &self,
            _request: reth_eth_wire_types::snap::GetTrieNodesMessage,
            _priority: reth_net_p2p::priority::Priority,
        ) -> Self::Output {
            futures::future::ready(Ok(reth_net_p2p::error::WithPeerId {
                peer_id: reth_network_peers::PeerId::random(),
                result: reth_eth_wire_types::snap::TrieNodesMessage {
                    request_id: 1,
                    nodes: vec![],
                },
            }))
        }
    }

    #[tokio::test]
    async fn test_snap_sync_stage_creation() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let header_stream = futures::stream::empty::<B256>();
        
        let stage = SnapSyncStage::new(config, snap_client, header_stream);
        assert_eq!(stage.id(), StageId::SnapSync);
    }

    #[tokio::test]
    async fn test_snap_sync_stage_disabled() {
        let mut config = SnapSyncConfig::default();
        config.enabled = false;
        let snap_client = Arc::new(MockSnapClient);
        let header_stream = futures::stream::empty::<B256>();
        
        let mut stage = SnapSyncStage::new(config, snap_client, header_stream);
        let db = TestStageDB::default();
        let provider = db.factory.database_provider_rw().unwrap();
        let input = ExecInput { target: Some(100), checkpoint: None };
        
        let result = stage.execute(&provider, input);
        assert!(result.is_ok());
        assert!(result.unwrap().done);
    }

    #[tokio::test]
    async fn test_snap_sync_config_defaults() {
        let config = SnapSyncConfig::default();
        assert_eq!(config.max_ranges_per_execution, 100);
        assert_eq!(config.max_response_bytes, 2 * 1024 * 1024);
        assert!(!config.enabled);
    }

    #[test]
    fn test_account_range_request_creation() {
        let mut config = SnapSyncConfig::default();
        config.enabled = true;
        let snap_client = Arc::new(MockSnapClient);
        let header_stream = futures::stream::empty::<B256>();
        
        let mut stage = SnapSyncStage::new(config, snap_client, header_stream);
        stage.target_state_root = Some(B256::from([0x42; 32]));
        
        let request = stage.create_account_range_request(B256::ZERO, B256::from([0xff; 32]));
        assert_eq!(request.request_id, 1);
        assert_eq!(request.starting_hash, B256::ZERO);
        assert_eq!(request.limit_hash, B256::from([0xff; 32]));
        assert_eq!(request.root_hash, B256::from([0x42; 32]));
    }

    #[tokio::test]
    async fn test_hashed_state_empty_check() {
        let config = SnapSyncConfig::default();
        let snap_client = Arc::new(MockSnapClient);
        let header_stream = futures::stream::empty::<B256>();
        
        let stage = SnapSyncStage::new(config, snap_client, header_stream);
        let db = TestStageDB::default();
        let provider = db.factory.database_provider_rw().unwrap();
        
        // Empty database should return true
        assert!(stage.is_hashed_state_empty(&provider).unwrap());
    }
}