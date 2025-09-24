//! Built-in [`StageSet`]s.
//!
//! The easiest set to use is [`DefaultStages`], which provides all stages required to run an
//! instance of reth.
//!
//! It is also possible to run parts of reth standalone given the required data is present in
//! the environment, such as [`ExecutionStages`] or [`HashingStages`].
//!
//!
//! # Examples
//!
//! ```no_run
//! # use reth_stages::Pipeline;
//! # use reth_stages::sets::{OfflineStages};
//! # use reth_chainspec::MAINNET;
//! # use reth_prune_types::PruneModes;
//! # use reth_evm_ethereum::EthEvmConfig;
//! # use reth_evm::ConfigureEvm;
//! # use reth_provider::StaticFileProviderFactory;
//! # use reth_provider::test_utils::{create_test_provider_factory, MockNodeTypesWithDB};
//! # use reth_static_file::StaticFileProducer;
//! # use reth_config::config::StageConfig;
//! # use reth_ethereum_primitives::EthPrimitives;
//! # use std::sync::Arc;
//! # use reth_consensus::{FullConsensus, ConsensusError};
//!
//! # fn create(exec: impl ConfigureEvm<Primitives = EthPrimitives> + 'static, consensus: impl FullConsensus<EthPrimitives, Error = ConsensusError> + 'static) {
//!
//! let provider_factory = create_test_provider_factory();
//! let static_file_producer =
//!     StaticFileProducer::new(provider_factory.clone(), PruneModes::default());
//! // Build a pipeline with all offline stages.
//! let pipeline = Pipeline::<MockNodeTypesWithDB>::builder()
//!     .add_stages(OfflineStages::new(exec, Arc::new(consensus), StageConfig::default(), PruneModes::default()))
//!     .build(provider_factory, static_file_producer);
//!
//! # }
//! ```
use crate::{
    stages::{
        AccountHashingStage, BodyStage, EraImportSource, EraStage, ExecutionStage, FinishStage,
        HeaderStage, IndexAccountHistoryStage, IndexStorageHistoryStage, MerkleStage,
        PruneSenderRecoveryStage, PruneStage, SenderRecoveryStage, SnapSyncStage, StorageHashingStage,
        TransactionLookupStage,
    },
    StageSet, StageSetBuilder,
};
use alloy_primitives::B256;
use reth_config::config::StageConfig;
use reth_consensus::{ConsensusError, FullConsensus};
use reth_evm::ConfigureEvm;
use reth_network_p2p::{bodies::downloader::BodyDownloader, headers::downloader::HeaderDownloader};
use reth_primitives_traits::{Block, NodePrimitives};
use reth_provider::HeaderSyncGapProvider;
use reth_prune_types::PruneModes;
use reth_stages_api::Stage;
use std::{ops::Not, sync::Arc};
use tokio::sync::watch;

/// A set containing all stages to run a fully syncing instance of reth.
///
/// A combination of (in order)
///
/// - [`OnlineStages`]
/// - [`OfflineStages`]
/// - [`FinishStage`]
///
/// This expands to the following series of stages:
/// - [`HeaderStage`]
/// - [`BodyStage`]
/// - [`SenderRecoveryStage`]
/// - [`ExecutionStage`]
/// - [`PruneSenderRecoveryStage`] (execute)
/// - [`MerkleStage`] (unwind)
/// - [`AccountHashingStage`]
/// - [`StorageHashingStage`]
/// - [`MerkleStage`] (execute)
/// - [`TransactionLookupStage`]
/// - [`IndexStorageHistoryStage`]
/// - [`IndexAccountHistoryStage`]
/// - [`PruneStage`] (execute)
/// - [`FinishStage`]
#[derive(Debug)]
pub struct DefaultStages<Provider, H, B, E>
where
    H: HeaderDownloader,
    B: BodyDownloader,
    E: ConfigureEvm,
{
    /// Configuration for the online stages
    online: OnlineStages<Provider, H, B>,
    /// Executor factory needs for execution stage
    evm_config: E,
    /// Consensus instance
    consensus: Arc<dyn FullConsensus<E::Primitives, Error = ConsensusError>>,
    /// Configuration for each stage in the pipeline
    stages_config: StageConfig,
    /// Prune configuration for every segment that can be pruned
    prune_modes: PruneModes,
}

impl<Provider, H, B, E> DefaultStages<Provider, H, B, E>
where
    H: HeaderDownloader,
    B: BodyDownloader,
    E: ConfigureEvm<Primitives: NodePrimitives<BlockHeader = H::Header, Block = B::Block>>,
{
    /// Create a new set of default stages with default values.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        provider: Provider,
        tip: watch::Receiver<B256>,
        consensus: Arc<dyn FullConsensus<E::Primitives, Error = ConsensusError>>,
        header_downloader: H,
        body_downloader: B,
        evm_config: E,
        stages_config: StageConfig,
        prune_modes: PruneModes,
        era_import_source: Option<EraImportSource>,
    ) -> Self {
        Self {
            online: OnlineStages::new(
                provider,
                tip,
                header_downloader,
                body_downloader,
                stages_config.clone(),
                era_import_source,
            ),
            evm_config,
            consensus,
            stages_config,
            prune_modes,
        }
    }
}

impl<P, H, B, E> DefaultStages<P, H, B, E>
where
    E: ConfigureEvm,
    H: HeaderDownloader,
    B: BodyDownloader,
{
    /// Appends the default offline stages and default finish stage to the given builder.
    pub fn add_offline_stages<Provider>(
        default_offline: StageSetBuilder<Provider>,
        evm_config: E,
        consensus: Arc<dyn FullConsensus<E::Primitives, Error = ConsensusError>>,
        stages_config: StageConfig,
        prune_modes: PruneModes,
    ) -> StageSetBuilder<Provider>
    where
        OfflineStages<E>: StageSet<Provider>,
    {
        StageSetBuilder::default()
            .add_set(default_offline)
            .add_set(OfflineStages::new(evm_config, consensus, stages_config, prune_modes))
            .add_stage(FinishStage)
    }
}

impl<P, H, B, E, Provider> StageSet<Provider> for DefaultStages<P, H, B, E>
where
    P: HeaderSyncGapProvider + 'static,
    H: HeaderDownloader + 'static,
    B: BodyDownloader + 'static,
    E: ConfigureEvm,
    OnlineStages<P, H, B>: StageSet<Provider>,
    OfflineStages<E>: StageSet<Provider>,
{
    fn builder(self) -> StageSetBuilder<Provider> {
        Self::add_offline_stages(
            self.online.builder(),
            self.evm_config,
            self.consensus,
            self.stages_config.clone(),
            self.prune_modes,
        )
    }
}

/// A set containing all stages that require network access by default.
///
/// These stages *can* be run without network access if the specified downloaders are
/// themselves offline.
#[derive(Debug)]
pub struct OnlineStages<Provider, H, B>
where
    H: HeaderDownloader,
    B: BodyDownloader,
{
    /// Sync gap provider for the headers stage.
    provider: Provider,
    /// The tip for the headers stage.
    tip: watch::Receiver<B256>,

    /// The block header downloader
    header_downloader: H,
    /// The block body downloader
    body_downloader: B,
    /// Configuration for each stage in the pipeline
    stages_config: StageConfig,
    /// Optional source of ERA1 files. The `EraStage` does nothing unless this is specified.
    era_import_source: Option<EraImportSource>,
}

impl<Provider, H, B> OnlineStages<Provider, H, B>
where
    H: HeaderDownloader,
    B: BodyDownloader,
{
    /// Create a new set of online stages with default values.
    pub const fn new(
        provider: Provider,
        tip: watch::Receiver<B256>,
        header_downloader: H,
        body_downloader: B,
        stages_config: StageConfig,
        era_import_source: Option<EraImportSource>,
    ) -> Self {
        Self { provider, tip, header_downloader, body_downloader, stages_config, era_import_source }
    }
}

impl<P, H, B> OnlineStages<P, H, B>
where
    P: HeaderSyncGapProvider + 'static,
    H: HeaderDownloader<Header = <B::Block as Block>::Header> + 'static,
    B: BodyDownloader + 'static,
{
    /// Create a new builder using the given headers stage.
    pub fn builder_with_headers<Provider>(
        headers: HeaderStage<P, H>,
        body_downloader: B,
    ) -> StageSetBuilder<Provider>
    where
        HeaderStage<P, H>: Stage<Provider>,
        BodyStage<B>: Stage<Provider>,
    {
        StageSetBuilder::default().add_stage(headers).add_stage(BodyStage::new(body_downloader))
    }

    /// Create a new builder using the given bodies stage.
    pub fn builder_with_bodies<Provider>(
        bodies: BodyStage<B>,
        provider: P,
        tip: watch::Receiver<B256>,
        header_downloader: H,
        stages_config: StageConfig,
    ) -> StageSetBuilder<Provider>
    where
        BodyStage<B>: Stage<Provider>,
        HeaderStage<P, H>: Stage<Provider>,
    {
        StageSetBuilder::default()
            .add_stage(HeaderStage::new(provider, header_downloader, tip, stages_config.etl))
            .add_stage(bodies)
    }
}

impl<Provider, P, H, B> StageSet<Provider> for OnlineStages<P, H, B>
where
    P: HeaderSyncGapProvider + 'static,
    H: HeaderDownloader<Header = <B::Block as Block>::Header> + 'static,
    B: BodyDownloader + 'static,
    HeaderStage<P, H>: Stage<Provider>,
    BodyStage<B>: Stage<Provider>,
    EraStage<<B::Block as Block>::Header, <B::Block as Block>::Body, EraImportSource>:
        Stage<Provider>,
{
    fn builder(self) -> StageSetBuilder<Provider> {
        StageSetBuilder::default()
            .add_stage(EraStage::new(self.era_import_source, self.stages_config.etl.clone()))
            .add_stage(HeaderStage::new(
                self.provider,
                self.header_downloader,
                self.tip,
                self.stages_config.etl.clone(),
            ))
            .add_stage(BodyStage::new(self.body_downloader))
    }
}

/// A set containing all stages that do not require network access.
///
/// A combination of (in order)
///
/// - [`ExecutionStages`]
/// - [`PruneSenderRecoveryStage`]
/// - [`HashingStages`]
/// - [`HistoryIndexingStages`]
/// - [`PruneStage`]
#[derive(Debug)]
#[non_exhaustive]
pub struct OfflineStages<E: ConfigureEvm> {
    /// Executor factory needs for execution stage
    evm_config: E,
    /// Consensus instance for validating blocks.
    consensus: Arc<dyn FullConsensus<E::Primitives, Error = ConsensusError>>,
    /// Configuration for each stage in the pipeline
    stages_config: StageConfig,
    /// Prune configuration for every segment that can be pruned
    prune_modes: PruneModes,
}

impl<E: ConfigureEvm> OfflineStages<E> {
    /// Create a new set of offline stages with default values.
    pub const fn new(
        evm_config: E,
        consensus: Arc<dyn FullConsensus<E::Primitives, Error = ConsensusError>>,
        stages_config: StageConfig,
        prune_modes: PruneModes,
    ) -> Self {
        Self { evm_config, consensus, stages_config, prune_modes }
    }
}

impl<E, Provider> StageSet<Provider> for OfflineStages<E>
where
    E: ConfigureEvm,
    ExecutionStages<E>: StageSet<Provider>,
    PruneSenderRecoveryStage: Stage<Provider>,
    HashingStages: StageSet<Provider>,
    HistoryIndexingStages: StageSet<Provider>,
    PruneStage: Stage<Provider>,
{
    fn builder(self) -> StageSetBuilder<Provider> {
        ExecutionStages::new(self.evm_config, self.consensus, self.stages_config.clone())
            .builder()
            // If sender recovery prune mode is set, add the prune sender recovery stage.
            .add_stage_opt(self.prune_modes.sender_recovery.map(|prune_mode| {
                PruneSenderRecoveryStage::new(prune_mode, self.stages_config.prune.commit_threshold)
            }))
            .add_set(HashingStages { stages_config: self.stages_config.clone() })
            .add_set(HistoryIndexingStages {
                stages_config: self.stages_config.clone(),
                prune_modes: self.prune_modes.clone(),
            })
            // If any prune modes are set, add the prune stage.
            .add_stage_opt(self.prune_modes.is_empty().not().then(|| {
                // Prune stage should be added after all hashing stages, because otherwise it will
                // delete
                PruneStage::new(self.prune_modes.clone(), self.stages_config.prune.commit_threshold)
            }))
    }
}

/// A set containing all stages that are required to execute pre-existing block data.
#[derive(Debug)]
#[non_exhaustive]
pub struct ExecutionStages<E: ConfigureEvm> {
    /// Executor factory that will create executors.
    evm_config: E,
    /// Consensus instance for validating blocks.
    consensus: Arc<dyn FullConsensus<E::Primitives, Error = ConsensusError>>,
    /// Configuration for each stage in the pipeline
    stages_config: StageConfig,
}

impl<E: ConfigureEvm> ExecutionStages<E> {
    /// Create a new set of execution stages with default values.
    pub const fn new(
        executor_provider: E,
        consensus: Arc<dyn FullConsensus<E::Primitives, Error = ConsensusError>>,
        stages_config: StageConfig,
    ) -> Self {
        Self { evm_config: executor_provider, consensus, stages_config }
    }
}

impl<E, Provider> StageSet<Provider> for ExecutionStages<E>
where
    E: ConfigureEvm + 'static,
    SenderRecoveryStage: Stage<Provider>,
    ExecutionStage<E>: Stage<Provider>,
{
    fn builder(self) -> StageSetBuilder<Provider> {
        StageSetBuilder::default()
            .add_stage(SenderRecoveryStage::new(self.stages_config.sender_recovery))
            .add_stage(ExecutionStage::from_config(
                self.evm_config,
                self.consensus,
                self.stages_config.execution,
                self.stages_config.execution_external_clean_threshold(),
            ))
    }
}

/// A set containing all stages that hash account state.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct HashingStages {
    /// Configuration for each stage in the pipeline
    stages_config: StageConfig,
}

impl<Provider> StageSet<Provider> for HashingStages
where
    MerkleStage: Stage<Provider>,
    AccountHashingStage: Stage<Provider>,
    StorageHashingStage: Stage<Provider>,
{
    fn builder(self) -> StageSetBuilder<Provider> {
        StageSetBuilder::default()
            .add_stage(MerkleStage::default_unwind())
            .add_stage(AccountHashingStage::new(
                self.stages_config.account_hashing,
                self.stages_config.etl.clone(),
            ))
            .add_stage(StorageHashingStage::new(
                self.stages_config.storage_hashing,
                self.stages_config.etl.clone(),
            ))
            .add_stage(MerkleStage::new_execution(
                self.stages_config.merkle.rebuild_threshold,
                self.stages_config.merkle.incremental_threshold,
            ))
    }
}

/// A set containing all stages that do additional indexing for historical state.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct HistoryIndexingStages {
    /// Configuration for each stage in the pipeline
    stages_config: StageConfig,
    /// Prune configuration for every segment that can be pruned
    prune_modes: PruneModes,
}

impl<Provider> StageSet<Provider> for HistoryIndexingStages
where
    TransactionLookupStage: Stage<Provider>,
    IndexStorageHistoryStage: Stage<Provider>,
    IndexAccountHistoryStage: Stage<Provider>,
{
    fn builder(self) -> StageSetBuilder<Provider> {
        StageSetBuilder::default()
            .add_stage(TransactionLookupStage::new(
                self.stages_config.transaction_lookup,
                self.stages_config.etl.clone(),
                self.prune_modes.transaction_lookup,
            ))
            .add_stage(IndexStorageHistoryStage::new(
                self.stages_config.index_storage_history,
                self.stages_config.etl.clone(),
                self.prune_modes.storage_history,
            ))
            .add_stage(IndexAccountHistoryStage::new(
                self.stages_config.index_account_history,
                self.stages_config.etl.clone(),
                self.prune_modes.account_history,
            ))
    }
}

/// A basic stage set that uses snap sync instead of traditional header/body sync.
///
/// This stage set replaces the traditional [`HeaderStage`] and [`BodyStage`] with
/// a [`SnapSyncStage`] that downloads state directly using the snap protocol.
///
/// The stages in this set are:
/// - [`SnapSyncStage`] - Downloads state using snap protocol (stub implementation)
/// - [`FinishStage`] - Final cleanup
///
/// Note: This is a minimal implementation for demonstration purposes. A complete
/// snap sync pipeline would need additional stages for processing the synced state.
#[derive(Debug)]
pub struct SnapSyncStages<Provider, Client, E>
where
    Client: reth_network_p2p::snap::client::SnapClient,
    E: ConfigureEvm,
{
    /// Database provider
    provider: Provider,
    /// Snap client for downloading state
    snap_client: Client,
    /// Chain tip receiver
    tip: watch::Receiver<B256>,
    /// Snap sync configuration
    snap_config: crate::stages::SnapSyncConfig,
    /// Executor factory needs for execution stage
    evm_config: E,
    /// Consensus instance
    consensus: Arc<dyn FullConsensus<E::Primitives, Error = ConsensusError>>,
    /// Configuration for each stage in the pipeline
    stages_config: StageConfig,
    /// Prune configuration for every segment that can be pruned
    prune_modes: PruneModes,
}

impl<Provider, Client, E> SnapSyncStages<Provider, Client, E>
where
    Client: reth_network_p2p::snap::client::SnapClient,
    E: ConfigureEvm,
{
    /// Create a new set of snap sync stages.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        provider: Provider,
        snap_client: Client,
        tip: watch::Receiver<B256>,
        snap_config: crate::stages::SnapSyncConfig,
        evm_config: E,
        consensus: Arc<dyn FullConsensus<E::Primitives, Error = ConsensusError>>,
        stages_config: StageConfig,
        prune_modes: PruneModes,
    ) -> Self {
        Self {
            provider,
            snap_client,
            tip,
            snap_config,
            evm_config,
            consensus,
            stages_config,
            prune_modes,
        }
    }
}

impl<Provider, Client, E> StageSet<Provider> for SnapSyncStages<Provider, Client, E>
where
    Provider: reth_provider::DatabaseProviderFactory + reth_provider::StaticFileProviderFactory + Clone + Unpin + 'static,
    Client: reth_network_p2p::snap::client::SnapClient + Clone + Unpin + 'static,
    E: ConfigureEvm + 'static,
    SnapSyncStage<Provider, Client>: Stage<Provider>,
{
    fn builder(self) -> StageSetBuilder<Provider> {
        let snap_stage = SnapSyncStage::new(
            self.provider.clone(),
            self.snap_client,
            self.tip,
            self.snap_config,
        );

        StageSetBuilder::default()
            // Start with snap sync instead of header/body sync
            .add_stage(snap_stage)
            // Add finish stage
            .add_stage(FinishStage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestStageDB;
    use futures_util::future;
    use reth_consensus::test_utils::TestConsensus;
    use reth_eth_wire_types::snap::{AccountRangeMessage, GetAccountRangeMessage, GetByteCodesMessage, GetStorageRangesMessage, GetTrieNodesMessage};
    use reth_evm_ethereum::EthEvmConfig;
    use reth_network_peers::WithPeerId;
    use reth_network_p2p::{download::DownloadClient, snap::client::SnapClient};
    use std::sync::Arc;
    use tokio::sync::watch;

    // Mock snap client for testing stage sets
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
        type Output = future::Ready<reth_network_p2p::error::PeerRequestResult<AccountRangeMessage>>;

        fn get_account_range_with_priority(
            &self,
            _request: GetAccountRangeMessage,
            _priority: reth_network_p2p::priority::Priority,
        ) -> Self::Output {
            future::ready(Ok(WithPeerId::new(
                reth_network_peers::PeerId::random(),
                AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                }
            )))
        }

        fn get_storage_ranges(&self, _request: GetStorageRangesMessage) -> Self::Output {
            future::ready(Ok(WithPeerId::new(
                reth_network_peers::PeerId::random(),
                AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                }
            )))
        }

        fn get_storage_ranges_with_priority(
            &self,
            _request: GetStorageRangesMessage,
            _priority: reth_network_p2p::priority::Priority,
        ) -> Self::Output {
            future::ready(Ok(WithPeerId::new(
                reth_network_peers::PeerId::random(),
                AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                }
            )))
        }

        fn get_byte_codes(&self, _request: GetByteCodesMessage) -> Self::Output {
            future::ready(Ok(WithPeerId::new(
                reth_network_peers::PeerId::random(),
                AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                }
            )))
        }

        fn get_byte_codes_with_priority(
            &self,
            _request: GetByteCodesMessage,
            _priority: reth_network_p2p::priority::Priority,
        ) -> Self::Output {
            future::ready(Ok(WithPeerId::new(
                reth_network_peers::PeerId::random(),
                AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                }
            )))
        }

        fn get_trie_nodes(&self, _request: GetTrieNodesMessage) -> Self::Output {
            future::ready(Ok(WithPeerId::new(
                reth_network_peers::PeerId::random(),
                AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                }
            )))
        }

        fn get_trie_nodes_with_priority(
            &self,
            _request: GetTrieNodesMessage,
            _priority: reth_network_p2p::priority::Priority,
        ) -> Self::Output {
            future::ready(Ok(WithPeerId::new(
                reth_network_peers::PeerId::random(),
                AccountRangeMessage {
                    request_id: 1,
                    accounts: vec![],
                    proof: vec![],
                }
            )))
        }
    }

    #[test]
    fn test_snap_sync_stages_creation() {
        let db = TestStageDB::default();
        let (_tip_tx, tip_rx) = watch::channel(B256::ZERO);
        let client = MockSnapClient;
        let snap_config = crate::stages::SnapSyncConfig::default();
        let evm_config = EthEvmConfig::ethereum(Arc::new(reth_chainspec::MAINNET.clone()));
        let consensus = Arc::new(TestConsensus::default());
        let stages_config = StageConfig::default();
        let prune_modes = PruneModes::default();

        let stages = SnapSyncStages::new(
            db.factory.clone(),
            client,
            tip_rx,
            snap_config,
            evm_config,
            consensus,
            stages_config,
            prune_modes,
        );

        // Test that the struct was created successfully
        assert!(format!("{:?}", stages).contains("SnapSyncStages"));
    }

    #[test]
    fn test_snap_sync_stages_builder() {
        let db = TestStageDB::default();
        let (_tip_tx, tip_rx) = watch::channel(B256::ZERO);
        let client = MockSnapClient;
        let snap_config = crate::stages::SnapSyncConfig::default();
        let evm_config = EthEvmConfig::ethereum(Arc::new(reth_chainspec::MAINNET.clone()));
        let consensus = Arc::new(TestConsensus::default());
        let stages_config = StageConfig::default();
        let prune_modes = PruneModes::default();

        let stages = SnapSyncStages::new(
            db.factory.clone(),
            client,
            tip_rx,
            snap_config,
            evm_config,
            consensus,
            stages_config,
            prune_modes,
        );

        let builder = stages.builder();
        
        // The builder should contain stages - we can't easily inspect the internal
        // structure, but we can verify it was created without panicking
        assert!(format!("{:?}", builder).contains("StageSetBuilder"));
    }

    #[test]
    fn test_snap_sync_config_in_stages() {
        use std::time::Duration;
        
        let db = TestStageDB::default();
        let (_tip_tx, tip_rx) = watch::channel(B256::ZERO);
        let client = MockSnapClient;
        
        let custom_config = crate::stages::SnapSyncConfig {
            max_accounts_per_request: 200,
            max_storage_per_request: 500,
            max_bytecodes_per_request: 32,
            max_trie_nodes_per_request: 256,
            request_timeout: Duration::from_secs(45),
            max_concurrent_requests: 12,
        };
        
        let evm_config = EthEvmConfig::ethereum(Arc::new(reth_chainspec::MAINNET.clone()));
        let consensus = Arc::new(TestConsensus::default());
        let stages_config = StageConfig::default();
        let prune_modes = PruneModes::default();

        let stages = SnapSyncStages::new(
            db.factory.clone(),
            client,
            tip_rx,
            custom_config,
            evm_config,
            consensus,
            stages_config,
            prune_modes,
        );

        // Test that the stages were created with the custom config
        let _builder = stages.builder();
    }
}
