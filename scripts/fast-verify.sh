#!/usr/bin/env bash
set -uo pipefail

# Fast verification: cargo check only, no clippy or full test suite.
# Use this for quick feedback during development. The full verify.sh
# runs on TaskCompleted and before marking work done.

echo "==> Running fast Rust verification (cargo check only)" >&2

echo "--- Cargo check ---" >&2
cargo check 2>&1 || { echo "FAIL: cargo check failed" >&2; exit 2; }

echo "" >&2
echo "Fast check passed." >&2
