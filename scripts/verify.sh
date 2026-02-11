#!/usr/bin/env bash
set -uo pipefail

echo "==> Running Rust verification" >&2

echo "--- Cargo check ---" >&2
cargo check 2>&1 || { echo "FAIL: cargo check failed" >&2; exit 2; }

echo "--- Clippy ---" >&2
cargo clippy -- -D warnings 2>&1 || { echo "FAIL: clippy errors" >&2; exit 2; }

echo "--- Tests ---" >&2
cargo test 2>&1 || { echo "FAIL: cargo test failed" >&2; exit 2; }

echo "" >&2
echo "All checks passed." >&2
