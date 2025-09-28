# SnapSyncStage Implementation Plan and Progress

## Overview
This document outlines the implementation plan for the new `SnapSyncStage` that will be responsible for querying peers for ranges of trie data and inserting it into the database. When enabled, this stage will replace `SenderRecoveryStage`, `ExecutionStage`, and `PruneSenderRecoveryStage`.

## Architecture

### Core Algorithm
The stage follows this algorithm:
1. **Retrieve the latest header from the engine** - Get current state root
2. **Check if hashed state in `tables::HashedAccounts` is empty**:
   - If empty: start downloading from `0x0000...` account hash
   - If not empty: start from last entry in database
3. **Paginate over trie ranges using `GetAccountRange` requests**
4. **Repeat until state is retrieved** (final range until `0xffff...` is fetched)

### Key Components

#### 1. SnapSyncStage Structure
- **Configuration**: `SnapSyncConfig` with settings for max ranges, response bytes, enable/disable
- **Snap Client**: `Arc<C>` where `C: SnapClient` for peer communication
- **Header Stream**: `S: Stream<Item = B256>` for consensus engine integration
- **State Tracking**: Current target state root, starting hash, request ID counter

#### 2. Integration Points
- **Consensus Engine**: Subscribes to header stream to update target state root
- **Database**: Reads from and writes to `tables::HashedAccounts`
- **Network**: Uses `SnapClient` trait for peer communication
- **Pipeline**: Implements `Stage<Provider>` trait

#### 3. Data Flow
```
Consensus Engine → Header Stream → Target State Root Update
Database → Check Hashed State → Determine Starting Point
Snap Client → GetAccountRange → Process Response → Insert to DB
```

## Implementation Progress

### ✅ Completed Tasks

1. **Research and Analysis**
   - [x] Analyzed existing stages (`SenderRecoveryStage`, `ExecutionStage`, `PruneSenderRecoveryStage`)
   - [x] Studied snap sync protocol and `GetAccountRange` message structure
   - [x] Examined consensus engine integration patterns
   - [x] Researched `tables::HashedAccounts` usage in codebase

2. **Stage ID and Module Setup**
   - [x] Added `SnapSync` to `StageId` enum
   - [x] Updated `ALL` and `STATE_REQUIRED` arrays
   - [x] Added string representation for `SnapSync`
   - [x] Created `snap_sync.rs` module
   - [x] Added module exports to `mod.rs`

3. **Core Structure Implementation**
   - [x] Defined `SnapSyncConfig` with configuration options
   - [x] Implemented `SnapSyncStage` struct with required fields
   - [x] Created constructor and basic methods
   - [x] Implemented `Stage` trait with `id()`, `execute()`, `unwind()` methods

4. **Database Integration**
   - [x] Implemented `is_hashed_state_empty()` method
   - [x] Implemented `get_last_hashed_account()` for continuation
   - [x] Created `process_account_range()` for data insertion
   - [x] Added proper database cursor usage

5. **Snap Protocol Integration**
   - [x] Implemented `create_account_range_request()` method
   - [x] Added proper `GetAccountRangeMessage` construction
   - [x] Integrated with `SnapClient` trait
   - [x] Added response handling for `AccountRangeMessage`

6. **Testing Infrastructure**
   - [x] Created `MockSnapClient` for testing
   - [x] Implemented `DownloadClient` and `SnapClient` traits for mock
   - [x] Added basic test cases for stage creation and disabled state

### 🚧 In Progress Tasks

1. **Header Stream Integration**
   - [ ] Implement proper header stream subscription
   - [ ] Extract state root from headers
   - [ ] Handle header stream errors and reconnections

2. **Async Execution Handling**
   - [ ] Resolve async execution in sync context
   - [ ] Implement proper async/sync bridge
   - [ ] Add proper error handling for async operations

### ⏳ Pending Tasks

1. **Range Downloading Implementation**
   - [ ] Complete `download_account_ranges()` method
   - [ ] Implement proper pagination logic
   - [ ] Add range validation and error handling
   - [ ] Implement storage range downloading (future)

2. **State Tracking and Continuation**
   - [ ] Implement proper state root tracking
   - [ ] Add range continuation logic
   - [ ] Handle state root updates during execution
   - [ ] Add progress persistence

3. **Healing Algorithm Placeholders**
   - [ ] Create placeholder for trie healing
   - [ ] Add proof verification placeholders
   - [ ] Implement basic validation

4. **Database Insert Optimization**
   - [ ] Optimize batch inserts
   - [ ] Add transaction management
   - [ ] Implement proper error recovery

5. **Integration and Testing**
   - [ ] Add comprehensive test cases
   - [ ] Test with real snap client
   - [ ] Integration tests with pipeline
   - [ ] Performance testing

6. **Configuration and Documentation**
   - [ ] Add configuration options
   - [ ] Create usage documentation
   - [ ] Add logging and metrics
   - [ ] Update stage replacement logic

## Technical Challenges

### 1. Async/Sync Bridge
The main challenge is handling async snap client operations within the synchronous stage execution context. Current approach uses placeholders, but proper solution needed.

### 2. Header Stream Integration
Need to properly integrate with consensus engine header stream to get state root updates.

### 3. State Root Management
Complex logic for tracking and updating target state root during execution.

### 4. Error Handling
Robust error handling for network failures, invalid responses, and database errors.

## Next Steps

1. **Immediate (Next Session)**
   - Fix async execution handling
   - Implement proper header stream integration
   - Complete range downloading logic

2. **Short Term**
   - Add comprehensive testing
   - Implement healing algorithm placeholders
   - Optimize database operations

3. **Medium Term**
   - Add storage range support
   - Implement proper state management
   - Add metrics and monitoring

4. **Long Term**
   - Performance optimization
   - Advanced error recovery
   - Integration with full pipeline

## Code Quality Notes

- Follows existing reth patterns and conventions
- Uses proper error handling with `StageError`
- Implements required traits correctly
- Includes comprehensive documentation
- Maintains separation of concerns

## Dependencies

- `reth_eth_wire_types::snap` - Snap protocol message types
- `reth_net_p2p::snap` - Snap client trait
- `reth_db_api::tables` - Database table definitions
- `reth_stages_api` - Stage trait definitions
- `alloy_primitives` - Primitive types
- `futures` - Async utilities