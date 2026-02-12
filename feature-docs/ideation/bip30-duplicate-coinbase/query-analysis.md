# Query & Error Type Analysis for BIP30 Constraint Violations

## Fast vs Regular Query Paths

Two complete sets of write queries exist:

| Phase | Fast (CREATE) | Regular (MERGE) |
|-------|--------------|-----------------|
| Blocks | `CREATE_BLOCKS_FAST_QUERY` | `CREATE_BLOCKS_QUERY` |
| Outputs | `CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY` | `CREATE_OUTPUTS_QUERY` + `CREATE_LOCKED_TO_QUERY` |
| Transactions | `CREATE_TRANSACTIONS_FAST_QUERY` | `CREATE_TRANSACTIONS_QUERY` |
| HAS_OUTPUT | `CREATE_HAS_OUTPUT_FAST_QUERY` | `CREATE_HAS_OUTPUT_QUERY` |
| Inputs | `CREATE_INPUTS_FAST_QUERY` | `CREATE_INPUTS_QUERY` |
| PERFORMS | (same — already MERGE) | `CREATE_PERFORMS_BULK_QUERY` |
| BENEFITS_TO | (same — already MERGE) | `CREATE_BENEFITS_TO_BULK_QUERY` |

The fast queries use `CREATE` for node creation → constraint violation on duplicate unique keys.
The regular queries use `MERGE` → idempotent, handles duplicates via last-write-wins.

## Affected Phases for BIP30 Duplicates

The duplicate coinbase has the same txid, so it produces identical:
- `outputId` = `"{txid}:{vout}"` → **Phase 2 fails** (`CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY`)
- `txid` → **Phase 3 fails** (`CREATE_TRANSACTIONS_FAST_QUERY`)
- `inputId` = `"{txid}:0"` → **Phase 4 fails** (`CREATE_INPUTS_FAST_QUERY`)

Blocks are unaffected (different block hash). HAS_OUTPUT, PERFORMS, BENEFITS_TO use MATCH/MERGE.

## neo4rs Error Type for Constraint Violations

```rust
// neo4rs::Error enum
enum Error {
    Neo4j(Neo4jError),  // ← constraint violations land here
    // ... 18 other variants
}

// Neo4jError has:
impl Neo4jError {
    fn code(&self) -> &str;      // e.g. "Neo.ClientError.Schema.ConstraintValidationFailed"
    fn message(&self) -> &str;   // human-readable description
    fn kind(&self) -> Neo4jErrorKind;
}
```

**Constraint violation code**: `"Neo.ClientError.Schema.ConstraintValidationFailed"`

### Detection in Current Code

In `run_with_retry`, all neo4rs errors are wrapped as:
```rust
WriterError::QueryFailed(format!("{} failed (...): {}", operation_name, e))
```

The original `neo4rs::Error` is formatted into a string and the structured error info is lost. To match on constraint violations specifically, we'd need to inspect the `neo4rs::Error` before wrapping it.

## Call Chain

```
orchestrator.ingest_blocks_batch()
  → process_batch_chunk()
    → self.writer.write_outputs_fast()        // trait method
      → Neo4jWriter::write_outputs_fast()     // inherent impl
        → execute_batched()
          → run_with_retry()
            → self.graph.run(query)           // neo4rs
              → neo4rs::Error::Neo4j(...)     // constraint violation
```

## Where to Intercept

### Option A: At the orchestrator level (recommended)

In `process_batch_chunk`, check if any height in the chunk is a BIP30 height:

```rust
let has_bip30 = chunk.iter().any(|(h, _, _)| BIP30_DUPLICATE_HEIGHTS.contains(h));

if has_bip30 {
    // Use MERGE-based methods for this chunk
    self.writer.write_outputs(&output_data_batch).await?;
} else {
    // Use fast CREATE methods
    self.writer.write_outputs_fast(&output_data_batch).await?;
}
```

This must be done for outputs, transactions, AND inputs (3 phases).

**Pros**: No changes to writer layer, no error type inspection needed, uses existing tested MERGE queries.
**Cons**: Slight code duplication in the conditional branches.

### Option B: At the writer level with error inspection

Add a method like `write_outputs_fast_or_merge` that tries CREATE first, catches constraint violations, falls back to MERGE.

**Pros**: Transparent to orchestrator.
**Cons**: Requires extracting `neo4rs::Error` before it's stringified in `run_with_retry`. Retry logic would need refactoring. More complex.

### Option C: At the neo4rs error level

Modify `run_with_retry` to not retry constraint violations (they're not transient). Add a `WriterError::ConstraintViolation` variant.

**Pros**: More precise error handling.
**Cons**: Doesn't solve the core problem — the batch still fails. Still need to re-run with MERGE.

## Recommendation

**Option A** is the clear winner. It's the simplest, requires no writer-layer changes, and leverages existing MERGE queries that are already tested. The check `chunk.iter().any(|(h, _, _)| BIP30_DUPLICATE_HEIGHTS.contains(h))` is O(n) on a tiny constant array and adds negligible overhead.

The implementation would be localized to `process_batch_chunk` in `src/domain/ingestion.rs`, touching only the 3 phase call sites (outputs, transactions, inputs).

## UTXO Cache Consideration

The UTXO cache uses `HashMap::insert` semantics (overwrite on duplicate key). For BIP30 blocks, the duplicate coinbase outputs will overwrite the originals in cache. This is actually correct behavior — the later block's coinbase "replaces" the earlier one, and the earlier outputs were already spent (blocks 91722 and 91812 coinbase outputs were spent before blocks 91842 and 91880 duplicated them).
