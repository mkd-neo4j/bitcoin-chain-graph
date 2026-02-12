# Code Review: Ingestion Loop Transaction Usage

**File**: `src/domain/ingestion.rs`
**Focus**: How `begin_transaction`/`commit_transaction`/`rollback_transaction` are called

---

## 1. Transaction Lifecycle: begin / commit / rollback

Transactions are used **only** in `ingest_blocks_batch()` (lines 560-619). The single-block `ingest_block()` method (lines 488-536) does **not** use transactions at all — each phase's write is auto-committed independently.

### Call Pattern (lines 588-614)

```rust
// Begin atomic transaction for this chunk
self.writer.begin_transaction().await?;

let chunk_result = self.process_batch_chunk(chunk, batch_idx, blocks_in_batch).await;

match chunk_result {
    Ok(()) => {
        self.writer.commit_transaction().await?;
    }
    Err(e) => {
        if let Err(rollback_err) = self.writer.rollback_transaction().await {
            tracing::error!(error = %rollback_err, "Rollback failed after chunk error");
        }
        return Err(e);
    }
}
```

**Key observations:**
- `begin_transaction` failure propagates immediately via `?` (no cleanup needed)
- `commit_transaction` failure also propagates via `?` — the transaction is left in an ambiguous state (no rollback attempted on commit failure)
- `rollback_transaction` failure is logged but does **not** replace the original error — the original chunk error is returned

## 2. What Happens Between begin and commit

`process_batch_chunk()` (lines 625-1023) runs **all 7 ingestion phases** within a single transaction:

| Phase | Operation | Writer Method(s) |
|-------|-----------|-------------------|
| 1 | Block nodes | `write_blocks_fast()` |
| 2 | Output nodes + UTXO cache | `write_outputs_fast()` or `write_outputs()` (BIP30) |
| 3 | Transaction nodes with amounts | `write_transactions_fast()` or `write_transactions()` (BIP30) |
| 3.5 | HAS_OUTPUT relationships | `write_has_output_relationships_fast()` |
| 4 | Input nodes + SPENDS | `write_inputs_fast()` or `write_inputs()` (BIP30) |
| 6 | PERFORMS + BENEFITS_TO | `write_performs()` + `write_benefits_to()` (8 parallel buckets) |
| 7 | UTXO cache cleanup | In-memory only (no writer call) |
| checkpoint | Update checkpoint | `update_checkpoint()` |

**Total writer calls per chunk**: 6-7 write calls + 1 checkpoint, all within a single transaction.

### Phase 6 Parallel Writes (lines 940-975)

Phase 6 is notable: it spawns **8 parallel `tokio::spawn` tasks**, each calling `writer.write_performs()` and `writer.write_benefits_to()` concurrently. These all execute within the same transaction context. This means the transaction state must be accessible from multiple concurrent tasks sharing `Arc<W>`.

## 3. Error Handling — When Does Rollback Get Called?

Rollback is called when `process_batch_chunk()` returns `Err`. Any `?` propagation within the 7 phases triggers this. Specific failure points:

- Any `write_*` call fails (Neo4j query error, connection loss)
- `get_many_or_fail()` UTXO cache miss (line 808) — returns `WriterError`
- Phase 6 parallel task panic or write failure (lines 971-974)

**Gap**: If `commit_transaction()` fails (line 597), no rollback is attempted. The transaction is left dangling — Neo4j will eventually time it out, but this could cause issues with connection pool exhaustion if the connection is reused.

**Gap**: If `begin_transaction()` succeeds but the **first** operation in `process_batch_chunk()` fails, rollback is correctly called. However, UTXO cache mutations (Phase 2 inserts, Phase 7 removals) are **not** rolled back — they are in-memory side effects that diverge from the database state after rollback.

## 4. Batch Chunking Logic

### Outer Loop (line 568)

```rust
for (batch_idx, chunk) in blocks.chunks(batch_size).enumerate() {
```

- `blocks` is the full set of blocks to ingest
- `batch_size` is configurable (recommended 100-1000 for backlog, 10-100 for real-time)
- Each chunk gets its own transaction (begin/commit/rollback)
- Parent hash validation occurs at each chunk boundary (line 576)

### Capacity Pre-allocation (lines 662-672)

Before processing, the code counts total transactions, outputs, and inputs across the chunk to pre-allocate vectors. This avoids reallocations during accumulation.

### Transaction Scope = One Chunk

One Neo4j transaction wraps exactly one chunk of `batch_size` blocks. If a batch of 1000 blocks uses `batch_size=100`, there will be 10 sequential transactions.

## 5. Retry Logic

**There is no retry logic at the batch level.** If a chunk fails:

1. The transaction is rolled back
2. The original error is returned immediately (`return Err(e)`)
3. The entire `ingest_blocks_batch()` call fails
4. No subsequent chunks are processed

Retry responsibility is left to the caller (CLI layer or the `resume` command, which can restart from the last checkpoint).

## 6. Summary of Findings

| Aspect | Current State |
|--------|---------------|
| Transaction scope | One chunk of `batch_size` blocks |
| Single-block path | No transactions used |
| Parallel writes within txn | Yes — Phase 6 spawns 8 concurrent tasks |
| Rollback on failure | Yes, with error logging |
| Rollback on commit failure | No — gap |
| UTXO cache consistency on rollback | No — in-memory state diverges |
| Retry logic | None — fail-fast, caller retries via checkpoint |
| Transaction nesting | None — flat begin/commit per chunk |
