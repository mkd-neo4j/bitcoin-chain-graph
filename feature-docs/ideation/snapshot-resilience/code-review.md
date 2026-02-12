# Code Review: Snapshot Resilience

## Current Architecture Summary

### Snapshot Save Points (3 triggers)

1. **Periodic** (`main.rs:504-512`): `batch_end_height % snapshot_interval == 0` (default interval: 2000 blocks)
2. **Clean completion** (`main.rs:519-525`): After the ingestion loop finishes normally
3. **Graceful shutdown** (`main.rs:1086-1096`): On SIGINT/SIGTERM in live mode only — `ingest`/`resume` have NO signal handler

### Snapshot Load (on startup)

- `main.rs:364-379` (`ingest`/`resume`): loads snapshot, passes `start_height` for staleness comparison
- `main.rs:645-673` (live mode): loads snapshot, passes Neo4j checkpoint height
- On height mismatch: **warns but still loads** — considers cache "still usable but may contain stale entries"
- On CRC failure / corrupt file: returns empty cache, relies on pre-warming

### Snapshot File Format

- Custom binary, 24-byte header (`UTXO` magic, version 1, entry count, checkpoint height, CRC32)
- Variable-length entries (txid, vout, output_index, amount, script_type, optional address)
- **Atomic writes**: temp file + `sync_all()` + rename — no partial writes possible
- **CRC32 validation**: Two-pass load — reads all entries, validates CRC, then inserts. Corrupt file = empty cache.

### Resume Flow

1. Connect to Neo4j → read `IngestionCheckpoint` node → `sync_checkpoint_with_db()`
2. Load snapshot from disk (CRC-validated)
3. Optionally pre-warm from `.blk` files backwards
4. Start ingestion from `checkpoint_height + 1`

### The Bug: Crash-Restart Staleness Loop

The problem occurs when:
1. App ingests blocks, saving periodic snapshots at multiples of 2000
2. App crashes at some height (e.g., block 91,799)
3. Neo4j checkpoint advances to the last committed batch (e.g., 91,799) — committed within the DB transaction
4. Snapshot on disk stays at last periodic save (e.g., 13,999) — the batches between 14,000 and 91,799 completed but never hit a 2000-boundary save, OR crashed before the next boundary
5. On restart: loads stale snapshot (13,999), resumes from checkpoint (91,800)
6. If the crash recurs quickly (same bad block), the batch never completes far enough to reach the next 2000 boundary
7. Cycle repeats indefinitely — snapshot never advances

## Key Files to Modify

| File | Lines | What |
|------|-------|------|
| `src/main.rs` | 503-512 | Periodic save trigger (3 locations) |
| `src/main.rs` | 519-525 | Clean completion save |
| `src/main.rs` | 364-379 | Snapshot load + staleness handling |
| `src/domain/utxo/cache.rs` | 783-957 | `load_from_file()` — staleness detection |
| `src/domain/utxo/cache.rs` | 656-773 | `save_to_file()` |
| `src/config/mod.rs` | 197-202 | `utxo_cache_snapshot_interval` config |

## Critical Observations

### 1. No signal handling in `ingest`/`resume` commands
`run_streaming_ingestion()` has no `CancellationToken`. Only `run_live_ingestion()` handles SIGINT/SIGTERM. A Ctrl+C during `ingest` or `resume` kills the process immediately — no clean-exit cache save.

### 2. Snapshot save is decoupled from checkpoint commit
The checkpoint is updated inside the DB transaction (lines 1004-1020 of ingestion.rs), but the snapshot save happens AFTER the batch in `main.rs`. If the app crashes between checkpoint commit and snapshot save, the checkpoint advances but the snapshot stays behind.

### 3. Cache misses are hard errors
`get_many_or_fail()` returns `WriterError` on any cache miss. There is NO fallback to query Neo4j for missing UTXOs. A stale snapshot that's missing entries needed for the current block will cause immediate failure.

### 4. The `begin_transaction`/`commit_transaction` are `todo!()` stubs
Individual Cypher queries auto-commit. Crash recovery via `sync_checkpoint_with_db` compensates by rolling back incomplete blocks.

### 5. Pre-warming is the only recovery mechanism
When a snapshot is stale, pre-warming from `.blk` files backwards is the only way to fill the gap. But pre-warming has a configurable depth (`utxo_prewarm_depth`) and may not go back far enough if the staleness gap is large.
