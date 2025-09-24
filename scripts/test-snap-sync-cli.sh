#!/bin/bash

# Test script to verify snap sync CLI integration
# This script tests that the snap sync command line arguments work correctly

set -e

echo "🧪 Testing Snap Sync CLI Integration..."

# Test 1: Basic snap sync mode
echo "Test 1: Basic snap sync mode"
if cargo run --bin reth -- --sync-mode snap --help > /dev/null 2>&1; then
    echo "✅ Snap sync mode argument accepted"
else
    echo "❌ Snap sync mode argument failed"
    exit 1
fi

# Test 2: Deprecated snap sync flag
echo "Test 2: Deprecated snap sync flag"
if cargo run --bin reth -- --snap-sync --help > /dev/null 2>&1; then
    echo "✅ Deprecated snap sync flag accepted"
else
    echo "❌ Deprecated snap sync flag failed"
    exit 1
fi

# Test 3: Snap sync configuration parameters
echo "Test 3: Snap sync configuration parameters"
if cargo run --bin reth -- \
    --sync-mode snap \
    --snap-max-concurrent-requests 20 \
    --snap-max-response-bytes 4194304 \
    --snap-max-accounts-per-request 2000 \
    --snap-commit-threshold 20000 \
    --help > /dev/null 2>&1; then
    echo "✅ Snap sync configuration parameters accepted"
else
    echo "❌ Snap sync configuration parameters failed"
    exit 1
fi

# Test 4: Checkpoint sync mode
echo "Test 4: Checkpoint sync mode"
if cargo run --bin reth -- --sync-mode checkpoint --help > /dev/null 2>&1; then
    echo "✅ Checkpoint sync mode accepted"
else
    echo "❌ Checkpoint sync mode failed"
    exit 1
fi

# Test 5: Invalid sync mode should fail
echo "Test 5: Invalid sync mode should fail"
if cargo run --bin reth -- --sync-mode invalid --help > /dev/null 2>&1; then
    echo "❌ Invalid sync mode was accepted (should have failed)"
    exit 1
else
    echo "✅ Invalid sync mode correctly rejected"
fi

# Test 6: Help output should include sync options
echo "Test 6: Help output should include sync options"
if cargo run --bin reth -- --help | grep -q "sync-mode"; then
    echo "✅ Help output includes sync-mode option"
else
    echo "❌ Help output missing sync-mode option"
    exit 1
fi

if cargo run --bin reth -- --help | grep -q "snap-sync"; then
    echo "✅ Help output includes snap-sync option"
else
    echo "❌ Help output missing snap-sync option"
    exit 1
fi

echo ""
echo "🎉 All CLI tests passed! Snap sync CLI integration is working correctly."
echo ""
echo "📋 Available snap sync options:"
echo "  --sync-mode {full|snap|checkpoint}  Sync mode selection"
echo "  --snap-sync                         Enable snap sync (deprecated)"
echo "  --snap-max-concurrent-requests      Max concurrent requests"
echo "  --snap-max-response-bytes           Max response size in bytes"
echo "  --snap-max-accounts-per-request     Max accounts per request"
echo "  --snap-max-storage-slots-per-request Max storage slots per request"
echo "  --snap-max-byte-codes-per-request   Max byte codes per request"
echo "  --snap-max-trie-nodes-per-request   Max trie nodes per request"
echo "  --snap-commit-threshold             Commit threshold"
echo ""
echo "💡 Example usage:"
echo "  reth --sync-mode snap --snap-max-concurrent-requests 20"
echo "  reth --snap-sync  # deprecated but still works"