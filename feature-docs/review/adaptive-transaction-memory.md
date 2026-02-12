---
title: Adaptive Transaction Memory Control
status: review
priority: high
ideation-ref: feature-docs/ideation/transaction-memory-control/
affected-files:
  - src/config/mod.rs
  - src/domain/ingestion.rs
  - src/main.rs
  - config/default.toml
  - config/live.toml
---

# Adaptive Transaction Memory Control

## Summary

Neo4j transaction memory explodes on modern blocks (post-2017) because `ingestion.batch_size` is a fixed block count that doesn't account for the 1000x variation in block data volume across Bitcoin's history. Early blocks contain ~5 transactions (~16 KB in transaction state); modern blocks contain ~3,000 transactions (~10 MB each). A fixed batch of 200 modern blocks hits Neo4j's 2GB transaction limit.

This feature replaces the fixed `batch_size` and three ghost config fields with a single adaptive knob — `max_transaction_memory_mb` — that dynamically sizes each transaction's block count based on the actual entity counts (transactions, outputs, inputs) of the already-parsed blocks. The orchestrator counts entities before writing and splits blocks into chunks that fit within the memory budget, so early blocks batch thousands together while modern blocks batch ~60-75.

## Acceptance Criteria

### Adaptive Chunking

1. GIVEN `max_transaction_memory_mb = 600` and a batch of 100 blocks where each block has 3,000 transactions, 6,000 outputs, and 7,500 inputs WHEN `ingest_blocks_batch()` is called THEN it produces multiple adaptive chunks where each chunk's `estimate_batch_memory()` is at most 600 × 1,024 × 1,024 bytes

2. GIVEN `max_transaction_memory_mb = 600` and a batch of 5,000 blocks where each block has 1 transaction, 1 output, and 0 non-coinbase inputs WHEN `ingest_blocks_batch()` is called THEN it produces a single chunk containing all 5,000 blocks (total estimate well under budget)

3. GIVEN a batch containing a mix of small blocks (5 txs) and large blocks (3,000 txs) WHEN `compute_adaptive_chunks()` runs THEN earlier chunks contain more blocks and later chunks contain fewer blocks, adapting to the increasing data volume

4. GIVEN a single block whose estimated memory exceeds `max_transaction_memory_mb` WHEN `compute_adaptive_chunks()` runs THEN it places that block alone in its own chunk (minimum 1 block per chunk) and logs a warning at `tracing::warn!` level including the block height, estimated memory in MB, and the configured limit

5. GIVEN an empty blocks slice WHEN `compute_adaptive_chunks()` runs THEN it returns an empty `Vec<Range<usize>>`

### Memory Estimator

6. GIVEN a `bitcoin::Block` with T transactions, O total outputs across all transactions, and I total inputs across all transactions WHEN `estimate_block_memory()` is called THEN it returns `500 + (T × 400) + (O × 550) + (I × 550)` bytes

7. GIVEN the memory estimation constants `BYTES_PER_BLOCK = 500`, `BYTES_PER_TX = 400`, `BYTES_PER_OUTPUT = 550`, `BYTES_PER_INPUT = 550` WHEN defined in `src/domain/ingestion.rs` THEN they are `pub(crate)` constants (accessible to tests but not public API)

### Config Changes

8. GIVEN `IngestionConfig` WHEN its fields are inspected THEN it contains `max_transaction_memory_mb: usize` with a default value of `600`

9. GIVEN `IngestionConfig` WHEN its fields are inspected THEN it does NOT contain `batch_size` or `checkpoint_interval` fields

10. GIVEN a TOML config file with `[ingestion] max_transaction_memory_mb = 400` WHEN parsed THEN `config.ingestion.max_transaction_memory_mb` equals `400`

11. GIVEN a TOML config file with legacy fields `batch_size`, `checkpoint_interval`, or `max_batch_memory_mb` under `[ingestion]` WHEN parsed THEN deserialization succeeds without error (serde `deny_unknown_fields` is NOT set, so unknown fields are silently ignored for backward compatibility)

### Ingestion Loop Integration

12. GIVEN `max_transaction_memory_mb` from config WHEN `ingest_blocks_batch()` is called THEN it calls `compute_adaptive_chunks()` with `max_memory_bytes = max_transaction_memory_mb * 1024 * 1024` and iterates over the resulting chunk ranges, wrapping each in `begin_transaction()` / `commit_transaction()`

13. GIVEN the `ingest_blocks_batch()` method signature WHEN inspected THEN the `batch_size: usize` parameter has been removed — the method takes only `&self` and `blocks: &[(u32, Block, String)]`

14. GIVEN an adaptive chunk of N blocks WHEN the chunk is processed THEN the existing 7-phase pipeline (`process_batch_chunk`) runs identically to before — no changes to phase ordering, UTXO cache logic, or snapshot saving

### Observability

15. GIVEN adaptive chunking produces K chunks from a batch WHEN each chunk begins processing THEN a `tracing::info!` log is emitted with structured fields: `chunk` (1-indexed), `total_chunks`, `blocks` (count), `estimated_mb` (rounded), and the height range `start_height` to `end_height`

### Main.rs Caller Update

16. GIVEN `src/main.rs` calls `orchestrator.ingest_blocks_batch()` WHEN the call sites are inspected THEN none pass a `batch_size` argument — they call `orchestrator.ingest_blocks_batch(&batch).await`

17. GIVEN the outer block-loading loop in `main.rs` WHEN it accumulates blocks from disk THEN it uses a fixed read-ahead size (hardcoded constant, e.g. `5000`) that is independent of `max_transaction_memory_mb` — the memory-adaptive splitting happens inside the orchestrator, not in the loader

## Edge Cases

- Single block with 10,000+ transactions (stress-test block) exceeds memory budget — placed alone in its own chunk, warning logged with height and estimated MB
- `max_transaction_memory_mb = 1` — effectively per-block transactions, functional but slow; no error
- `max_transaction_memory_mb = 10000` — very large budget, may hit Neo4j server-side limits; not our concern (user is responsible for Neo4j server config)
- Blocks with 0 non-coinbase transactions (height 0 through ~170) — negligible memory, thousands pack into one chunk
- Bitcoin blocks at BIP30 duplicate heights (91842, 91880) — handled identically; BIP30 logic is in `process_batch_chunk`, not in chunking

## Out of Scope

- **Do NOT modify `src/writer/neo4j/mod.rs`** — the Neo4j writer's `execute_batched()`, transaction methods, and `write_batch_size` sub-chunking are unchanged. `write_batch_size` controls UNWIND query size within a transaction (separate concern from transaction memory budget). Coupling them would create a confusing dependency between config fields.
- **Do NOT modify `src/writer/traits.rs` or `src/writer/mock.rs`** — the GraphWriter trait and MockWriter are unaffected. The adaptive chunking happens in the orchestrator before any writer methods are called.
- **Do NOT modify `src/parser/`** — `single_block_loader.rs` has its own `INDEX_BATCH_SIZE` (500) and `rpc_provider.rs` uses `bitcoin_rpc.batch_size`. These are block-loading concerns, not transaction memory concerns. Changing them would break the parser/domain boundary.
- **Do NOT modify `config/loader.rs`** test assertions — update them to reflect the new config fields, but do not change the test infrastructure itself.
- **Do NOT add runtime memory measurement** (e.g. querying Neo4j heap usage mid-transaction) — the estimator uses pre-computed entity counts which is fast and deterministic. Runtime measurement would add latency, Neo4j API dependency, and non-determinism.
- **Do NOT remove `neo4j.write_batch_size`** — it controls a separate concern (UNWIND query parameter size). Removing it would force a single UNWIND per phase, which can exceed Neo4j query parameter limits on large batches.

## Technical Notes

- **Estimation constants are conservative**: `BYTES_PER_OUTPUT = 550` and `BYTES_PER_INPUT = 550` intentionally overestimate by ~30% to absorb Neo4j index maintenance overhead, PERFORMS/BENEFITS_TO aggregation variance, and internal transaction bookkeeping. If users still hit OOM, they lower the knob.
- **`estimate_block_memory()` operates on `bitcoin::Block`** (not domain types) because it runs before conversion. The `block.txdata` Vec gives exact transaction/output/input counts with no allocation.
- **`compute_adaptive_chunks()` returns `Vec<Range<usize>>`** (index ranges into the blocks slice) rather than cloning block data. The caller indexes the original slice.
- **Checkpoint behavior is unchanged**: `update_checkpoint()` runs inside each chunk's transaction, exactly as before. The only difference is chunk sizes vary instead of being fixed.
- **Snapshot saving is unchanged**: `try_save_snapshot()` runs after each chunk's commit, exactly as before. More frequent commits on modern blocks means more frequent snapshots — this is a benefit, not a problem.
- **Ghost field removal**: `checkpoint_interval` was defined in `IngestionConfig` but never referenced outside config tests. `max_batch_memory_mb` and `utxo_cache_snapshot_interval` existed only in TOML files, never in the Rust struct. All three are dead code.
- **Backward compatibility**: serde's default behavior ignores unknown TOML fields, so users with old config files containing `batch_size` or `checkpoint_interval` won't get deserialization errors. The fields are simply ignored.
- **Read-ahead batch in main.rs**: The outer loading loop keeps a fixed read-ahead (e.g. 5000 blocks loaded from disk at once). This is a disk I/O optimization unrelated to transaction memory. The adaptive splitting happens inside the orchestrator after blocks are loaded.
- **Rejected: MERGE queries for idempotent replay** — MERGE does a uniqueness lookup on every row, massive performance penalty at millions of entities. Adaptive batching with CREATE + checkpoint recovery is faster and simpler.
- **Rejected: per-phase commits** — would require either MERGE (slow) or cleanup-on-resume (DETACH DELETE, complex and error-prone). Keeping single-transaction atomicity per chunk avoids both problems.
- **Rejected: runtime Neo4j memory queries** — adds latency, API dependency, and non-determinism. Static estimation from entity counts is fast, predictable, and sufficient.
- **Follow the pattern in** `src/domain/ingestion.rs:697-708` for entity counting — the same `block.txdata.len()`, `tx.output.len()`, `tx.input.len()` iteration is used in `process_batch_chunk()` for Vec pre-allocation.
