# Snap Sync API Documentation

## Overview

This document provides comprehensive API documentation for the snap sync implementation in Reth. Snap sync allows Ethereum nodes to quickly synchronize state without processing every individual block.

## Core Components

### 1. SnapSyncDownloader

The main downloader for snap sync operations.

```rust
pub struct SnapSyncDownloader<C: SnapClient, Provider: HeaderProvider> {
    // Private fields
}
```

#### Methods

##### `new`
```rust
pub fn new(
    client: Arc<C>, 
    provider: Provider, 
    config: SnapSyncConfig
) -> Self
```

Creates a new snap sync downloader.

**Parameters:**
- `client`: Snap client for making network requests
- `provider`: Database provider for reading headers
- `config`: Configuration for the downloader

**Returns:** New `SnapSyncDownloader` instance

##### `start_account_range_download`
```rust
pub async fn start_account_range_download(
    &mut self,
    state_root: B256,
) -> Result<(), SnapSyncError>
```

Starts downloading account ranges for the given state root.

**Parameters:**
- `state_root`: Root hash of the state trie to download

**Returns:** `Result<(), SnapSyncError>`

##### `get_current_state_root`
```rust
pub fn get_current_state_root(&self) -> Result<B256, SnapSyncError>
```

Gets the current state root from the latest header.

**Returns:** `Result<B256, SnapSyncError>`

#### Stream Implementation

The downloader implements `Stream<Item = Result<SnapSyncResult, SnapSyncError>>`:

```rust
impl<C: SnapClient, Provider: HeaderProvider> Stream for SnapSyncDownloader<C, Provider> {
    type Item = Result<SnapSyncResult, SnapSyncError>;
    
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>>;
}
```

### 2. TaskSnapDownloader

Task-based downloader for individual snap sync requests.

```rust
pub struct TaskSnapDownloader<C: SnapClient> {
    // Private fields
}
```

#### Methods

##### `new`
```rust
pub fn new(client: Arc<C>) -> Self
```

Creates a new task-based snap downloader.

##### `download_account_range`
```rust
pub async fn download_account_range(
    &mut self,
    root_hash: B256,
    starting_hash: B256,
    limit_hash: B256,
    response_bytes: u64,
) -> PeerRequestResult<AccountRangeMessage>
```

Downloads account range data.

**Parameters:**
- `root_hash`: Root hash of the account trie
- `starting_hash`: Starting account hash
- `limit_hash`: Limit account hash
- `response_bytes`: Maximum response size

**Returns:** `PeerRequestResult<AccountRangeMessage>`

##### `download_storage_ranges`
```rust
pub async fn download_storage_ranges(
    &mut self,
    root_hash: B256,
    account_hashes: Vec<B256>,
    starting_hash: B256,
    limit_hash: B256,
    response_bytes: u64,
) -> PeerRequestResult<StorageRangesMessage>
```

Downloads storage range data.

##### `download_byte_codes`
```rust
pub async fn download_byte_codes(
    &mut self,
    hashes: Vec<B256>,
    response_bytes: u64,
) -> PeerRequestResult<ByteCodesMessage>
```

Downloads byte code data.

##### `download_trie_nodes`
```rust
pub async fn download_trie_nodes(
    &mut self,
    root_hash: B256,
    paths: Vec<TriePath>,
    response_bytes: u64,
) -> PeerRequestResult<TrieNodesMessage>
```

Downloads trie node data.

### 3. SnapRequestQueue

Request queue for managing snap sync requests.

```rust
pub struct SnapRequestQueue {
    // Private fields
}
```

#### Methods

##### `new`
```rust
pub fn new() -> Self
```

Creates a new empty request queue.

##### `next_request_id`
```rust
pub fn next_request_id(&mut self) -> u64
```

Gets the next request ID and increments the counter.

##### `push_account_range`
```rust
pub fn push_account_range(&mut self, request: GetAccountRangeMessage)
```

Adds an account range request to the queue.

##### `pop_account_range`
```rust
pub fn pop_account_range(&mut self) -> Option<GetAccountRangeMessage>
```

Pops the next account range request.

##### `has_pending_requests`
```rust
pub fn has_pending_requests(&self) -> bool
```

Checks if there are any pending requests.

##### `pending_count`
```rust
pub fn pending_count(&self) -> usize
```

Gets the total number of pending requests.

### 4. SnapSyncStage

Pipeline stage for snap sync operations.

```rust
pub struct SnapSyncStage<Provider, Client: SnapClient> {
    // Private fields
}
```

#### Methods

##### `new`
```rust
pub fn new(
    provider: Provider,
    client: Arc<Client>,
    config: SnapSyncConfig,
    etl_config: EtlConfig,
) -> Self
```

Creates a new snap sync stage.

**Parameters:**
- `provider`: Database provider
- `client`: Snap client
- `config`: Snap sync configuration
- `etl_config`: ETL configuration

#### Stage Implementation

The stage implements the `Stage<Provider>` trait:

```rust
impl<Provider, Client> Stage<Provider> for SnapSyncStage<Provider, Client> {
    fn id(&self) -> StageId { StageId::SnapSync }
    
    fn execute(&mut self, provider: &Provider, input: ExecInput) -> Result<ExecOutput, StageError>;
    
    fn unwind(&mut self, provider: &Provider, input: UnwindInput) -> Result<UnwindOutput, StageError>;
}
```

## Configuration

### SnapSyncConfig

Configuration for snap sync operations.

```rust
pub struct SnapSyncConfig {
    pub max_concurrent_requests: usize,
    pub max_response_bytes: u64,
    pub max_accounts_per_request: u64,
    pub max_storage_slots_per_request: u64,
    pub max_byte_codes_per_request: u64,
    pub max_trie_nodes_per_request: u64,
    pub commit_threshold: u64,
}
```

#### Default Values

```rust
impl Default for SnapSyncConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: 10,
            max_response_bytes: 2 * 1024 * 1024, // 2MB
            max_accounts_per_request: 1000,
            max_storage_slots_per_request: 1000,
            max_byte_codes_per_request: 100,
            max_trie_nodes_per_request: 1000,
            commit_threshold: 10_000,
        }
    }
}
```

#### Configuration File

```toml
[stages.snap_sync]
max_concurrent_requests = 10
max_response_bytes = 2097152
max_accounts_per_request = 1000
max_storage_slots_per_request = 1000
max_byte_codes_per_request = 100
max_trie_nodes_per_request = 1000
commit_threshold = 10000
```

## Data Types

### SnapSyncResult

Result of a snap sync download operation.

```rust
pub enum SnapSyncResult {
    AccountRange(AccountRangeMessage),
    StorageRanges(StorageRangesMessage),
    ByteCodes(ByteCodesMessage),
    TrieNodes(TrieNodesMessage),
}
```

### SnapSyncError

Error type for snap sync operations.

```rust
pub enum SnapSyncError {
    Network(PeerRequestError),
    InvalidStateRoot(B256),
    MissingAccountData(B256),
    MissingStorageData(B256),
    InvalidProof,
}
```

## Network Integration

### FetchClient SnapClient Implementation

The `FetchClient` implements the `SnapClient` trait:

```rust
impl<N: NetworkPrimitives> SnapClient for FetchClient<N> {
    type Output = HeadersClientFuture<PeerRequestResult<AccountRangeMessage>>;
    
    fn get_account_range_with_priority(
        &self,
        request: GetAccountRangeMessage,
        priority: Priority,
    ) -> Self::Output;
    
    fn get_storage_ranges_with_priority(
        &self,
        request: GetStorageRangesMessage,
        priority: Priority,
    ) -> Pin<Box<dyn Future<Output = PeerRequestResult<StorageRangesMessage>> + Send + Sync>>;
    
    fn get_byte_codes_with_priority(
        &self,
        request: GetByteCodesMessage,
        priority: Priority,
    ) -> Pin<Box<dyn Future<Output = PeerRequestResult<ByteCodesMessage>> + Send + Sync>>;
    
    fn get_trie_nodes_with_priority(
        &self,
        request: GetTrieNodesMessage,
        priority: Priority,
    ) -> Pin<Box<dyn Future<Output = PeerRequestResult<TrieNodesMessage>> + Send + Sync>>;
}
```

## Usage Examples

### Basic Usage

```rust
use reth_network_downloaders::snap::SnapSyncDownloader;
use reth_config::config::SnapSyncConfig;

// Create configuration
let config = SnapSyncConfig::default();

// Create downloader
let mut downloader = SnapSyncDownloader::new(snap_client, provider, config);

// Start downloading
downloader.start_account_range_download(state_root).await?;

// Process results
let mut stream = Box::pin(downloader);
while let Some(result) = stream.next().await {
    match result? {
        SnapSyncResult::AccountRange(accounts) => {
            // Process account data
            for account in accounts.accounts {
                println!("Account: {:?}", account.hash);
            }
        }
        SnapSyncResult::StorageRanges(storage) => {
            // Process storage data
            for account_slots in storage.slots {
                for slot in account_slots {
                    println!("Storage: {:?}", slot.hash);
                }
            }
        }
        SnapSyncResult::ByteCodes(codes) => {
            // Process byte codes
            for code in codes.codes {
                println!("Byte code length: {}", code.len());
            }
        }
        SnapSyncResult::TrieNodes(nodes) => {
            // Process trie nodes
            for node in nodes.nodes {
                println!("Trie node length: {}", node.len());
            }
        }
    }
}
```

### Pipeline Integration

```rust
use reth_stages::sets::DefaultStages;
use reth_stages::Pipeline;

// Create pipeline with snap sync
let pipeline = Pipeline::builder()
    .add_stages(DefaultStages::new(
        provider,
        tip_rx,
        consensus,
        headers_downloader,
        bodies_downloader,
        snap_client,  // Snap client parameter
        evm_config,
        stage_config,
        prune_modes,
        era_import_source,
    ))
    .build(provider_factory, static_file_producer);
```

### Custom Configuration

```rust
use reth_config::config::{StageConfig, SnapSyncConfig};

// Create custom configuration
let mut stage_config = StageConfig::default();
stage_config.snap_sync = SnapSyncConfig {
    max_concurrent_requests: 20,
    max_response_bytes: 4 * 1024 * 1024, // 4MB
    max_accounts_per_request: 2000,
    max_storage_slots_per_request: 2000,
    max_byte_codes_per_request: 200,
    max_trie_nodes_per_request: 2000,
    commit_threshold: 20000,
};
```

## Testing

### Test Utilities

```rust
use reth_network_downloaders::snap::test_utils::{TestSnapClient, create_test_snap_sync_config};

// Create test client
let test_client = TestSnapClient::new()
    .add_account_range_response(AccountRangeMessage {
        request_id: 1,
        accounts: vec![],
        proof: vec![],
    });

// Create test configuration
let config = create_test_snap_sync_config();

// Use in tests
let downloader = SnapSyncDownloader::new(Arc::new(test_client), provider, config);
```

### Mock Implementation

```rust
use reth_network_downloaders::snap::test_utils::TestSnapClient;

#[tokio::test]
async fn test_snap_sync() {
    let client = Arc::new(TestSnapClient::new());
    let config = SnapSyncConfig::default();
    let mut downloader = SnapSyncDownloader::new(client, provider, config);
    
    // Test downloader functionality
    downloader.start_account_range_download(B256::default()).await.unwrap();
}
```

## Error Handling

### Common Error Patterns

```rust
use reth_network_downloaders::snap::downloader::SnapSyncError;

match result {
    Ok(snap_result) => {
        // Process successful result
    }
    Err(SnapSyncError::Network(network_error)) => {
        // Handle network errors
        eprintln!("Network error: {}", network_error);
    }
    Err(SnapSyncError::InvalidStateRoot(root)) => {
        // Handle invalid state root
        eprintln!("Invalid state root: {:?}", root);
    }
    Err(SnapSyncError::MissingAccountData(hash)) => {
        // Handle missing account data
        eprintln!("Missing account data: {:?}", hash);
    }
    Err(SnapSyncError::InvalidProof) => {
        // Handle invalid proof
        eprintln!("Invalid proof received");
    }
}
```

## Performance Considerations

### Memory Usage
- ETL collectors handle large datasets efficiently
- Configurable batch sizes control memory usage
- Stream-based processing minimizes memory footprint

### Network Efficiency
- Configurable concurrent request limits
- Response size limits prevent memory exhaustion
- Request batching optimizes throughput

### Storage Performance
- Integration with static file system
- Efficient data serialization
- Checkpoint management for recovery

## Best Practices

1. **Configuration**: Use appropriate limits for your network and hardware
2. **Error Handling**: Always handle network errors gracefully
3. **Resource Management**: Monitor memory usage with large datasets
4. **Testing**: Use mock clients for unit testing
5. **Monitoring**: Track sync progress and performance metrics