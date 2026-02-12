# Design: Save Snapshot After Each Batch Chunk

## Current Flow

```
main.rs: for height in start..=max {
    batch.push(block);
    if batch.len() >= batch_size {
        orchestrator.ingest_blocks_batch(&batch, batch_size);  // ← may process multiple chunks
        // periodic save here (batch_end_height % 2000 == 0)
        batch.clear();
    }
}
```

Inside `ingest_blocks_batch` (ingestion.rs:560-619):
```
for chunk in blocks.chunks(batch_size) {
    writer.begin_transaction();
    process_batch_chunk(chunk);     // ← includes checkpoint update at line 1014
    writer.commit_transaction();    // ← checkpoint is committed with data
}
```

Note: `ingest_blocks_batch` receives `blocks` (the full batch from main.rs) and `batch_size` (same value), so `blocks.chunks(batch_size)` typically produces **one chunk** equal to the full batch. But if the batch accumulated more than `batch_size` blocks (e.g., the last iteration), there could be multiple chunks.

## Problem

The snapshot save in `main.rs:504-512` runs AFTER `ingest_blocks_batch` returns, and only if `batch_end_height % 2000 == 0`. The checkpoint update runs INSIDE `process_batch_chunk` at the end of the DB transaction. This creates a window where:

1. Checkpoint is committed to Neo4j ✓
2. App crashes before returning to main.rs
3. Snapshot never saved ✗

## Proposed Design

Move snapshot saving INTO `ingest_blocks_batch`, right after `commit_transaction()` succeeds:

```rust
// ingestion.rs — ingest_blocks_batch
for (batch_idx, chunk) in blocks.chunks(batch_size).enumerate() {
    self.writer.begin_transaction().await?;
    let chunk_result = self.process_batch_chunk(chunk, ...).await;
    match chunk_result {
        Ok(()) => {
            self.writer.commit_transaction().await?;

            // NEW: Save snapshot immediately after successful commit
            let end_height = chunk.last().map(|(h, _, _)| *h).unwrap_or(0);
            self.save_cache_snapshot(end_height)?;  // new method
        }
        Err(e) => { /* rollback, return error */ }
    }
}
```

### Where should the snapshot path come from?

Options:
1. **Pass cache_file into `ingest_blocks_batch`** — simple, but widens the API
2. **Store cache_file in IngestionOrchestrator** — cleaner, set once at construction
3. **New method on UtxoCache that remembers its path** — couples cache to filesystem

**Recommendation: Option 2.** Add `cache_snapshot_path: Option<PathBuf>` to `IngestionOrchestrator`. Set it during construction or via a setter before ingestion starts. The orchestrator already owns the cache, so it's natural for it to own the persistence config too.

### What about the interval config?

With this design, `utxo_cache_snapshot_interval` becomes irrelevant — we save after every committed chunk. This is fine because:
- I/O cost is ~100ms per save (negligible vs batch processing time)
- The interval was only needed because saves were decoupled from commits
- Simpler mental model: "snapshot is always in sync with checkpoint"

We could keep the interval as an optional throttle (save every N chunks instead of every chunk), but YAGNI — start with every chunk.

## Changes Required

### `src/domain/ingestion.rs`
1. Add `cache_snapshot_path: Option<PathBuf>` field to `IngestionOrchestrator`
2. Add setter or constructor parameter for it
3. Add `save_cache_snapshot(&self, height: u32)` private method
4. Call it after `commit_transaction()` in `ingest_blocks_batch`

### `src/main.rs`
1. Set `cache_snapshot_path` on orchestrator after construction
2. Remove the periodic save block (`main.rs:503-512`) from all three locations
3. Keep the completion save (`main.rs:519-525`) as a final safety net
4. Keep the shutdown save in live mode (`main.rs:1086-1096`) as a safety net

### `src/config/mod.rs`
1. Deprecate or remove `utxo_cache_snapshot_interval` (or keep for backward compat)

### `src/writer/traits.rs` / `src/writer/mock.rs`
- No changes needed — snapshot is cache-level, not writer-level

## Edge Cases

1. **First batch after fresh start**: Snapshot file doesn't exist yet → first save creates it
2. **Empty batch**: No blocks processed → no commit → no save (correct)
3. **Save failure**: Log warning, continue ingestion (non-fatal, same as current behavior)
4. **Concurrent access**: `save_to_file` locks each shard sequentially — safe but blocks reads briefly during save. This is fine because save happens between batches, not during processing.

## Also Fix: Signal Handling for ingest/resume

Separate but related: add `CancellationToken` + signal handler to `run_streaming_ingestion()` so Ctrl+C triggers a clean shutdown with cache save, matching what `run_live_ingestion()` already does. This is a separate feature but should be bundled.
