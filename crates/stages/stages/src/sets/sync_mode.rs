//! Sync mode-based stage selection

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

/// Sync mode for stage selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// Full sync - download and verify all blocks from genesis
    Full,
    /// Snap sync - fast state synchronization using snap protocol
    Snap,
    /// Checkpoint sync - sync from a trusted checkpoint
    Checkpoint,
}

/// A set containing stages based on sync mode
///
/// This provides different stage configurations based on the sync mode:
/// - **Full**: HeaderStage → BodyStage → ExecutionStages
/// - **Snap**: SnapSyncStage → ExecutionStages (skips header/body download)
/// - **Checkpoint**: Similar to snap but with checkpoint validation
#[derive(Debug)]
pub struct SyncModeStages<Provider, H, B, S, E>
where
    H: HeaderDownloader,
    B: BodyDownloader,
    S: reth_network_p2p::snap::client::SnapClient,
    E: ConfigureEvm,
{
    /// Sync mode
    sync_mode: SyncMode,
    /// Configuration for the online stages
    online: OnlineStages<Provider, H, B, S>,
    /// Executor factory needs for execution stage
    evm_config: E,
    /// Consensus instance
    consensus: Arc<dyn FullConsensus<E::Primitives, Error = ConsensusError>>,
    /// Configuration for each stage in the pipeline
    stages_config: StageConfig,
    /// Prune configuration for every segment that can be pruned
    prune_modes: PruneModes,
    /// Optional source of ERA1 files
    era_import_source: Option<EraImportSource>,
}

impl<Provider, H, B, S, E> SyncModeStages<Provider, H, B, S, E>
where
    H: HeaderDownloader,
    B: BodyDownloader,
    S: reth_network_p2p::snap::client::SnapClient,
    E: ConfigureEvm,
{
    /// Create a new sync mode stages set
    pub fn new(
        sync_mode: SyncMode,
        provider: Provider,
        tip: watch::Receiver<B256>,
        header_downloader: H,
        body_downloader: B,
        snap_client: Arc<S>,
        evm_config: E,
        consensus: Arc<dyn FullConsensus<E::Primitives, Error = ConsensusError>>,
        stages_config: StageConfig,
        prune_modes: PruneModes,
        era_import_source: Option<EraImportSource>,
    ) -> Self {
        let online = OnlineStages::new(
            provider,
            tip,
            header_downloader,
            body_downloader,
            snap_client,
            stages_config.clone(),
            era_import_source,
        );

        Self {
            sync_mode,
            online,
            evm_config,
            consensus,
            stages_config,
            prune_modes,
            era_import_source,
        }
    }
}

impl<P, H, B, S, E> StageSet<P> for SyncModeStages<P, H, B, S, E>
where
    P: HeaderSyncGapProvider + 'static,
    H: HeaderDownloader<Header = <B::Block as Block>::Header> + 'static,
    B: BodyDownloader + 'static,
    S: reth_network_p2p::snap::client::SnapClient + 'static,
    E: ConfigureEvm + 'static,
    E::Primitives: NodePrimitives,
{
    fn builder(self) -> StageSetBuilder<P> {
        let mut builder = StageSetBuilder::default();

        // Add stages based on sync mode
        match self.sync_mode {
            SyncMode::Full => {
                // Traditional full sync: Headers → Bodies → Execution
                builder = self.online.builder_with_headers(
                    HeaderStage::new(
                        self.online.provider.clone(),
                        self.online.header_downloader,
                        self.online.tip,
                        self.stages_config.etl.clone(),
                    ),
                    self.online.body_downloader,
                    self.online.snap_client,
                    self.stages_config.clone(),
                );
            }
            SyncMode::Snap => {
                // Snap sync: SnapSync → Execution (skips headers/bodies)
                builder = builder
                    .add_stage(SnapSyncStage::new(
                        self.online.provider.clone(),
                        self.online.snap_client,
                        self.stages_config.snap_sync,
                        self.stages_config.etl.clone(),
                    ));
            }
            SyncMode::Checkpoint => {
                // Checkpoint sync: Similar to snap but with checkpoint validation
                // For now, treat as snap sync - checkpoint validation can be added later
                builder = builder
                    .add_stage(SnapSyncStage::new(
                        self.online.provider.clone(),
                        self.online.snap_client,
                        self.stages_config.snap_sync,
                        self.stages_config.etl.clone(),
                    ));
            }
        }

        // Add execution stages (common to all sync modes)
        builder = self.add_execution_stages(builder);

        builder
    }

    fn stages(&self) -> Vec<Box<dyn Stage<P>>> {
        // This is a fallback implementation - the builder method should be used
        vec![]
    }
}

impl<P, H, B, S, E> SyncModeStages<P, H, B, S, E>
where
    P: HeaderSyncGapProvider + 'static,
    H: HeaderDownloader<Header = <B::Block as Block>::Header> + 'static,
    B: BodyDownloader + 'static,
    S: reth_network_p2p::snap::client::SnapClient + 'static,
    E: ConfigureEvm + 'static,
    E::Primitives: NodePrimitives,
{
    /// Add execution stages that are common to all sync modes
    fn add_execution_stages(self, mut builder: StageSetBuilder<P>) -> StageSetBuilder<P> {
        // Add execution stages
        builder = builder
            .add_stage(SenderRecoveryStage::new(
                self.stages_config.sender_recovery,
                self.stages_config.etl.clone(),
            ))
            .add_stage(ExecutionStage::new(
                self.evm_config,
                self.consensus,
                self.stages_config.execution,
                self.stages_config.etl.clone(),
            ))
            .add_stage(PruneSenderRecoveryStage::new(
                self.stages_config.sender_recovery,
                self.stages_config.etl.clone(),
            ));

        // Add Merkle stage (unwind)
        builder = builder.add_stage(MerkleStage::new_unwind(
            self.stages_config.merkle,
            self.stages_config.etl.clone(),
        ));

        // Add hashing stages
        builder = builder
            .add_stage(AccountHashingStage::new(
                self.stages_config.account_hashing,
                self.stages_config.etl.clone(),
            ))
            .add_stage(StorageHashingStage::new(
                self.stages_config.storage_hashing,
                self.stages_config.etl.clone(),
            ));

        // Add Merkle stage (execute)
        builder = builder.add_stage(MerkleStage::new_execute(
            self.stages_config.merkle,
            self.stages_config.etl.clone(),
        ));

        // Add indexing stages
        builder = builder
            .add_stage(TransactionLookupStage::new(
                self.stages_config.transaction_lookup,
                self.stages_config.etl.clone(),
            ))
            .add_stage(IndexStorageHistoryStage::new(
                self.stages_config.index_storage_history,
                self.stages_config.etl.clone(),
            ))
            .add_stage(IndexAccountHistoryStage::new(
                self.stages_config.index_account_history,
                self.stages_config.etl.clone(),
            ));

        // Add prune stage
        if self.prune_modes != PruneModes::default() {
            builder = builder.add_stage(PruneStage::new(
                self.prune_modes,
                self.stages_config.prune,
                self.stages_config.etl.clone(),
            ));
        }

        // Add finish stage
        builder.add_stage(FinishStage::new(self.stages_config.finish))
    }
}

/// Online stages for sync mode
#[derive(Debug)]
pub struct OnlineStages<Provider, H, B, S>
where
    H: HeaderDownloader,
    B: BodyDownloader,
    S: reth_network_p2p::snap::client::SnapClient,
{
    /// Sync gap provider for the headers stage.
    provider: Provider,
    /// The tip for the headers stage.
    tip: watch::Receiver<B256>,

    /// The block header downloader
    header_downloader: H,
    /// The block body downloader
    body_downloader: B,
    /// The snap sync client
    snap_client: Arc<S>,
    /// Configuration for each stage in the pipeline
    stages_config: StageConfig,
    /// Optional source of ERA1 files. The `EraStage` does nothing unless this is specified.
    era_import_source: Option<EraImportSource>,
}

impl<Provider, H, B, S> OnlineStages<Provider, H, B, S>
where
    H: HeaderDownloader,
    B: BodyDownloader,
    S: reth_network_p2p::snap::client::SnapClient,
{
    /// Create a new set of online stages with default values.
    pub fn new(
        provider: Provider,
        tip: watch::Receiver<B256>,
        header_downloader: H,
        body_downloader: B,
        snap_client: Arc<S>,
        stages_config: StageConfig,
        era_import_source: Option<EraImportSource>,
    ) -> Self {
        Self { 
            provider, 
            tip, 
            header_downloader, 
            body_downloader, 
            snap_client,
            stages_config, 
            era_import_source 
        }
    }

    /// Create a new builder using the given headers stage (for full sync)
    pub fn builder_with_headers<Provider>(
        headers: HeaderStage<P, H>,
        body_downloader: B,
        snap_client: Arc<S>,
        stages_config: StageConfig,
    ) -> StageSetBuilder<Provider>
    where
        HeaderStage<P, H>: Stage<Provider>,
        BodyStage<B>: Stage<Provider>,
    {
        StageSetBuilder::default()
            .add_stage(headers)
            .add_stage(BodyStage::new(body_downloader))
    }

    /// Create a new builder using the given bodies stage (for full sync)
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
            .add_stage(HeaderStage::new(provider.clone(), header_downloader, tip, stages_config.etl.clone()))
            .add_stage(bodies)
    }
}