# Rollback & Error Handling Analysis for BIP30 Constraint Violation

## Question

If we catch and ignore a constraint violation during `write_outputs_fast` for BIP30 duplicate blocks (91842, 91880), does the rest of the pipeline still work correctly?

## Current Architecture

### Batch Ingestion Flow (`ingest_blocks_batch`)

```
for each chunk of blocks:
    begin_transaction()
    process_batch_chunk(chunk)  // phases 1-7
    commit_transaction()        // or rollback on error
```

Inside `process_batch_chunk`:
1. Phase 1: `write_blocks_fast` (CREATE blocks)
2. Phase 2: `write_outputs_fast` (CREATE outputs) + populate UTXO cache concurrently
3. Phase 3: `write_transactions_fast` (CREATE transactions) — uses UTXO cache for amounts
4. Phase 3.5: `write_has_output_relationships_fast` (CREATE HAS_OUTPUT)
5. Phase 4: `write_inputs_fast` (CREATE inputs + SPENDS)
6. Phase 6: `write_performs` + `write_benefits_to` (MERGE — already idempotent)
7. Phase 7: Remove spent outputs from UTXO cache
8. Update checkpoint

### Error Propagation

Every phase uses `?` to propagate errors. If Phase 2 (`write_outputs_fast`) fails, `process_batch_chunk` returns `Err`, and the caller (`ingest_blocks_batch`) calls `rollback_transaction()` then returns the error up to `main.rs`.

### Rollback Logic (`sync_checkpoint_with_db`)

On resume, `sync_checkpoint_with_db` walks backwards from the highest DB block, checking `check_block_complete` (expected tx count == actual tx count). Incomplete blocks are rolled back via `rollback_block` (DETACH DELETE of inputs, outputs, transactions, block).

### Transaction Boundaries

**Critical finding**: `begin_transaction()` and `commit_transaction()` are `todo!()` in `Neo4jWriter`. This means each `execute_batched` call runs its own implicit auto-commit transactions. There is no atomic batch — each phase commits independently.

## Analysis: Is Catching the Constraint Violation Safe?

### The Problem

Block 91842 has a coinbase with the same txid as block 91722's coinbase. Block 91880 has a coinbase with the same txid as block 91812's coinbase. The `write_outputs_fast` query uses `CREATE` which fails on the uniqueness constraint for the `output_id` (format: `{txid}:{vout}`).

The outputs are batched with UNWIND — an entire batch of outputs across multiple blocks fails as one query, not just the duplicate row.

### What Happens at Each Layer

| Component | Impact of catching constraint error | Safe? |
|---|---|---|
| **Neo4j outputs** | The duplicate output already exists from the earlier block. CREATE fails for the whole UNWIND batch. We need to handle this at a finer granularity than "skip the error". | **NO — see below** |
| **UTXO cache** | Cache population runs concurrently with the write via `tokio::join!`. The cache insert is a HashMap overwrite — same key just overwrites. Cache is fine regardless. | **YES** |
| **Phase 3 (transactions)** | Transaction CREATE uses txid as unique key. The duplicate coinbase txid will also cause a constraint violation in `write_transactions_fast`. | **NO — also fails** |
| **Phase 3.5 (HAS_OUTPUT)** | Creates relationship from existing tx to existing output. If both already exist, CREATE on the relationship won't violate uniqueness (relationships don't have unique constraints). | **YES** |
| **Phase 4 (inputs)** | Coinbase inputs have `input_id = "{txid}:0"`. Duplicate txid → duplicate input_id → constraint violation. | **NO — also fails** |
| **Phase 6 (PERFORMS/BENEFITS_TO)** | Uses MERGE — idempotent, no constraint issues. | **YES** |
| **Phase 7 (cache cleanup)** | Just removes keys — idempotent. | **YES** |
| **Checkpoint** | Only updates after all phases succeed. | **YES** |

### Key Finding: It's Not Just Outputs

The constraint violation cascades through **three** phases:
1. Phase 2: `write_outputs_fast` — duplicate `output_id`
2. Phase 3: `write_transactions_fast` — duplicate `txid`
3. Phase 4: `write_inputs_fast` — duplicate `input_id`

### UNWIND Batch Granularity Problem

The `execute_batched` method chunks items by `self.batch_size` and runs each chunk as one UNWIND query. If a BIP30 block is in a batch with other blocks, the entire UNWIND batch fails — not just the duplicate row. This means catching the error at the `execute_batched` level would skip writing ALL outputs in that batch, including legitimate new outputs from non-duplicate transactions.

### Rollback/Resume Safety

The `sync_checkpoint_with_db` resume logic:
- Walks backwards from highest block, checking tx count matches expected
- Rolls back incomplete blocks
- Updates checkpoint to highest complete block

This is **sound** for crash recovery but **irrelevant** to the BIP30 fix because the error currently causes the entire ingestion to abort, not a partial write. The question is how to make the BIP30 blocks succeed in the first place, not how to recover from them.

## Conclusion

**Simply catching and ignoring the constraint violation error is NOT sufficient** because:

1. The error affects the entire UNWIND batch, not just the duplicate row
2. Three different phases (outputs, transactions, inputs) all have the same duplicate-key problem
3. The coinbase is just one transaction in the block — other transactions are new and must be written

### Recommended Approaches (in order of preference)

1. **Detect BIP30 block in batch → fall back to MERGE queries for that batch only**
   - Check if any height in the chunk is in `BIP30_DUPLICATE_HEIGHTS`
   - If yes, use the regular `write_outputs()`, `write_transactions()`, `write_inputs()` (which use MERGE) instead of the fast CREATE variants
   - All other batches continue using fast CREATE
   - Minimal code change, leverages existing MERGE queries

2. **Split the BIP30 block into its own single-block batch**
   - Before batching, check if any block is BIP30
   - Process BIP30 blocks as individual batches using MERGE queries
   - Process all other blocks normally with CREATE

3. **Use Cypher `CREATE ... ON CONFLICT DO NOTHING`** (if Neo4j supports it)
   - Not available in standard Neo4j Cypher as of Neo4j 5.x

4. **Pre-filter duplicate outputs from the UNWIND data**
   - Before sending to Neo4j, check if the coinbase txid already exists in the UTXO cache from an earlier block
   - If it does, skip that transaction's outputs/inputs from the batch data
   - Problem: the cache may have evicted the old entry (LRU), so this isn't reliable

### Approach 1 is safest because:
- MERGE queries already exist and are tested
- The BIP30 log warning already identifies these blocks
- Only 2 blocks in all of Bitcoin history are affected
- Performance impact is negligible (2 blocks out of 800k+)
