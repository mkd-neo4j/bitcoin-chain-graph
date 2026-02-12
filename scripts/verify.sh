#!/usr/bin/env bash
set -uo pipefail

# Full verification pipeline: cargo check → clippy → tests.
# Verbose output goes to agent_logs/; only the summary is printed to stderr.

mkdir -p agent_logs

echo "==> Running Rust verification" >&2

echo "--- Format check ---" >&2
cargo fmt -- --check 2>&1 | tee agent_logs/cargo-fmt.log | tail -10 >&2
[ "${PIPESTATUS[0]}" -eq 0 ] || { echo "FAIL: cargo fmt check failed (see agent_logs/cargo-fmt.log)" >&2; exit 2; }

echo "--- Cargo check ---" >&2
cargo check 2>&1 | tee agent_logs/cargo-check.log | tail -10 >&2
[ "${PIPESTATUS[0]}" -eq 0 ] || { echo "FAIL: cargo check failed (see agent_logs/cargo-check.log)" >&2; exit 2; }

echo "--- Clippy ---" >&2
cargo clippy -- -D warnings 2>&1 | tee agent_logs/clippy.log | tail -10 >&2
[ "${PIPESTATUS[0]}" -eq 0 ] || { echo "FAIL: clippy errors (see agent_logs/clippy.log)" >&2; exit 2; }

echo "--- Tests ---" >&2
cargo test 2>&1 | tee agent_logs/cargo-test.log | tail -10 >&2
[ "${PIPESTATUS[0]}" -eq 0 ] || { echo "FAIL: cargo test failed (see agent_logs/cargo-test.log)" >&2; exit 2; }

echo "" >&2
echo "All checks passed." >&2
