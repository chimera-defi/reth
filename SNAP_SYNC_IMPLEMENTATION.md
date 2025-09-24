# Snap Sync Implementation

This document describes the snap sync implementation added to Reth as requested in issue #17177.

## Overview

Snap sync is an alternative synchronization method that downloads Ethereum state data directly using the snap protocol, rather than downloading and executing all historical transactions. This can significantly speed up initial node synchronization.

## Implementation Status

The current implementation provides the basic infrastructure for snap sync:

### ✅ Completed
- **SnapSyncStage**: A new stage that implements the `Stage` trait for snap sync functionality
- **StageId::SnapSync**: Added snap sync to the stage ID enumeration
- **SnapSyncStages**: A stage set that uses snap sync instead of traditional header/body sync
- **Basic test coverage**: Unit tests to ensure the stage can be created and integrated
- **Documentation**: Updated documentation to include information about snap sync

### 🚧 Stub Implementation
The current implementation is a stub that demonstrates the structure but does not perform actual snap sync operations. The `execute` method currently logs a message and returns success without downloading any data.

### 📋 Future Work Needed
To make this a complete snap sync implementation, the following would need to be implemented:

1. **Actual Snap Protocol Communication**
   - Implement proper async snap sync logic using the existing `SnapClient`
   - Handle account range requests and responses
   - Download storage ranges, bytecodes, and trie nodes
   - Implement request batching and concurrency control

2. **State Reconstruction**
   - Reconstruct the state trie from downloaded data
   - Verify state consistency and integrity
   - Handle partial downloads and resume capability

3. **Error Handling and Resilience**
   - Implement proper error handling for network failures
   - Add retry logic for failed requests
   - Handle malicious or incorrect responses from peers

4. **Integration with Existing Systems**
   - Integrate with the existing database layer
   - Ensure compatibility with other stages in the pipeline
   - Add proper metrics and monitoring

## Usage

To use the snap sync stage (once fully implemented), you would replace the default stages with the snap sync stage set:

```rust
use reth_stages::sets::SnapSyncStages;

// Instead of DefaultStages, use SnapSyncStages
let pipeline = Pipeline::builder()
    .add_stages(SnapSyncStages::new(
        provider_factory.clone(),
        snap_client,
        tip_rx,
        snap_config,
        evm_config,
        consensus,
        stage_config,
        prune_modes,
    ))
    .build(provider_factory, static_file_producer);
```

## Files Modified

- `crates/stages/types/src/id.rs` - Added `StageId::SnapSync`
- `crates/stages/stages/src/stages/snap_sync.rs` - New snap sync stage implementation
- `crates/stages/stages/src/stages/mod.rs` - Export snap sync stage
- `crates/stages/stages/src/sets.rs` - Added `SnapSyncStages` stage set
- `crates/stages/stages/Cargo.toml` - Added required dependencies
- `docs/crates/stages.md` - Updated documentation

## Testing

Run the snap sync tests with:
```bash
cargo test -p reth-stages snap_sync
```

## Architecture

The snap sync implementation follows the existing Reth stage architecture:

```
SnapSyncStage
├── SnapSyncConfig - Configuration for batch sizes, timeouts, etc.
├── SnapSyncProgress - Tracks sync progress and metrics
└── Stage trait implementation
    ├── execute() - Main sync logic (currently stub)
    └── unwind() - Cleanup on reorg/rollback
```

The `SnapSyncStages` set provides a complete pipeline alternative:
- `SnapSyncStage` - Downloads state using snap protocol
- `FinishStage` - Final cleanup

## Contributing

To continue development of this feature:

1. Implement the actual snap sync logic in `SnapSyncStage::execute()`
2. Add comprehensive integration tests
3. Implement proper error handling and retry logic
4. Add metrics and monitoring
5. Performance testing and optimization

## References

- [Ethereum Snap Sync Protocol](https://github.com/ethereum/devp2p/blob/master/caps/snap.md)
- [EIP-2481: eth/66 protocol version](https://eips.ethereum.org/EIPS/eip-2481)
- Original issue: #17177 (if it exists in the repository)