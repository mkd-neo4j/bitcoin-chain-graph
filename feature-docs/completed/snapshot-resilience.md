---
title: Snapshot Resilience — Save After Every Committed Batch
status: completed
priority: high
ideation-ref: feature-docs/ideation/snapshot-resilience/
affected-files:
  - src/domain/ingestion.rs
  - src/main.rs
  - src/config/mod.rs
---

# Snapshot Resilience — Save After Every Committed Batch

## Summary

The UTXO cache snapshot drifts arbitrarily far behind the Neo4j checkpoint when the app crashes repeatedly, because periodic saves only trigger at 2000-block boundaries and clean-exit saves never run during crashes. This feature moves snapshot saving into `ingest_blocks_batch` — immediately after each successful `commit_transaction()` — so the snapshot always stays in sync with the checkpoint. It also removes the now-dead periodic snapshot logic from `main.rs` and the `utxo_cache_snapshot_interval` config field.

## Acceptance Criteria

1. GIVEN `IngestionOrchestrator` is constructed with a non-empty `cache_snapshot_path` WHEN `ingest_blocks_batch` successfully commits a chunk THEN `save_to_file` is called with the chunk's last block height

2. GIVEN `IngestionOrchestrator` is constructed with `cache_snapshot_path = None` WHEN `ingest_blocks_batch` successfully commits a chunk THEN no snapshot save is attempted

3. GIVEN a snapshot save fails inside `ingest_blocks_batch` WHEN the error is logged THEN ingestion continues without returning an error (non-fatal, `tracing::warn!`)

4. GIVEN `run_streaming_ingestion` is called WHEN configuring the orchestrator THEN `cache_snapshot_path` is set from `config.performance.utxo_cache_file` (or `None` if empty string)

5. GIVEN `run_live_ingestion` catchup phase calls `ingest_blocks_batch` WHEN a chunk commits successfully THEN the snapshot is saved (same code path as streaming — no separate periodic save logic)

6. GIVEN `run_live_ingestion` real-time phase calls `ingest_blocks_batch` with a single block WHEN the chunk commits successfully THEN the snapshot is saved

7. GIVEN the periodic snapshot blocks in `run_streaming_ingestion` (`main.rs:503-512`), `run_live_ingestion` catchup (`main.rs:857-865`), and `run_live_ingestion` real-time (`main.rs:988-996`) WHEN the feature is complete THEN all three blocks are removed entirely — no residual dead code

8. GIVEN the `utxo_cache_snapshot_interval` field in `PerformanceConfig` and its default function `default_utxo_cache_snapshot_interval` WHEN the feature is complete THEN both are removed from `src/config/mod.rs`, along with any references in validation or documentation

9. GIVEN existing tests that verify snapshot persistence (e.g., `tests/domain/utxo/persistence.rs`) WHEN the feature is complete THEN all existing persistence tests still pass — `save_to_file` and `load_from_file` behavior is unchanged

## Edge Cases

- **First batch after fresh start** (no snapshot file exists) — `save_to_file` creates the file atomically via temp+rename; no special handling needed
- **Empty batch** (no blocks to process) — `ingest_blocks_batch` iterates zero chunks, no commit happens, no save attempted
- **Snapshot save I/O failure** (disk full, permissions) — logged as `tracing::warn!`, ingestion continues; next successful batch overwrites
- **Single-block batch in live real-time mode** — `ingest_blocks_batch(&[block], 1)` produces one chunk of one block; snapshot saved after commit, which is correct but frequent. I/O cost is ~100ms for 150MB file, acceptable for real-time cadence (~1 block/10 min)

## Out of Scope

- **Signal handling for `ingest`/`resume` commands** — adding `CancellationToken` + SIGINT/SIGTERM handler to `run_streaming_ingestion()` is a related improvement but a separate feature. Do not add it here — it changes control flow in `main.rs` beyond the snapshot logic and needs its own acceptance criteria. The snapshot-after-commit fix already eliminates the primary data loss scenario (stale snapshot on crash).
- **Removing the completion save in `run_streaming_ingestion` (`main.rs:519-525`)** — keep this as a safety net. It is NOT dead code because it covers the final partial batch that may not align with `batch_size`. Do not remove it.
- **Removing the shutdown save in `run_live_ingestion` (`main.rs:1086-1096`)** — keep this as a safety net for graceful shutdown. The live real-time loop may have processed blocks between the last `ingest_blocks_batch` call and shutdown. Do not remove it.
- **Implementing `begin_transaction`/`commit_transaction`** — these are currently `todo!()` stubs in Neo4jWriter. This feature does not change that; the snapshot save is independent of DB transaction semantics.

## Technical Notes

- **Where to save**: Inside `ingest_blocks_batch` (ingestion.rs:560-619), after `self.writer.commit_transaction().await?` succeeds at line 597. Extract the chunk's last height from `chunk.last().map(|(h, _, _)| *h)`.
- **How to pass the path**: Add `cache_snapshot_path: Option<PathBuf>` field to `IngestionOrchestrator`. Set it via a new public method `set_cache_snapshot_path(&mut self, path: Option<PathBuf>)` called in `main.rs` before ingestion starts. Do NOT change the constructor signature — it would break MockWriter test setup across many files.
- **Private helper**: Add `fn try_save_snapshot(&self, height: u32)` to `IngestionOrchestrator` that checks `self.cache_snapshot_path.is_some()`, calls `save_to_file`, and logs on error. This keeps the batch loop clean.
- **Config removal**: Remove `utxo_cache_snapshot_interval` field, `default_utxo_cache_snapshot_interval()` function, and any validation referencing it. The `utxo_cache_file` field stays — it's still used to set the orchestrator's path and for the completion/shutdown saves.
- **Follow the pattern in** `src/domain/ingestion.rs` for field additions — see how `writer: Arc<W>` and `utxo_cache: UtxoCache` are initialized.
- **Rejected**: Saving the snapshot inside `process_batch_chunk` (before checkpoint update) — this would save BEFORE the commit, so on crash the snapshot could be ahead of the checkpoint. Save must happen AFTER commit to maintain the invariant: snapshot height ≤ checkpoint height.
- **Rejected**: Adding a `SnapshotManager` abstraction — unnecessary indirection for a single `save_to_file` call behind an `Option<PathBuf>` check.
