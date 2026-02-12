# Code Review: UTXO Cache Shutdown Height Bug

## Affected File

`src/main.rs` — `run_live_mode()` function (lines ~660–1104)

## Three `save_to_file` Call Sites in Live Mode

| Site | Location | Height Source | Correct? |
|------|----------|---------------|----------|
| Chunk commit | `ingestion.rs:750` | `end_height` from committed chunk | Yes |
| Shutdown save | `main.rs:1072` | `current_height.saturating_sub(1)` | **BUG** |
| Ingest completion | `main.rs:525` | `start_height + blocks_processed` | N/A (different command) |

## The Race Condition (RPC Catchup Path)

```
Line 819: ingest_blocks_batch(&blocks) succeeds
          → internally commits chunks, saves snapshot with correct end_height (ingestion.rs:750)
Line 835: shutdown_token.is_cancelled() → true → break
Line 844: current_height = batch_end + 1  ← NEVER REACHED
...
Line 1072: save_height = current_height.saturating_sub(1)  ← STALE, overwrites correct snapshot
```

`current_height` still holds the value from before the batch started. The shutdown save overwrites the correct per-chunk snapshot with a stale height.

## The Race Condition (ZMQ Real-Time Path)

```
Lines 964-1039: inner while loop processes blocks one at a time
  Line 972: ingest_blocks_batch(&[block_tuple]) succeeds
  Line 982: height += 1 (local variable, not current_height)
  ... loop continues ...
Line 1043: current_height = effective_tip + 1  ← only set AFTER loop completes
```

If shutdown fires during the inner while loop, `current_height` is whatever it was before entering the loop. Same bug.

## `current_height` Semantics

`current_height` means "next block to fetch", NOT "last committed block". It's updated at:
- Line 844: `current_height = batch_end + 1` (after RPC batch, after shutdown check)
- Line 1028: `current_height = height` (reorg case only)
- Line 1043: `current_height = effective_tip + 1` (after ZMQ inner loop completes)

None of these run before the shutdown check that breaks out of the loop.

## Proposed Fix Validation

The proposed fix (separate `last_committed_height: Option<u32>` variable) is sound:

1. **RPC catchup**: Set `last_committed_height = Some(batch_end)` after line 832 (after `ingest_blocks_batch` succeeds, before shutdown check at line 835). This works because `batch_end` is computed at line 810 from the actual blocks fetched.

2. **ZMQ real-time**: Set `last_committed_height = Some(height)` after line 974 (after successful single-block ingestion). This works because `height` is the actual block that was just committed.

3. **Shutdown save**: Use `last_committed_height` instead of `current_height.saturating_sub(1)`. If `None`, skip the save (no blocks committed this session → existing cache file is still valid).

## Edge Cases

- **Shutdown before any batch completes**: `last_committed_height` is `None`, save is skipped. Correct — the existing cache file (if any) from a prior run is still valid.
- **Shutdown during first fetch (before any ingestion)**: Same as above.
- **Multiple chunks within a batch**: `try_save_snapshot` in ingestion.rs already saves after each chunk. The shutdown save with `batch_end` would match or be close to the last chunk's end height. Since `batch_end` is the end of the full batch and all chunks committed, this is correct.
- **Reorg during ZMQ**: After rollback, `height = fork_point + 1` and the loop breaks. `last_committed_height` might be ahead of the rollback point. However, the rollback itself should have cleaned the cache. **This needs further consideration** — but it's a separate issue from the core bug.

## Additional Observation

The `ingest` command (non-live) has its own save at line 522-525 using `start_height + blocks_processed`. This is a different code path and not affected by this bug, but uses a similar pattern that could theoretically have a similar issue if interrupted. Worth noting but out of scope.
