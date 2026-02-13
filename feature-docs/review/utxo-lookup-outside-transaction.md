---
title: Move UTXO Fallback Lookups Outside Write Transaction
status: review
priority: high
affected-files:
  - src/domain/ingestion.rs
---

# Move UTXO Fallback Lookups Outside Write Transaction

## Summary

During `ingest_blocks_batch`, UTXO cache misses trigger `get_many_with_fallback()` which issues read queries to Neo4j via `lookup_outputs_batch()`. These reads currently execute inside the write transaction opened by `begin_transaction()`, inflating the transaction's memory footprint and duration. At height ~207K, a 200-block batch triggered 24,220 Neo4j fallback reads inside the write transaction, contributing to GC pressure that caused the commit to hang.

`lookup_outputs_batch()` already uses auto-commit queries (`self.graph.execute()`) rather than the active transaction, so the reads are not transactionally coupled to the writes. Moving the key collection (Phase 3a) and `get_many_with_fallback()` call to execute *before* `begin_transaction()` reduces transaction duration and memory without changing correctness.

## Acceptance Criteria

1. GIVEN a batch of blocks with UTXO cache misses WHEN `ingest_blocks_batch()` processes a chunk THEN `get_many_with_fallback()` completes before `self.writer.begin_transaction()` is called for that chunk

2. GIVEN a batch of blocks WHEN the input key collection (Phase 3a logic — iterating non-coinbase inputs to build `all_input_keys`) runs THEN it executes before `begin_transaction()`, not inside `process_batch_chunk()`

3. GIVEN pre-fetched UTXO data from `get_many_with_fallback()` WHEN `process_batch_chunk()` runs Phase 3 (transaction creation with amount calculation) THEN it receives the pre-fetched `HashMap<UtxoKey, CachedOutput>` as a parameter and uses it directly instead of calling `get_many_with_fallback()` again

4. GIVEN `process_batch_chunk()` WHEN its signature is inspected THEN it accepts an additional parameter `prefetched_utxos: HashMap<UtxoKey, CachedOutput>` containing the pre-resolved UTXO data

5. GIVEN same-block UTXO spending (an output created and spent within the same batch) WHEN Phase 2 populates the UTXO cache and Phase 3a collects input keys THEN the same-block output resolves from the in-memory cache (not via Neo4j fallback), so pre-fetching before the transaction does not break same-block resolution

6. GIVEN `ingest_blocks_batch()` processes multiple adaptive chunks WHEN each chunk runs THEN each chunk performs its own independent pre-fetch before its own `begin_transaction()` call

7. GIVEN the UTXO fallback log line `"UTXO cache fallback to Neo4j resolved all misses"` WHEN it is emitted THEN it appears in logs *before* the `"Processing adaptive chunk"` log line for that chunk

## Edge Cases

- Batch with zero cache misses (all inputs resolve from cache) — `get_many_with_fallback()` returns immediately from cache, no Neo4j queries issued, no behavioral change
- Batch where every input is a cache miss — all 24K+ lookups happen before the transaction, transaction contains only writes
- Multiple adaptive chunks in one batch — each chunk pre-fetches independently; chunk 2's pre-fetch may benefit from cache entries populated by chunk 1's Phase 2

## Out of Scope

- **Do NOT modify `src/writer/neo4j/mod.rs`** — `lookup_outputs_batch()` already uses auto-commit queries and needs no changes. Modifying the writer risks breaking the query retry/timeout logic.
- **Do NOT change the UTXO cache internals in `src/domain/utxo/cache.rs`** — `get_many_with_fallback()` is correct as-is; we're only changing *when* it's called, not *how*.
- **Do NOT change the memory estimator constants or `max_transaction_memory_mb`** — that is a separate feature for re-calibrating Neo4j memory estimation.
- **Do NOT refactor `process_batch_chunk` beyond adding the new parameter** — the 7-phase pipeline internals are unchanged.

## Technical Notes

- The key collection logic currently lives inside `process_batch_chunk()` at the Phase 3a section (building `all_input_keys` from non-coinbase inputs). Extract this into a standalone function or inline it in `ingest_blocks_batch()` before the transaction boundary.
- `process_batch_chunk` currently calls `get_many_with_fallback` at two sites: line ~957 (batch path) and line ~1297 (single-block path). Both must be replaced with the pre-fetched data.
- Same-block UTXO spending safety: Phase 2 inserts outputs into the cache *during* `process_batch_chunk`. Since pre-fetching happens before the chunk is processed, same-block outputs won't be in the cache yet during pre-fetch. However, this is fine — same-block inputs will miss during pre-fetch, but Phase 2 populates the cache before Phase 3 uses it. The pre-fetched map should be used as a *fallback source* alongside the cache, or Phase 3 should check the cache first (which will have same-block entries) and only fall back to the pre-fetched map for cross-batch entries.
- **Rejected**: Pre-fetching for all chunks at once before any transaction. This would work but requires holding all UTXO data in memory simultaneously for multi-chunk batches. Per-chunk pre-fetch is simpler and bounded.
