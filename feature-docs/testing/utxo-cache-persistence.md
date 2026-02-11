---
title: UTXO Cache Persistence
status: testing
priority: high
ideation-ref: docs/enhancements/02-utxo-cache-persistence.md
affected-files:
  - src/domain/utxo/cache.rs
  - src/config/mod.rs
  - src/main.rs
  - config/live.toml
  - Cargo.toml
---

# UTXO Cache Persistence

## Summary

Add save/load persistence for the in-memory UTXO cache (~2GB, ~27M entries) using a custom binary format with CRC32 integrity checks. On graceful shutdown (SIGINT or SIGTERM), the cache is serialized to disk. On startup, if the file exists, it's loaded back — replacing minutes of Neo4j pre-warming with seconds of sequential I/O. Periodic snapshots during ingestion protect against hard crashes (OOM, kill -9). Also fixes a pre-existing bug where SIGTERM is not handled (systemd sends SIGTERM on `systemctl stop`).

## Acceptance Criteria

1. GIVEN an empty `UtxoCache` with entries inserted WHEN `save_to_file(path, checkpoint_height)` is called THEN a binary file is written at `path` containing a 24-byte header (magic "UTXO", version 1 u32 LE, entry count u64 LE, checkpoint height u32 LE, CRC32 u32 LE) followed by all cache entries, and the method returns `Ok(entry_count)`

2. GIVEN a valid cache file produced by `save_to_file` WHEN `load_from_file(path, current_checkpoint_height)` is called on an empty cache THEN all entries are restored into the correct shards with LRU ordering preserved (MRU entries at front), the CRC32 is validated, and the method returns `Ok(loaded_count)`

3. GIVEN `load_from_file` is called with a path that does not exist WHEN the method executes THEN it returns `Ok(0)` without error

4. GIVEN a cache file with corrupted bytes (CRC mismatch) WHEN `load_from_file` is called THEN it returns `Err` with `ErrorKind::InvalidData` containing the expected and computed CRC values, and no entries are inserted into the cache

5. GIVEN a cache file with invalid magic bytes WHEN `load_from_file` is called THEN it returns `Err` with `ErrorKind::InvalidData`

6. GIVEN a cache file saved at height 100 WHEN `load_from_file` is called with `current_checkpoint_height = Some(200)` THEN a `tracing::warn!` is emitted with `saved_at_height` and `current_height` fields, and the cache is still loaded successfully

7. GIVEN `save_to_file` is called WHEN the write is in progress THEN data is written to `<path>.bin.tmp` first, then atomically renamed to `<path>`, preventing corrupt partial files on crash

8. GIVEN `PerformanceConfig` WHEN deserialized from TOML without `utxo_cache_file` or `utxo_cache_snapshot_interval` THEN defaults are `"utxo_cache.bin"` and `2000` respectively

9. GIVEN the `live` command is running WHEN a SIGTERM signal is received THEN the shutdown handler fires (same as SIGINT), allowing graceful cache save before exit

10. GIVEN `utxo_cache_file` is set and the `live` command starts WHEN a cache file exists at that path THEN the cache is loaded from file before ingestion begins

11. GIVEN periodic snapshots are enabled (`utxo_cache_snapshot_interval > 0`) WHEN ingestion reaches a height divisible by the interval THEN `save_to_file` is called with the current height

12. GIVEN `utxo_cache_file` is empty string WHEN the application runs THEN no cache load, save, or periodic snapshot is attempted

## Edge Cases

- Cache file truncated mid-entry (e.g., filesystem corruption) — `read_exact` returns `UnexpectedEof`, load fails gracefully with `Err`, cache remains empty
- Cache capacity reduced in config between save and load — entries exceeding new capacity are evicted by LRU during insertion, no error
- Cache capacity increased between save and load — extra space remains free, no error
- `save_to_file` called with poisoned shard mutex — panics with "shard poisoned" (matches existing cache behavior)
- Address field contains maximum-length Bitcoin address (~90 bytes bech32m) — u16 length prefix handles up to 65535 bytes
- `ScriptTypeTag` value in file is > 7 (Unknown) — deserialized as `ScriptTypeTag::Unknown`
- Periodic snapshot fails (disk full, permissions) — warning logged, ingestion continues (non-fatal)

## Out of Scope

- **Compression (LZ4/zstd)** — adds dependency complexity for v1; sequential I/O is already fast (~3s for 1.4GB). Can be added in v2 if I/O proves to be a bottleneck. Do not add compression to `save_to_file`/`load_from_file`.
- **Adding `CancellationToken` to `run_streaming_ingestion`** — that function runs a synchronous for-loop, not an async select. Adding graceful shutdown there is a larger refactor. Periodic snapshots provide adequate crash protection for ingest/resume commands.
- **Backwards-seeking file read** — could reduce memory during load by avoiding per-shard vecs, but adds complexity. The two-pass approach is simpler and still fast for v1.

## Technical Notes

- The enhancement doc at `docs/enhancements/02-utxo-cache-persistence.md` contains the complete implementation spec including all code, file format, and integration points. The builder should follow it closely.
- `lru` crate v0.12 `iter()` takes `&self`, returns `(&K, &V)` in MRU-to-LRU order without modifying LRU state — safe for save.
- `UtxoKey.txid` and `UtxoKey.vout` are private but accessible from `cache.rs` (same module).
- Load inserts directly via `shard.put()` to avoid inflating atomic stats counters.
- Per-shard reverse insertion preserves LRU ordering: file stores MRU-first from `iter()`, reverse gives LRU-first insertion.
- The `cache_capacity()` doctest in `config/mod.rs` must be updated to include the two new `PerformanceConfig` fields or it won't compile.
- Add `crc32fast = "1.4"` to `Cargo.toml` — tiny zero-dep crate for hardware-accelerated CRC32.
- **Rejected**: Using serde/bincode for serialization — adds unnecessary dependency and slower than raw byte I/O for this fixed-format data. Custom binary format is simpler and faster.
- **Rejected**: Compression in v1 — sequential I/O at ~500 MB/s makes the ~1.4GB file take ~3 seconds. Compression adds latency (CPU) that may not offset I/O savings on fast storage.
