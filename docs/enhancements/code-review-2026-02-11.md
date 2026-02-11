# Code Review Findings — 2026-02-11

Three-reviewer audit focused on data corruption/rollback safety, performance, and test coverage.

---

## Data Safety

### 1. No Neo4j Transaction Boundaries (CRITICAL)

**Files**: `src/writer/neo4j/mod.rs:117-173`, `src/domain/ingestion.rs:485-536`

Every `execute_batched()` and `graph.run()` call is auto-committed. The 7-phase `ingest_block()` pipeline has no transactional wrapper. A crash between phases leaves orphaned data (e.g., output nodes with no parent transaction).

`ingest_blocks_batch()` writes hundreds of blocks across 7 phases with no transaction boundary, checkpointing only at batch end.

**Mitigant**: `sync_checkpoint_with_db()` rolls back incomplete blocks on resume, but only checks transaction count — not missing inputs, outputs, or relationships.

**Recommendation**: Wrap each batch chunk's phases in a Neo4j transaction via `graph.start_txn()` / `txn.commit()`.

### 2. `check_block_complete` Silently Swallows Errors (HIGH)

**File**: `src/writer/neo4j/mod.rs:840-841`

```rust
let expected: i64 = row.get("expectedTxCount").unwrap_or(0);
let actual: i64 = row.get("actualTxCount").unwrap_or(0);
```

Missing or wrong-typed fields silently return 0. Safe direction (block appears incomplete, gets rolled back) but hides real errors.

**Recommendation**: Use proper error propagation: `row.get("expectedTxCount").map_err(|e| ...)?`

### 3. `create_checkpoint` Non-Atomic DELETE+CREATE (MEDIUM)

**File**: `src/writer/neo4j/mod.rs:605-618`

Two separate queries: DELETE then CREATE. Crash between them = no checkpoint = re-ingest from genesis.

**Recommendation**: Use a single MERGE-based query or wrap in a Neo4j transaction.

### 4. Crash Recovery Only Checks Transaction Count (MEDIUM)

**File**: `src/domain/ingestion.rs:261-338`

`sync_checkpoint_with_db()` checks block completeness by comparing `txCount` vs actual transaction nodes. A block could have all transactions (Phase 3) but be missing HAS_OUTPUT (Phase 3.5), inputs (Phase 4), or relationships (Phase 6).

**Recommendation**: Add completeness checks for input/output counts, or rely on Neo4j transactions (finding #1) to make this moot.

### 5. UTXO Cache Not Rolled Back on Failure (MEDIUM)

**File**: `src/domain/ingestion.rs:498-521`

Phase 2 populates the UTXO cache concurrently with writing outputs. If a later phase fails, cache contains entries for the failed block. On retry, stale entries could produce incorrect amount calculations.

**Recommendation**: Evict cache entries for the current block on phase failure, or accept as known limitation since re-ingestion overwrites them.

### 6. Parallel Phase 6 Writes — Neo4j Deadlock Risk (MEDIUM)

**File**: `src/domain/ingestion.rs:866-903`

Phase 6 partitions PERFORMS/BENEFITS_TO into 8 address-hashed buckets for parallel writes. Neo4j can deadlock on internal page-level locks. The `run_with_retry` in the writer should handle this if deadlock errors are classified as `QueryFailed`.

**Recommendation**: Verify Neo4j deadlock errors are retryable. Currently they should be, so likely fine in practice.

---

## Performance

### 7. Phase 6 Re-derives Addresses (WARNING)

**File**: `src/domain/ingestion.rs:~1215`

`write_simplified_layer_rust` calls `OutputData::from_output()` to re-extract addresses from raw transaction data, duplicating work already done in Phase 2.

**Recommendation**: Pass already-computed `OutputData` vec from Phase 2 into Phase 6.

### 8. `fill_percentage` Acquires All 16 Shard Locks Twice (WARNING)

**File**: `src/domain/utxo/cache.rs:652-661`

First loop counts entries, second counts capacities. Each acquires all 16 locks.

**Recommendation**: Single loop collecting both values per shard.

### 9. MERGE Output Query Has Redundant ON CREATE/ON MATCH (CONVENTION)

**File**: `src/writer/neo4j/queries.rs` — `CREATE_OUTPUTS_QUERY`

`ON CREATE SET` and `ON MATCH SET` have identical property lists. Simplify to a single `SET` after MERGE.

### 10. `run_live_ingestion` Is ~500 Lines (SUGGESTION)

**File**: `src/main.rs`

Handles RPC catchup, ZMQ subscription, reorg detection, and graceful shutdown inline. Hard to test or modify individually.

**Recommendation**: Extract into `rpc_catchup()`, `zmq_loop()`, `handle_reorg()`.

### 11. Phase 6 Bucket Count Hardcoded to 8 (SUGGESTION)

**File**: `src/domain/ingestion.rs`

For very large blocks, more buckets could improve throughput.

**Recommendation**: Make configurable or auto-scale based on block size.

---

## Dead Code to Remove

| Item | Location | Notes |
|------|----------|-------|
| `mark_output_spent` trait method | `src/writer/traits.rs:226-230` | Never called from domain layer |
| `mark_output_spent` Neo4jWriter impl | `src/writer/neo4j/mod.rs:586-603` | Dead code |
| `mark_output_spent` MockWriter impl | `src/writer/mock.rs:233-248` | Dead code |
| `MARK_OUTPUT_SPENT_QUERY` constant | `src/writer/neo4j/queries.rs:255-261` | Unreferenced |
| `with_buffer_size` deprecated method | `src/parser/block_file.rs` | No callers |
| `#[allow(dead_code)]` fields | `src/parser/block_index.rs`, `src/parser/single_block_loader.rs` | Need TODO(#issue) or removal |
| `hex` in `[dev-dependencies]` | `Cargo.toml` | Already in `[dependencies]` |

---

## Test Coverage Gaps

### Critical

1. **`sync_checkpoint_with_db()` crash recovery** — completely untested. Scenarios: crash mid-batch with partial writes, all blocks beyond checkpoint incomplete, empty DB with stale checkpoint.
2. **`get_many_with_fallback()` missing UTXO detection** — partial Neo4j results, all keys missing from both cache and DB.
3. **Rollback + UTXO cache consistency** — rollback N blocks, verify cache state, re-ingest and verify correct amounts.
4. **`ingest_block` partial failure** — mock writer fails on `write_inputs` after earlier phases succeed, verify recovery path.

### High

5. **`WriterError::is_retryable()`** — assert each variant returns correct retryable status.
6. **MockWriter `rollback_block` spent-status** — currently doesn't track SPENDS relationships, so rollback tests can't validate real behavior.
7. **BIP30 duplicate transaction handling** — simulate blocks with duplicate txids at heights 91842/91880.
8. **`check_block_complete()` edge cases** — block with 0 transactions, tx_count mismatch.
9. **`lookup_outputs_batch`** — batch with mix of found/not-found outputs.

### Medium

10. **Config validation** — missing required fields, invalid TOML, empty neo4j.uri.
11. **`UtxoCache::new(0)`** — `#[should_panic]` test.
12. **`partition_performs_by_address` / `partition_benefits_by_address`** — deterministic bucket assignment, even distribution.
13. **`UtxoKey::from_hex_txid` with invalid hex** — returns None.

---

## What Is Well-Covered

- All 7 ingestion phases via MockWriter (genesis through 250 blocks)
- Domain model conversions (BlockData, TransactionData, OutputData, InputData)
- Address extraction for all 7 script types + testnet + malformed
- UTXO cache: insert, get, remove, LRU eviction, batch ops, concurrency, prewarm, persistence with CRC
- Checkpoint lifecycle: create, update, resume, status transitions
- Reorg detection: parent hash validation, rollback, rollback+reingest
- Phase ordering (outputs before transactions)
- Transaction amount balance (fee = input - output)
