---
title: Re-calibrate Block Memory Estimator for Neo4j Heap Cost
status: testing
priority: high
affected-files:
  - src/domain/ingestion.rs
---

# Re-calibrate Block Memory Estimator for Neo4j Heap Cost

## Summary

The `estimate_block_memory()` function and its constants (`BYTES_PER_BLOCK=500`, `BYTES_PER_TX=400`, `BYTES_PER_OUTPUT=550`, `BYTES_PER_INPUT=550`) were calibrated for Rust struct sizes, not Neo4j transaction heap cost. Neo4j's per-entity overhead in a write transaction is significantly higher — each node creation costs ~2-4KB of transaction state (property storage, index updates, lock entries, undo log), and each relationship costs a similar amount.

At height ~207K, 200 blocks estimated at ~101MB (Rust-side) produced ~18M Neo4j nodes/relationships in a single transaction, causing stop-the-world GC pauses (3.5s, 8.7s, 6.1s) that prevented the commit from completing. The `max_transaction_memory_mb` config (default 600) is meant to cap Neo4j transaction memory, but with the wrong constants it massively underestimates the actual cost.

This feature re-calibrates the constants to reflect Neo4j heap cost per entity. The estimator must also account for relationships (NEXT_BLOCK, HAS_OUTPUT, SPENDS, LOCKED_TO, PERFORMS, BENEFITS_TO) which are created alongside nodes but not currently counted.

## Acceptance Criteria

1. GIVEN `estimate_block_memory()` WHEN called on a block with T transactions, O outputs, and I non-coinbase inputs THEN the estimate accounts for both nodes AND relationships created in all 7 phases: 1 Block node + 1 NEXT_BLOCK rel, T Transaction nodes + T PERFORMS rels, O Output nodes + O HAS_OUTPUT rels + O LOCKED_TO rels + O BENEFITS_TO rels, I Input nodes + I SPENDS rels

2. GIVEN the memory estimation constants WHEN inspected THEN each constant reflects Neo4j transaction heap cost per entity (node or relationship), not Rust struct size — expected range is 1500-4000 bytes per entity based on Neo4j internals (property count, index updates, transaction state overhead)

3. GIVEN `max_transaction_memory_mb` with default value WHEN the default is inspected in `IngestionOrchestrator::new()` THEN it is set to a value that prevents the production incident (200 blocks at height ~207K must split into multiple chunks, not one)

4. GIVEN the block at height 207600 with approximately 90 transactions, 270 outputs, and 270 inputs WHEN `estimate_block_memory()` is called THEN the result multiplied by 200 (blocks in the batch) exceeds `max_transaction_memory_mb × 1024 × 1024`, causing `compute_adaptive_chunks()` to produce multiple chunks

5. GIVEN the existing `compute_adaptive_chunks()` function and `ingest_blocks_batch()` loop WHEN the constants are updated THEN no changes are needed to the chunking algorithm or the transaction wrapping logic — only the constants and default change

6. GIVEN early Bitcoin blocks (height 0-170000, ~1-5 transactions per block) WHEN `estimate_block_memory()` is called on each THEN a batch of 200 such blocks still fits within the default `max_transaction_memory_mb` as a single chunk (the re-calibration should not make early blocks unnecessarily slow)

## Edge Cases

- Modern blocks (post-SegWit, ~3000 transactions) — should produce very small chunks (1-5 blocks per transaction), which is correct and expected
- Blocks with many OP_RETURN outputs (no address, no LOCKED_TO relationship) — estimator may slightly overcount; acceptable since it's an upper bound
- Coinbase-only blocks (height 0) — 1 transaction, 1 output, 0 non-coinbase inputs; estimate should still be reasonable (not zero)

## Out of Scope

- **Do NOT modify `compute_adaptive_chunks()` or the chunking algorithm** — the algorithm is correct; only its input estimates are wrong.
- **Do NOT modify `src/writer/neo4j/mod.rs`** — no writer changes needed. The transaction wrapping in `ingest_blocks_batch()` already handles multiple chunks correctly.
- **Do NOT add a separate `max_blocks_per_commit` config** — the memory estimator should be accurate enough that a block count cap is unnecessary. Adding a second knob creates confusing interactions with `max_transaction_memory_mb`.
- **Do NOT move UTXO fallback lookups** — that is a separate feature (`utxo-lookup-outside-transaction.md`).

## Technical Notes

- Neo4j transaction memory per entity varies by property count and index coverage. A node with 5 indexed string properties costs more than a node with 2 integer properties. The constants should be conservative (overestimate) since underestimation caused the production incident.
- The 7 phases create the following entity counts per block: 1 Block + 1 NEXT_BLOCK + T Transactions + T PERFORMS + O Outputs + O HAS_OUTPUT + O LOCKED_TO + O BENEFITS_TO + I Inputs + I SPENDS. Total entities ≈ 1 + 1 + 2T + 4O + 2I. With ~270 outputs and inputs per block at height 207K, that's ~1600 entities per block × 200 blocks = ~320K entities per transaction.
- The formula should become: `BYTES_PER_BLOCK_NODE + BYTES_PER_NEXT_BLOCK_REL + T * (BYTES_PER_TX_NODE + BYTES_PER_PERFORMS_REL) + O * (BYTES_PER_OUTPUT_NODE + BYTES_PER_HAS_OUTPUT_REL + BYTES_PER_LOCKED_TO_REL + BYTES_PER_BENEFITS_TO_REL) + I * (BYTES_PER_INPUT_NODE + BYTES_PER_SPENDS_REL)`. For simplicity, a single `BYTES_PER_NODE` and `BYTES_PER_REL` constant pair may be sufficient if property counts are similar across entity types.
- **Rejected**: Using a fixed `max_blocks_per_commit` instead of re-calibrating. This would work but defeats the purpose of the adaptive chunking system — early blocks would be unnecessarily slow, and modern blocks might still exceed limits if the cap is too high.
- **Rejected**: Querying Neo4j's `dbms.memory.transaction.max` at startup to auto-set the budget. This adds complexity and a dependency on Neo4j admin API access. The config field is explicit and sufficient.
