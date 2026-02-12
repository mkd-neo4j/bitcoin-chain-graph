---
title: Fix UTXO Cache Shutdown Save Height
status: review
priority: high
ideation-ref: feature-docs/ideation/utxo-cache-shutdown-height/
affected-files:
  - src/main.rs
---

# Fix UTXO Cache Shutdown Save Height

## Summary

When `run_live_mode()` receives SIGTERM during RPC catchup or ZMQ real-time ingestion, the graceful shutdown saves the UTXO cache with a stale height derived from `current_height` (which tracks the next block to fetch, not the last committed block). This overwrites the correct per-chunk snapshot written by `try_save_snapshot()` in `ingestion.rs:750`. On restart, the cache is missing UTXOs created between the saved height and the Neo4j checkpoint, causing missing UTXO errors and a crash.

The fix introduces a `last_committed_height: Option<u32>` variable that is updated after every successful `ingest_blocks_batch` call. The shutdown save uses this instead of `current_height.saturating_sub(1)`.

## Acceptance Criteria

1. GIVEN `run_live_mode()` is in RPC catchup and a batch has been committed (e.g. blocks 207600–207799) WHEN SIGTERM arrives after `ingest_blocks_batch` returns `Ok(())` but before `current_height = batch_end + 1` executes THEN the shutdown `save_to_file` call uses height `207799` (the actual `batch_end`), not `207599` (`current_height - 1`)

2. GIVEN `run_live_mode()` is in the ZMQ real-time inner loop and block 300005 has just been committed via `ingest_blocks_batch(&[block_tuple])` WHEN the outer loop's shutdown check fires THEN the shutdown `save_to_file` call uses height `300005`, not the `current_height` value from before the inner loop started

3. GIVEN `run_live_mode()` starts and SIGTERM arrives before any `ingest_blocks_batch` call completes WHEN the shutdown save logic runs THEN `save_to_file` is NOT called (preserving the existing cache file from a prior run) AND a log message at INFO level indicates no blocks were committed this session

4. GIVEN `run_live_mode()` completes multiple RPC catchup batches (batch 1: blocks 0–199, batch 2: blocks 200–399) and then receives SIGTERM during the third batch fetch WHEN the shutdown save runs THEN `save_to_file` uses height `399` (the end of the last successfully committed batch)

5. GIVEN `run_live_mode()` is in the ZMQ real-time inner loop and processes blocks 500–505 individually WHEN SIGTERM arrives after block 503 commits but before block 504 is fetched THEN the shutdown `save_to_file` uses height `503`

## Edge Cases

- **SIGTERM during RPC fetch (before ingestion)** — `last_committed_height` retains value from prior batch (or `None` if first batch). Shutdown save uses that value or skips. No stale overwrite.
- **SIGTERM after `ingest_blocks_batch` error** — The batch failed, so `last_committed_height` is NOT updated. Shutdown save uses the prior committed height. Correct — failed batches are rolled back by `ingestion.rs`.
- **Multiple chunks within a single batch** — `try_save_snapshot` in `ingestion.rs:750` saves after each chunk commit. The shutdown save with `batch_end` matches the final chunk's end height since all chunks committed successfully (otherwise `ingest_blocks_batch` would have returned `Err`).
- **No cache file configured** (`cache_file` is empty) — The existing `if !cache_file.is_empty()` guard already handles this. No change needed.

## Out of Scope

- **Reorg + shutdown interaction** — After a rollback, `last_committed_height` may reference a height that was rolled back. The rollback cleans the cache, but the shutdown save could write a misleading height. This is a separate, pre-existing issue unrelated to the `current_height` bug. Do NOT attempt to fix rollback cache consistency here — it requires changes to `ingestion.rs` rollback logic.
- **`ingest` command shutdown save** (`main.rs:522–525`) — Uses `start_height + blocks_processed`, a different code path with its own potential issue. Fixing it here risks scope creep into a function (`run_ingest_mode`) that has different control flow. Separate bug fix if needed.
- **`try_save_snapshot` changes** — The per-chunk snapshot save in `ingestion.rs:750` is correct. Do NOT modify it.

## Technical Notes

- `last_committed_height` must be declared as `Option<u32>` initialized to `None` alongside `current_height` and `blocks_processed` near line 720.
- In the RPC catchup path, set `last_committed_height = Some(batch_end)` immediately after `ingest_blocks_batch` returns `Ok(())` (after line 832, before the shutdown check at line 835). `batch_end` is computed at line 810 from the actual fetched blocks.
- In the ZMQ real-time path, set `last_committed_height = Some(height)` after `ingest_blocks_batch(&[block_tuple])` returns `Ok(())` (after line 974, inside the `Ok(())` arm).
- The shutdown save block (lines 1070–1088) changes from unconditional `save_to_file(cache_file, current_height.saturating_sub(1))` to conditional `if let Some(save_height) = last_committed_height { save_to_file(cache_file, save_height) }`.
- The `last_height` log at line 1095 should also use `last_committed_height.unwrap_or(resume_height).saturating_sub(1)` or similar, but this is cosmetic — the critical fix is the `save_to_file` call.
- Follow the existing pattern: `save_to_file` returns `Result<usize>` and is matched with `Ok(saved)` / `Err(e)` arms. Keep the same structure.
- **Rejected**: Using `batch_end` directly in the shutdown save without a tracking variable — this doesn't work because `batch_end` is scoped to the loop iteration and the ZMQ path uses `height` (a different local variable). A single `last_committed_height` covers both paths.
