# Snap Sync Implementation for Reth

## 🎯 **Quick Start**

This directory contains the snap sync implementation for Reth, providing fast state synchronization for Ethereum nodes.

### **Current Status**
- **Progress**: 50% Complete
- **State Root Discovery**: ✅ **COMPLETED**
- **State Verification**: 🔄 **IN PROGRESS**
- **State Healing**: ⏳ **PENDING**

### **Key Files**
- **Main Documentation**: [`docs/snap-sync.md`](docs/snap-sync.md)
- **State Discovery**: `crates/net/downloaders/src/snap/state_discovery.rs`
- **State Verification**: `crates/net/downloaders/src/snap/state_verifier.rs` (coming soon)
- **State Healing**: `crates/net/downloaders/src/snap/state_healer.rs` (coming soon)

### **Usage**
```bash
# Start Reth with snap sync
reth --sync-mode snap

# Start Reth with specific snap sync configuration
reth --sync-mode snap --snap-sync.max-peers 10
```

### **Development**
```bash
# Run snap sync tests
cargo test --package reth-net-downloaders snap

# Run specific test
cargo test --package reth-net-downloaders snap::state_discovery_tests
```

## 📚 **Documentation**

- **[Complete Documentation](docs/snap-sync.md)** - Comprehensive guide to snap sync implementation
- **[API Reference](docs/snap-sync-api.md)** - API documentation (coming soon)
- **[Configuration Guide](docs/snap-sync-configuration.md)** - Configuration options (coming soon)

## 🚀 **Next Steps**

1. **State Verification System** - Implement Merkle proof verification
2. **State Healing System** - Implement missing data recovery
3. **Two-Phase Sync Flow** - Integrate state download with forward sync
4. **Comprehensive Testing** - End-to-end testing with real network data

## 🤝 **Contributing**

Please read the main documentation in [`docs/snap-sync.md`](docs/snap-sync.md) before contributing to the snap sync implementation.

## 📄 **License**

This project is part of Reth and follows the same license terms.