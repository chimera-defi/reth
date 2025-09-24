# Snap Sync Implementation

This module implements snap sync functionality for Reth, enabling faster state synchronization by downloading state snapshots from peers instead of computing the state from block execution.

## Overview

Snap sync (State Network Access Protocol) is an Ethereum protocol that allows nodes to download the current state of the blockchain more efficiently. Instead of executing every transaction to compute the state, nodes can request state snapshots directly from peers.

## Features

- **Account Range Requests**: Download account data in batches
- **Storage Range Requests**: Download storage slots for specific accounts
- **Bytecode Requests**: Download contract bytecode by hash
- **Trie Node Requests**: Download Merkle proof nodes for verification
- **Pipeline Integration**: Integrated with Reth's stage-based pipeline system
- **Configurable**: Extensive configuration options for tuning performance

## Architecture

### Core Components

1. **SnapSyncClient**: Trait defining the interface for snap sync operations
2. **SnapSyncDownloader**: Main coordinator for state download operations
3. **SnapSyncStage**: Pipeline stage that integrates snap sync into the sync process
4. **Example Implementation**: Complete example showing how to use the API

### Protocol Messages

The implementation supports all SNAP/1 protocol messages:

- `GetAccountRange` / `AccountRange`: Account state requests and responses
- `GetStorageRanges` / `StorageRanges`: Storage slot requests and responses
- `GetByteCodes` / `ByteCodes`: Contract bytecode requests and responses
- `GetTrieNodes` / `TrieNodes`: Merkle proof requests and responses

## Usage

### Basic Usage

```rust
use reth_network::downloaders::snap::{
    ExampleSnapSyncClient, SnapSyncConfig, SnapSyncDownloader,
};
use std::sync::Arc;

// Create a snap sync client
let client = Arc::new(ExampleSnapSyncClient::new());

// Configure snap sync
let config = SnapSyncConfig {
    max_concurrent_requests: 10,
    max_response_size: 10_000_000, // 10MB
    account_batch_size: 1000,
    storage_batch_size: 1000,
    bytecode_batch_size: 100,
};

// Create downloader
let downloader = SnapSyncDownloader::new(client, config);

// Download state for a specific block
let block_hash = B256::ZERO;
let stats = downloader.download_state(block_hash).await?;
```

### CLI Integration

Snap sync can be enabled via CLI:

```bash
# Enable snap sync
reth node --snap-sync

# With custom configuration (if supported)
reth node --snap-sync --max-response-size 50000000
```

### Pipeline Integration

The snap sync stage can be integrated into Reth's pipeline:

```rust
use reth_stages::sets::{OnlineStages};
use reth_stages::stages::{SnapSyncStage};

// Create online stages with snap sync
let online_stages = OnlineStages::new_with_snap_sync(
    provider,
    tip_receiver,
    header_downloader,
    body_downloader,
    snap_downloader,
    stage_config,
    era_import_source,
);
```

## Configuration

### SnapSyncConfig

- `max_concurrent_requests`: Maximum number of concurrent requests (default: 10)
- `max_response_size`: Maximum response size in bytes (default: 10MB)
- `account_batch_size`: Number of accounts per request (default: 1000)
- `storage_batch_size`: Number of storage slots per request (default: 1000)
- `bytecode_batch_size`: Number of bytecode hashes per request (default: 100)

## Implementation Details

### State Download Process

1. **Account Discovery**: Request account ranges starting from root hash
2. **Storage Download**: For each account, download associated storage slots
3. **Bytecode Download**: Download bytecode for contract accounts
4. **Verification**: Verify downloaded data against Merkle proofs
5. **Storage**: Write downloaded state to database

### Error Handling

The implementation includes comprehensive error handling for:
- Network errors and timeouts
- Invalid responses from peers
- Corrupted or incomplete data
- Storage write failures

### Performance Optimizations

- **Concurrent Requests**: Multiple requests can be in flight simultaneously
- **Batch Processing**: Large ranges are broken into manageable batches
- **Priority Queuing**: Critical requests can be prioritized
- **Memory Management**: Efficient buffering and streaming of large responses

## Testing

Run the example implementation:

```bash
cargo test -p reth-network --lib downloaders::snap::example::tests
```

## Future Enhancements

- **Parallel State Download**: Download multiple state ranges simultaneously
- **Adaptive Configuration**: Automatically adjust configuration based on network conditions
- **Snapshot Restoration**: Support for importing pre-computed snapshots
- **Advanced Verification**: Enhanced Merkle proof verification
- **Compression Support**: Support for compressed state data

## References

- [Ethereum SNAP Protocol Specification](https://github.com/ethereum/devp2p/blob/master/caps/snap.md)
- [Reth Network Architecture](https://github.com/paradigmxyz/reth)
- [Ethereum State Sync](https://ethereum.org/en/developers/docs/nodes-and-clients/#sync-modes)