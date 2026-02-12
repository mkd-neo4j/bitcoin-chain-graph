# neo4rs Constraint Violation Error Analysis

## Summary

When the fast output CREATE query hits a duplicate `outputId` (BIP30 duplicate coinbase), Neo4j returns a constraint violation error. This document traces the exact error type through neo4rs 0.8.0 and identifies where to catch it.

## Neo4j Error Code for Constraint Violations

Neo4j returns error code: `Neo.ClientError.Schema.ConstraintValidationFailed`

## How neo4rs 0.8.0 Classifies This Error

The error flows through this chain:

1. **Neo4j server** returns a FAILURE message with `code: "Neo.ClientError.Schema.ConstraintValidationFailed"` and a `message` string.

2. **`Neo4jErrorKind::classify()`** (`src/errors.rs:115-155`) parses the dot-separated code:
   - `class = "ClientError"` → enters the `ClientError` match arm
   - `subclass = "Schema"` → falls through to `(Some(_), _)` catch-all at line 149
   - **Result: `Neo4jErrorKind::Client(Neo4jClientErrorKind::Other)`**

3. **`neo4rs::Error::Neo4j(Neo4jError { kind, code, message })`** is constructed.

4. The `Display` impl formats it as: `Neo4j error 'Neo.ClientError.Schema.ConstraintValidationFailed': <message>`

## Current Code Path: Where the Error Surfaces

The call chain for fast output writes:

```
IngestionOrchestrator::ingest_outputs()          # ingestion.rs:710
  → writer.write_outputs_fast()                   # traits.rs (trait method)
    → Neo4jWriter::write_outputs_fast()           # neo4j/mod.rs:292
      → self.execute_batched(...)                 # neo4j/mod.rs:117
        → self.run_with_retry(...)                # neo4j/mod.rs:180
          → self.graph.run(query)                 # neo4rs call
            → Err(neo4rs::Error::Neo4j(...))      # ← constraint violation here
```

In `run_with_retry()` (neo4j/mod.rs:191-245):
- The `neo4rs::Error` is caught at line 199: `Ok(Err(e))`
- It's wrapped into `WriterError::QueryFailed(format!(...))` at line 201
- **CRITICAL**: `WriterError::QueryFailed` returns `is_retryable() == true`, so the error is **retried** before propagating — constraint violations are NOT transient and should NOT be retried

The error message string contains the original neo4rs error via `format!("{}", e)`, which includes the Neo4j error code. So the string will contain `Neo.ClientError.Schema.ConstraintValidationFailed`.

## The Problem with Current Retry Logic

A constraint violation (`Neo.ClientError.Schema.ConstraintValidationFailed`) is:
- **NOT retryable** — retrying will always produce the same error
- Currently classified as `WriterError::QueryFailed` which IS retryable
- This means on BIP30 blocks, the system will retry `max_retries` times before finally returning the error

neo4rs itself classifies this as `Neo4jClientErrorKind::Other`, and `can_retry()` returns `false` for `Client(Other)`. But our code doesn't check neo4rs retryability — it wraps everything in `WriterError::QueryFailed`.

## Where to Catch the Error

### Option A: In `run_with_retry()` — check before retrying (recommended)

Before wrapping in `WriterError::QueryFailed`, inspect the `neo4rs::Error` to detect constraint violations. If it's a constraint violation, return immediately without retrying.

```rust
// In run_with_retry, line 199-219:
Ok(Err(e)) => {
    // Check if this is a constraint violation (not retryable)
    if is_constraint_violation(&e) {
        return Err(WriterError::ConstraintViolation(format!(...)));
    }
    // ... existing retry logic
}
```

To detect constraint violations from `neo4rs::Error`:
```rust
fn is_constraint_violation(e: &neo4rs::Error) -> bool {
    match e {
        neo4rs::Error::Neo4j(neo4j_err) => {
            neo4j_err.code() == "Neo.ClientError.Schema.ConstraintValidationFailed"
        }
        _ => false,
    }
}
```

### Option B: In `write_outputs_fast()` — wrap execute_batched with error handling

Catch the error returned from `execute_batched()` and check if it's a BIP30 constraint violation:

```rust
pub async fn write_outputs_fast(&self, outputs: &[OutputData]) -> Result<()> {
    match self.execute_batched(...).await {
        Ok(()) => Ok(()),
        Err(WriterError::QueryFailed(msg))
            if msg.contains("ConstraintValidationFailed") => {
            // BIP30 duplicate — log and continue
            tracing::warn!("Constraint violation in output write (possible BIP30 duplicate)");
            Ok(())
        }
        Err(e) => Err(e),
    }
}
```

### Option C: In the orchestrator (`ingest_outputs()`) — catch at the call site

Catch the error in `ingestion.rs:710` where `write_outputs_fast` is called. This is the highest level and easiest to scope to specific block heights.

## Recommendation

**Option A is best** because:
1. It avoids wasted retry attempts on a deterministic failure
2. It introduces a new `WriterError::ConstraintViolation` variant that's semantically correct
3. The caller (orchestrator) can then match on this specific variant and decide whether to skip (BIP30) or fail

**Option C is the simplest** for a scoped fix because:
1. The orchestrator knows the current block height and can check against BIP30_DUPLICATE_HEIGHTS
2. No changes to the writer layer needed
3. But it still wastes retries before the error surfaces

**Best approach: combine A + C**:
- Add `ConstraintViolation` variant to `WriterError` (not retryable)
- Detect it in `run_with_retry()` to avoid wasted retries
- In the orchestrator, match on `ConstraintViolation` and skip only for known BIP30 heights

## Affected Queries

The constraint violation can occur in these fast queries during BIP30 duplicate blocks:

| Query | Constraint | Duplicate field |
|-------|-----------|----------------|
| `CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY` | Output.outputId uniqueness | `<dup_txid>:0` |
| `CREATE_TRANSACTIONS_FAST_QUERY` | Transaction.txid uniqueness | `<dup_txid>` |
| `CREATE_HAS_OUTPUT_FAST_QUERY` | No uniqueness constraint on rels | N/A (won't fail) |
| `CREATE_INPUTS_FAST_QUERY` | Input.inputId uniqueness | `<dup_txid>:<idx>` |

Phase 2 (outputs) runs first, so the constraint violation will hit `CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY` before any other query.

## Key neo4rs Types to Import

```rust
use neo4rs::Error as Neo4rsError;  // Already available via neo4rs crate

// Pattern matching:
match &neo4rs_error {
    neo4rs::Error::Neo4j(neo4j_err) => {
        let code: &str = neo4j_err.code();     // "Neo.ClientError.Schema.ConstraintValidationFailed"
        let msg: &str = neo4j_err.message();   // Human-readable constraint details
        let kind: Neo4jErrorKind = neo4j_err.kind(); // Client(Other)
    }
    _ => { /* not a Neo4j server error */ }
}
```

## BIP30 Duplicate Heights (for reference)

- Block 91,722 and 91,880 share coinbase txid `d5d27987...`
- Block 91,812 and 91,842 share coinbase txid `e3bf3d07...`
- The SECOND occurrence of each pair (91,880 and 91,842) is where the constraint violation hits
