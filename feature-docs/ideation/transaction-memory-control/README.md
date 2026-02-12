---
feature: transaction-memory-control
status: complete
created: 2026-02-12
---

# Ideation: Transaction Memory Control

This folder is a workspace for exploring and shaping a feature before it becomes a formal feature doc.

## How to Use

Add files here as you think through the feature. Common artifacts:

- **Code reviews** — Analysis of existing code that this feature will touch
- **Research notes** — How other projects solve this, API docs, trade-offs
- **Design sketches** — Data flow diagrams, component trees, schema changes
- **Spike results** — Quick experiments to validate an approach
- **Conversation logs** — Key decisions and reasoning captured from Claude sessions

There are no rules about file names or formats. Use whatever helps you think.

## When You're Ready

When the feature is clear enough to write testable acceptance criteria:

1. Say "create the feature" in your current session, or source `feature-docs/new-feature.md`
2. Claude will read everything in this folder and draft a feature doc
3. Review and refine the draft
4. The final doc is saved to `feature-docs/ready/<feature-name>.md`
5. This README's status is updated to `complete`
6. Kick off the test-writer: `@test-writer Pick up feature-docs/ready/<feature-name>.md`

This folder stays as an archive of the thinking that led to the feature doc.

## Progress

### 2026-02-12 — Initial exploration
- **Summary**: Neo4j transaction memory explodes on larger blocks (post-2017). Current config has too many interrelated settings (ingestion/neo4j/batch/blocks) that are hard to reason about. User wants ONE setting that controls everything and prevents memory blowup.
- **Open questions**:
  - What are all the current settings that affect transaction size?
  - How does data volume scale with block height (pre-SegWit vs post-SegWit)?
  - Where exactly does memory accumulate in a transaction?
  - What's the right single knob — block count, transaction count, output count, or memory estimate?
  - Can we make it adaptive (auto-adjust) vs fixed?

### 2026-02-12 — Deep codebase analysis complete
- **What we did**: 3 parallel agents reviewed all config settings, ingestion loop, Neo4j write paths, domain models, query constants, and block data scaling
- **Key findings**:
  - 4 ghost/unused config fields: `max_batch_memory_mb`, `checkpoint_interval`, `utxo_cache_snapshot_interval`, plus `batch_size` is passed redundantly
  - Only ONE setting actually controls transaction memory: `ingestion.batch_size` (blocks per txn)
  - Entity counts are already computed in `process_batch_chunk()` — we just need to move counting BEFORE chunking
  - Memory scales linearly: ~3,300 bytes × transactions_per_block
  - Modern blocks: ~8-10 MB/block. Early blocks: ~16 KB/block. 1000x difference.
- **Decisions made**:
  - Adaptive batching (Option 4) — dynamically size chunks based on memory estimate
  - Keep CREATE queries (no MERGE performance penalty)
  - Keep single-transaction atomicity per chunk
  - MERGE rejected: huge performance penalty at scale
  - Cleanup-on-resume rejected: complex, error-prone DETACH DELETE
  - Per-phase commits rejected: breaks atomicity, needs MERGE or cleanup
- **Artifacts**: code-review.md, api-design.md

### 2026-02-12 — API design complete
- **What we did**: Designed adaptive chunking algorithm, memory estimator, config changes, and exact code insertion points
- **Decisions made**:
  - ONE knob: `max_transaction_memory_mb` (default 600)
  - Remove: `batch_size`, `checkpoint_interval`, `max_batch_memory_mb`, `utxo_cache_snapshot_interval`
  - Keep: `neo4j.write_batch_size` (UNWIND chunk size, separate concern)
  - Insert point: `ingest_blocks_batch()` line 603, replace `blocks.chunks(batch_size)`
  - Estimator: count txs/outputs/inputs from parsed Block, multiply by ~500 bytes each
  - Safety: always at least 1 block per chunk (can't split a block)
  - Logging: each chunk logs block count, entity counts, estimated MB
- **Open questions**: None — ready to create feature doc
