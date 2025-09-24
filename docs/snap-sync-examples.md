# Snap Sync Usage Examples

## Overview

This document provides practical examples of how to use the snap sync implementation in Reth. These examples demonstrate common use cases and best practices.

## Basic Examples

### 1. Simple Account Range Download

```rust
use reth_network_downloaders::snap::{SnapSyncDownloader, SnapSyncResult};
use reth_config::config::SnapSyncConfig;
use reth_network_downloaders::snap::test_utils::TestSnapClient;
use alloy_primitives::B256;
use futures::StreamExt;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a test client (in production, use a real snap client)
    let client = Arc::new(TestSnapClient::new());
    
    // Create configuration
    let config = SnapSyncConfig::default();
    
    // Create a mock provider (in production, use real database provider)
    let provider = reth_provider::test_utils::MockProvider::default();
    
    // Create downloader
    let mut downloader = SnapSyncDownloader::new(client, provider, config);
    
    // Start downloading account ranges
    let state_root = B256::from([1u8; 32]);
    downloader.start_account_range_download(state_root).await?;
    
    // Process the stream of results
    let mut stream = Box::pin(downloader);
    while let Some(result) = stream.next().await {
        match result? {
            SnapSyncResult::AccountRange(accounts) => {
                println!("Received {} accounts", accounts.accounts.len());
                for account in accounts.accounts {
                    println!("Account hash: {:?}", account.hash);
                    println!("Account data length: {}", account.body.len());
                }
            }
            _ => {
                // Handle other result types
            }
        }
    }
    
    Ok(())
}
```

### 2. Custom Configuration

```rust
use reth_config::config::SnapSyncConfig;

fn create_custom_config() -> SnapSyncConfig {
    SnapSyncConfig {
        max_concurrent_requests: 20,           // More concurrent requests
        max_response_bytes: 4 * 1024 * 1024,  // 4MB response limit
        max_accounts_per_request: 2000,       // Larger batches
        max_storage_slots_per_request: 2000,
        max_byte_codes_per_request: 200,
        max_trie_nodes_per_request: 2000,
        commit_threshold: 20000,              // Commit more frequently
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Arc::new(TestSnapClient::new());
    let config = create_custom_config();
    let provider = reth_provider::test_utils::MockProvider::default();
    
    let mut downloader = SnapSyncDownloader::new(client, provider, config);
    
    // Use the custom configuration
    let state_root = B256::from([1u8; 32]);
    downloader.start_account_range_download(state_root).await?;
    
    // Process results...
    
    Ok(())
}
```

### 3. Task-Based Downloading

```rust
use reth_network_downloaders::snap::task::TaskSnapDownloader;
use reth_eth_wire_types::snap::{GetAccountRangeMessage, GetStorageRangesMessage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Arc::new(TestSnapClient::new());
    let mut task_downloader = TaskSnapDownloader::new(client);
    
    // Download account range
    let account_result = task_downloader.download_account_range(
        B256::from([1u8; 32]), // root_hash
        B256::from([2u8; 32]), // starting_hash
        B256::from([3u8; 32]), // limit_hash
        1024 * 1024,           // response_bytes (1MB)
    ).await?;
    
    println!("Downloaded {} accounts", account_result.accounts.len());
    
    // Download storage ranges
    let storage_result = task_downloader.download_storage_ranges(
        B256::from([1u8; 32]), // root_hash
        vec![B256::from([4u8; 32])], // account_hashes
        B256::from([5u8; 32]), // starting_hash
        B256::from([6u8; 32]), // limit_hash
        1024 * 1024,           // response_bytes
    ).await?;
    
    println!("Downloaded storage for {} accounts", storage_result.slots.len());
    
    Ok(())
}
```

## Pipeline Integration Examples

### 1. Basic Pipeline Setup

```rust
use reth_stages::sets::DefaultStages;
use reth_stages::Pipeline;
use reth_config::config::StageConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create provider factory
    let provider_factory = reth_provider::test_utils::create_test_provider_factory();
    
    // Create consensus
    let consensus = Arc::new(reth_consensus::test_utils::TestConsensus::default());
    
    // Create tip channel
    let (tip_tx, tip_rx) = tokio::sync::watch::channel(B256::default());
    
    // Create downloaders
    let headers_downloader = reth_network_p2p::test_utils::TestHeadersClient::default();
    let bodies_downloader = reth_network_p2p::test_utils::TestBodiesClient::default();
    let snap_client = Arc::new(TestSnapClient::new());
    
    // Create EVM config
    let evm_config = reth_evm_ethereum::EthEvmConfig::mainnet();
    
    // Create stage config
    let stage_config = StageConfig::default();
    
    // Create prune modes
    let prune_modes = reth_prune_types::PruneModes::none();
    
    // Create static file producer
    let static_file_producer = reth_static_file::StaticFileProducer::new(
        provider_factory.clone(),
        prune_modes.clone(),
    );
    
    // Create pipeline with snap sync
    let pipeline = Pipeline::builder()
        .with_tip_sender(tip_tx)
        .add_stages(DefaultStages::new(
            provider_factory.database_provider_ro().unwrap(),
            tip_rx,
            consensus,
            headers_downloader,
            bodies_downloader,
            snap_client,  // Snap client parameter
            evm_config,
            stage_config,
            prune_modes,
            None, // era_import_source
        ))
        .build(provider_factory, static_file_producer)?;
    
    println!("Pipeline created successfully with snap sync support");
    
    Ok(())
}
```

### 2. Custom Stage Configuration

```rust
use reth_config::config::{StageConfig, SnapSyncConfig};

fn create_custom_stage_config() -> StageConfig {
    let mut config = StageConfig::default();
    
    // Customize snap sync configuration
    config.snap_sync = SnapSyncConfig {
        max_concurrent_requests: 15,
        max_response_bytes: 3 * 1024 * 1024, // 3MB
        max_accounts_per_request: 1500,
        max_storage_slots_per_request: 1500,
        max_byte_codes_per_request: 150,
        max_trie_nodes_per_request: 1500,
        commit_threshold: 15000,
    };
    
    config
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stage_config = create_custom_stage_config();
    
    // Use the custom configuration in pipeline setup
    // ... (rest of pipeline setup)
    
    Ok(())
}
```

## Error Handling Examples

### 1. Basic Error Handling

```rust
use reth_network_downloaders::snap::downloader::SnapSyncError;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Arc::new(TestSnapClient::new());
    let config = SnapSyncConfig::default();
    let provider = reth_provider::test_utils::MockProvider::default();
    
    let mut downloader = SnapSyncDownloader::new(client, provider, config);
    
    match downloader.start_account_range_download(B256::default()).await {
        Ok(()) => {
            println!("Download started successfully");
        }
        Err(SnapSyncError::Network(network_error)) => {
            eprintln!("Network error: {}", network_error);
            // Handle network issues (retry, backoff, etc.)
        }
        Err(SnapSyncError::InvalidStateRoot(root)) => {
            eprintln!("Invalid state root: {:?}", root);
            // Handle invalid state root
        }
        Err(e) => {
            eprintln!("Other error: {}", e);
        }
    }
    
    Ok(())
}
```

### 2. Retry Logic

```rust
use tokio::time::{sleep, Duration};

async fn download_with_retry(
    downloader: &mut SnapSyncDownloader<TestSnapClient, MockProvider>,
    state_root: B256,
    max_retries: u32,
) -> Result<(), SnapSyncError> {
    let mut retries = 0;
    
    loop {
        match downloader.start_account_range_download(state_root).await {
            Ok(()) => return Ok(()),
            Err(SnapSyncError::Network(_)) if retries < max_retries => {
                retries += 1;
                let delay = Duration::from_millis(1000 * retries); // Exponential backoff
                println!("Retrying in {:?} (attempt {}/{})", delay, retries, max_retries);
                sleep(delay).await;
            }
            Err(e) => return Err(e),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Arc::new(TestSnapClient::new());
    let config = SnapSyncConfig::default();
    let provider = reth_provider::test_utils::MockProvider::default();
    
    let mut downloader = SnapSyncDownloader::new(client, provider, config);
    
    download_with_retry(&mut downloader, B256::default(), 3).await?;
    
    Ok(())
}
```

## Testing Examples

### 1. Unit Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use reth_network_downloaders::snap::test_utils::{TestSnapClient, create_test_snap_sync_config};
    use reth_eth_wire_types::snap::{AccountData, AccountRangeMessage};
    use alloy_primitives::B256;
    use alloy_primitives::Bytes;

    #[tokio::test]
    async fn test_account_range_download() {
        // Create test data
        let test_accounts = vec![
            AccountData {
                hash: B256::from([1u8; 32]),
                body: Bytes::from(vec![0x01, 0x02, 0x03]),
            },
            AccountData {
                hash: B256::from([2u8; 32]),
                body: Bytes::from(vec![0x04, 0x05, 0x06]),
            },
        ];
        
        // Create test client with predefined responses
        let client = Arc::new(
            TestSnapClient::new().add_account_range_response(AccountRangeMessage {
                request_id: 1,
                accounts: test_accounts.clone(),
                proof: vec![Bytes::from(vec![0x07, 0x08, 0x09])],
            })
        );
        
        // Create configuration
        let config = create_test_snap_sync_config();
        
        // Create provider
        let provider = reth_provider::test_utils::MockProvider::default();
        
        // Create downloader
        let mut downloader = SnapSyncDownloader::new(client, provider, config);
        
        // Test download
        let state_root = B256::from([3u8; 32]);
        downloader.start_account_range_download(state_root).await.unwrap();
        
        // Verify request queue has pending requests
        assert!(downloader.request_queue.has_pending_requests());
    }
    
    #[test]
    fn test_configuration_validation() {
        let config = SnapSyncConfig::default();
        
        assert_eq!(config.max_concurrent_requests, 10);
        assert_eq!(config.max_response_bytes, 2 * 1024 * 1024);
        assert_eq!(config.max_accounts_per_request, 1000);
    }
}
```

### 2. Integration Testing

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;
    use reth_stages::test_utils::{TestStageDB, StorageKind};

    #[tokio::test]
    async fn test_snap_sync_stage_integration() {
        // Create test database
        let test_db = TestStageDB::default();
        let provider = test_db.factory.provider_rw().unwrap();
        
        // Create test client
        let client = Arc::new(TestSnapClient::new());
        
        // Create configuration
        let config = SnapSyncConfig::default();
        let etl_config = EtlConfig::default();
        
        // Create stage
        let mut stage = SnapSyncStage::new(provider.clone(), client, config, etl_config);
        
        // Test stage execution
        let input = ExecInput {
            target: Some(100),
            checkpoint: None,
        };
        
        let result = stage.execute(&provider, input);
        assert!(result.is_ok());
        
        let output = result.unwrap();
        assert!(output.done);
    }
}
```

## Performance Monitoring Examples

### 1. Basic Metrics

```rust
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Arc::new(TestSnapClient::new());
    let config = SnapSyncConfig::default();
    let provider = reth_provider::test_utils::MockProvider::default();
    
    let mut downloader = SnapSyncDownloader::new(client, provider, config);
    
    let start_time = Instant::now();
    let state_root = B256::from([1u8; 32]);
    
    downloader.start_account_range_download(state_root).await?;
    
    let mut stream = Box::pin(downloader);
    let mut account_count = 0;
    let mut total_bytes = 0;
    
    while let Some(result) = stream.next().await {
        match result? {
            SnapSyncResult::AccountRange(accounts) => {
                account_count += accounts.accounts.len();
                for account in accounts.accounts {
                    total_bytes += account.body.len();
                }
            }
            _ => {}
        }
    }
    
    let duration = start_time.elapsed();
    println!("Downloaded {} accounts ({} bytes) in {:?}", account_count, total_bytes, duration);
    println!("Rate: {:.2} accounts/sec", account_count as f64 / duration.as_secs_f64());
    
    Ok(())
}
```

### 2. Memory Usage Monitoring

```rust
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct MemoryTracker;

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for MemoryTracker {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ret = System.alloc(layout);
        if !ret.is_null() {
            ALLOCATED.fetch_add(layout.size(), Ordering::SeqCst);
        }
        ret
    }
    
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        ALLOCATED.fetch_sub(layout.size(), Ordering::SeqCst);
        System.dealloc(ptr, layout);
    }
}

#[global_allocator]
static GLOBAL: MemoryTracker = MemoryTracker;

fn print_memory_usage() {
    let allocated = ALLOCATED.load(Ordering::SeqCst);
    println!("Memory allocated: {} bytes ({:.2} MB)", allocated, allocated as f64 / 1024.0 / 1024.0);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    print_memory_usage();
    
    let client = Arc::new(TestSnapClient::new());
    let config = SnapSyncConfig::default();
    let provider = reth_provider::test_utils::MockProvider::default();
    
    let mut downloader = SnapSyncDownloader::new(client, provider, config);
    
    print_memory_usage();
    
    // Perform downloads...
    
    print_memory_usage();
    
    Ok(())
}
```

## Configuration File Examples

### 1. Basic Configuration

```toml
# reth.toml
[stages.snap_sync]
max_concurrent_requests = 10
max_response_bytes = 2097152
max_accounts_per_request = 1000
max_storage_slots_per_request = 1000
max_byte_codes_per_request = 100
max_trie_nodes_per_request = 1000
commit_threshold = 10000
```

### 2. High-Performance Configuration

```toml
# reth.toml - For high-performance nodes
[stages.snap_sync]
max_concurrent_requests = 50
max_response_bytes = 8388608  # 8MB
max_accounts_per_request = 5000
max_storage_slots_per_request = 5000
max_byte_codes_per_request = 500
max_trie_nodes_per_request = 5000
commit_threshold = 50000
```

### 3. Low-Resource Configuration

```toml
# reth.toml - For resource-constrained nodes
[stages.snap_sync]
max_concurrent_requests = 5
max_response_bytes = 1048576   # 1MB
max_accounts_per_request = 500
max_storage_slots_per_request = 500
max_byte_codes_per_request = 50
max_trie_nodes_per_request = 500
commit_threshold = 5000
```

## Best Practices

1. **Start with default configuration** and tune based on your hardware and network
2. **Monitor memory usage** with large datasets
3. **Handle errors gracefully** with appropriate retry logic
4. **Use appropriate batch sizes** for your use case
5. **Test thoroughly** with mock clients before production use
6. **Monitor performance metrics** to optimize configuration
7. **Use static file integration** for large datasets
8. **Implement proper logging** for debugging and monitoring