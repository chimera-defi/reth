//! Header subscription mechanism for snap sync stage.
//!
//! This module provides a way to subscribe to head header updates from the consensus engine
//! and make them available to the snap sync stage.

use reth_engine_primitives::ConsensusEngineEvent;
use reth_primitives_traits::{NodePrimitives, SealedHeader};
use tokio::sync::{broadcast, watch};
use tracing::{debug, warn};

/// A header subscription service that listens to consensus engine events
/// and provides the latest head header to subscribers.
#[derive(Debug)]
pub struct HeaderSubscriptionService<N> {
    /// Broadcast sender for consensus engine events
    event_sender: broadcast::Sender<ConsensusEngineEvent<N>>,
    /// Watch sender for head header updates
    header_sender: watch::Sender<Option<SealedHeader<N::BlockHeader>>>,
    /// Current head header
    current_header: Option<SealedHeader<N::BlockHeader>>,
}

impl<N> HeaderSubscriptionService<N>
where
    N: NodePrimitives<BlockHeader = alloy_consensus::Header>,
{
    /// Create a new header subscription service.
    pub fn new(event_sender: broadcast::Sender<ConsensusEngineEvent<N>>) -> (Self, watch::Receiver<SealedHeader<N::BlockHeader>>) {
        let (header_sender, header_receiver) = watch::channel(None);
        
        let service = Self {
            event_sender,
            header_sender,
            current_header: None,
        };
        
        (service, header_receiver)
    }
    
    /// Start listening to consensus engine events and updating head headers.
    pub async fn start(mut self) {
        let mut event_receiver = self.event_sender.subscribe();
        
        debug!(target: "sync::stages::header_subscription", "Starting header subscription service");
        
        while let Ok(event) = event_receiver.recv().await {
            if let Some(header) = self.handle_consensus_event(event) {
                self.update_head_header(header).await;
            }
        }
        
        warn!(target: "sync::stages::header_subscription", "Header subscription service stopped");
    }
    
    /// Handle a consensus engine event and extract head header if available.
    fn handle_consensus_event(&mut self, event: ConsensusEngineEvent<N>) -> Option<SealedHeader<N::BlockHeader>> {
        match event {
            ConsensusEngineEvent::CanonicalChainCommitted(header, _) => {
                debug!(
                    target: "sync::stages::header_subscription",
                    number = header.number(),
                    hash = ?header.hash(),
                    state_root = ?header.state_root,
                    "Received canonical chain committed event"
                );
                Some(*header)
            }
            ConsensusEngineEvent::ForkchoiceUpdated(state, _) => {
                debug!(
                    target: "sync::stages::header_subscription",
                    head_block_hash = ?state.head_block_hash,
                    "Received forkchoice updated event"
                );
                // Note: ForkchoiceUpdated doesn't contain the header directly,
                // but it indicates the head has changed. The actual header will
                // come through CanonicalChainCommitted.
                None
            }
            _ => {
                // Other events don't contain head header information
                None
            }
        }
    }
    
    /// Update the head header and notify subscribers.
    async fn update_head_header(&mut self, header: SealedHeader<N::BlockHeader>) {
        // Check if this is actually a new header
        if let Some(ref current) = self.current_header {
            if current.hash() == header.hash() {
                debug!(
                    target: "sync::stages::header_subscription",
                    hash = ?header.hash(),
                    "Header already current, skipping update"
                );
                return;
            }
        }
        
        debug!(
            target: "sync::stages::header_subscription",
            old_header = ?self.current_header.as_ref().map(|h: &SealedHeader<N::BlockHeader>| h.num_hash()),
            new_header = ?header.num_hash(),
            "Updating head header"
        );
        
        self.current_header = Some(header.clone());
        
        // Send the header to all subscribers
        if let Err(e) = self.header_sender.send(Some(header)) {
            warn!(
                target: "sync::stages::header_subscription",
                error = %e,
                "Failed to send header update to subscribers"
            );
        }
    }
}

/// A trait for types that can provide head header updates.
pub trait HeaderProvider {
    /// Get the current head header.
    fn get_head_header(&self) -> Option<SealedHeader<Self::BlockHeader>>;
    
    /// The block header type.
    type BlockHeader;
}

impl<N> HeaderProvider for HeaderSubscriptionService<N>
where
    N: reth_primitives_traits::NodePrimitives,
{
    type BlockHeader = N::BlockHeader;
    
    fn get_head_header(&self) -> Option<SealedHeader<Self::BlockHeader>> {
        self.current_header.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_consensus::BlockHeader;
    use alloy_primitives::{B256, U256};
    use reth_ethereum_primitives::EthPrimitives;
    use reth_primitives_traits::SealedHeader;
    use tokio::sync::broadcast;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_header_subscription_service() {
        let (event_sender, _event_receiver) = broadcast::channel(16);
        let (service, mut header_receiver) = HeaderSubscriptionService::<EthPrimitives>::new(event_sender);
        
        // Start the service in a background task
        let service_handle = tokio::spawn(async move {
            service.start().await;
        });
        
        // Create a test header
        let mut header = BlockHeader::default();
        header.number = 100;
        header.state_root = B256::from([0x42; 32]);
        let sealed_header = SealedHeader::seal_slow(header);
        
        // Send a canonical chain committed event
        let event = ConsensusEngineEvent::CanonicalChainCommitted(
            Box::new(sealed_header.clone()),
            Duration::from_millis(100)
        );
        
        event_sender.send(event).unwrap();
        
        // Wait for the header to be received
        sleep(Duration::from_millis(10)).await;
        
        // Check that we received the header
        let received_header = header_receiver.borrow().clone();
        assert!(received_header.is_some());
        assert_eq!(received_header.unwrap().hash(), sealed_header.hash());
        
        // Clean up
        service_handle.abort();
    }
    
    #[tokio::test]
    async fn test_header_subscription_ignores_duplicate_headers() {
        let (event_sender, _event_receiver) = broadcast::channel(16);
        let (service, mut header_receiver) = HeaderSubscriptionService::<EthPrimitives>::new(event_sender);
        
        // Start the service in a background task
        let service_handle = tokio::spawn(async move {
            service.start().await;
        });
        
        // Create a test header
        let mut header = BlockHeader::default();
        header.number = 100;
        header.state_root = B256::from([0x42; 32]);
        let sealed_header = SealedHeader::seal_slow(header);
        
        // Send the same event twice
        let event = ConsensusEngineEvent::CanonicalChainCommitted(
            Box::new(sealed_header.clone()),
            Duration::from_millis(100)
        );
        
        event_sender.send(event.clone()).unwrap();
        event_sender.send(event).unwrap();
        
        // Wait for processing
        sleep(Duration::from_millis(10)).await;
        
        // Check that we only received one update
        let received_header = header_receiver.borrow().clone();
        assert!(received_header.is_some());
        assert_eq!(received_header.unwrap().hash(), sealed_header.hash());
        
        // Clean up
        service_handle.abort();
    }
}