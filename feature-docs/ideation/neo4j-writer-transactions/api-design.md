# API Design: Neo4jWriter Transaction Methods

## Struct Change

```rust
use tokio::sync::Mutex as TokioMutex;

pub struct Neo4jWriter {
    graph: Arc<Graph>,
    batch_size: usize,
    max_retries: usize,
    query_timeout: Duration,
    active_txn: TokioMutex<Option<Txn>>,  // NEW — tokio Mutex for await-safety
}
```

**Why `tokio::sync::Mutex`**: `Txn::run(&mut self)` is async. A `std::sync::Mutex` guard is `!Send` and cannot be held across `.await`. The `tokio::sync::Mutex` guard is `Send`, making it safe to hold while awaiting `txn.run()`. This is a targeted exception to the project convention — the other fields remain unchanged.

**Why `Option<Txn>`**: `Txn::commit(self)` and `Txn::rollback(self)` consume the Txn by value. We must `.take()` it out of the Option to transfer ownership.

---

## Data Flow: Two Execution Paths

```
                    ┌──────────────────────────────┐
                    │  write_blocks_fast()          │
                    │  write_outputs_fast()         │
                    │  write_transactions_fast()    │
                    │  write_inputs_fast()          │
                    │  write_has_output_rels_fast() │
                    │  write_performs()             │
                    │  write_benefits_to()          │
                    │  update_checkpoint()          │
                    │  mark_output_spent()          │
                    └────────────┬─────────────────┘
                                 │
                                 ▼
                    ┌────────────────────────────┐
                    │     execute_query()  NEW    │
                    │  (single dispatch point)    │
                    └────────────┬───────────────┘
                                 │
                    ┌────────────┴───────────────┐
                    │                            │
              active_txn.lock()            active_txn.lock()
              txn is Some(_)               txn is None
                    │                            │
                    ▼                            ▼
         ┌──────────────────┐       ┌──────────────────────┐
         │  TRANSACTIONAL   │       │    AUTO-COMMIT        │
         │                  │       │                       │
         │  txn.run(query)  │       │  run_with_retry {     │
         │  no retry        │       │    timeout {          │
         │  no timeout*     │       │      graph.run(query) │
         │  fail-fast       │       │    }                  │
         │                  │       │    exponential backoff │
         │                  │       │  }                    │
         └──────────────────┘       └───────────────────────┘

  * Neo4j server-side txn timeout applies (dbms.transaction.timeout)
```

### Why No Retries in Transactional Mode

A failed query inside a Neo4j transaction puts the server-side transaction in an **aborted** state. Subsequent queries on the same Txn will fail with "transaction has been terminated." The only valid recovery is rollback + retry the entire batch. The ingestion loop already handles this via checkpoint + resume.

### Why No Client-Side Timeout in Transactional Mode

The `tokio::time::timeout` wrapper in `run_with_retry` would cancel the future mid-flight, leaving the Txn in an unknown state. Server-side `dbms.transaction.timeout` is the appropriate safeguard. If a transactional query hangs, the server will terminate the transaction, and `txn.run()` will return an error that propagates to `process_batch_chunk()`, which triggers rollback.

---

## New Method: `execute_query`

This is the **single dispatch point** replacing direct `self.graph.run(q)` calls:

```rust
/// Execute a Cypher query, routing through the active transaction if one exists.
///
/// - With active transaction: runs via `txn.run()` (no retry, fail-fast)
/// - Without transaction: runs via `graph.run()` through `run_with_retry` (timeout + backoff)
async fn execute_query(
    &self,
    q: Query,
    operation_name: &str,
    batch_num: usize,
    total_batches: usize,
    record_count: usize,
) -> Result<()> {
    let mut txn_guard = self.active_txn.lock().await;

    if let Some(ref mut txn) = *txn_guard {
        // Transactional path: no retry, no client timeout
        txn.run(q).await.map_err(|e| {
            WriterError::TransactionFailed(format!(
                "{} failed in transaction (batch {}/{}, {} records): {}",
                operation_name, batch_num, total_batches, record_count, e
            ))
        })
    } else {
        // Auto-commit path: drop the lock, use existing retry logic
        drop(txn_guard);
        self.run_with_retry(
            operation_name,
            || {
                let q = q.clone();  // Query must be Clone (see note below)
                async { self.graph.run(q).await }
            },
            batch_num,
            total_batches,
            record_count,
        )
        .await
    }
}
```

**Problem: Query is not Clone.** The `run_with_retry` closure is `Fn()` (re-invocable), so it needs to reconstruct the Query on each attempt. The current code already does this — the closure captures `query_str`, `param_name`, and `bolt_data` by reference and rebuilds the Query each time.

This means `execute_query` as written above won't work for the auto-commit path with retries. We need a different approach.

---

## Revised Design: Modify `execute_batched`

Instead of a single `execute_query`, modify `execute_batched` to be transaction-aware:

```rust
async fn execute_batched<T, F>(
    &self,
    items: &[T],
    query_str: &str,
    param_name: &str,
    operation_name: &str,
    convert: F,
) -> Result<()>
where
    F: Fn(&[T]) -> Vec<BoltType>,
{
    if items.is_empty() {
        return Ok(());
    }

    let total_batches = items.len().div_ceil(self.batch_size);

    for (i, chunk) in items.chunks(self.batch_size).enumerate() {
        let bolt_data = convert(chunk);
        let batch_num = i + 1;
        let start = std::time::Instant::now();

        // Check if we're in a transaction
        let in_txn = self.active_txn.lock().await.is_some();

        if in_txn {
            // TRANSACTIONAL: run through txn, no retry
            let q = query(query_str).param(param_name, bolt_data.as_slice());
            let mut txn_guard = self.active_txn.lock().await;
            if let Some(ref mut txn) = *txn_guard {
                txn.run(q).await.map_err(|e| {
                    WriterError::TransactionFailed(format!(
                        "{} failed in transaction (batch {}/{}, {} records): {}",
                        operation_name, batch_num, total_batches, chunk.len(), e
                    ))
                })?;
            }
        } else {
            // AUTO-COMMIT: existing retry path
            self.run_with_retry(
                operation_name,
                || {
                    let q = query(query_str).param(param_name, bolt_data.as_slice());
                    async { self.graph.run(q).await }
                },
                batch_num,
                total_batches,
                chunk.len(),
            )
            .await?;
        }

        tracing::debug!(
            operation = operation_name,
            batch = batch_num,
            total_batches,
            records = chunk.len(),
            elapsed_ms = start.elapsed().as_millis() as u64,
            transactional = in_txn,
            "Batch write complete"
        );
    }

    Ok(())
}
```

**Note on double-lock**: We lock once to check `is_some()`, drop the guard, build the query, then lock again to run. This avoids holding the lock while building bolt_data. The check-then-act is safe because only the ingestion loop controls transaction lifecycle (single-threaded control flow for begin/commit/rollback).

---

## Phase 6 Parallelism Under Transactions

```
Phase 6 today (auto-commit):
  ┌──────────┐ ┌──────────┐ ┌──────────┐     ┌──────────┐
  │ Bucket 0 │ │ Bucket 1 │ │ Bucket 2 │ ... │ Bucket 7 │
  │ spawn    │ │ spawn    │ │ spawn    │     │ spawn    │
  │ graph.run│ │ graph.run│ │ graph.run│     │ graph.run│
  │ (pool)   │ │ (pool)   │ │ (pool)   │     │ (pool)   │
  └──────────┘ └──────────┘ └──────────┘     └──────────┘
       │            │            │                 │
       ▼            ▼            ▼                 ▼
   8 parallel connections from pool = true parallelism


Phase 6 under transaction:
  ┌──────────┐ ┌──────────┐ ┌──────────┐     ┌──────────┐
  │ Bucket 0 │ │ Bucket 1 │ │ Bucket 2 │ ... │ Bucket 7 │
  │ spawn    │ │ spawn    │ │ spawn    │     │ spawn    │
  │          │ │          │ │          │     │          │
  └────┬─────┘ └────┬─────┘ └────┬─────┘     └────┬─────┘
       │            │            │                 │
       └────────────┴────────────┴─────────────────┘
                           │
                    active_txn.lock()
                    (serialized access)
                           │
                           ▼
                  ┌──────────────────┐
                  │ Single Txn       │
                  │ Single connection│
                  │ Serial execution │
                  └──────────────────┘
```

**Impact**: Phase 6 loses parallelism when inside a transaction. This is **unavoidable** — a neo4rs `Txn` owns a single connection, and `run(&mut self)` requires exclusive access.

**Mitigation**: The 8 spawned tasks still execute — they just contend on the `active_txn` lock and serialize. No code change needed in ingestion.rs. The performance cost is bounded: Phase 6 writes PERFORMS and BENEFITS_TO relationships, which are typically fast UNWIND operations.

**Alternative considered and rejected**: Using 8 separate transactions (one per bucket). This breaks atomicity — if bucket 3 fails, buckets 0-2 are already committed. Not worth the complexity.

---

## Transaction Method Implementations

### begin_transaction

```rust
async fn begin_transaction(&self) -> Result<()> {
    let mut guard = self.active_txn.lock().await;
    if guard.is_some() {
        return Err(WriterError::TransactionFailed(
            "Cannot begin: transaction already active".into(),
        ));
    }
    let txn = self.graph.start_txn().await.map_err(|e| {
        WriterError::TransactionFailed(format!("BEGIN failed: {}", e))
    })?;
    *guard = Some(txn);
    tracing::debug!("Neo4j explicit transaction started");
    Ok(())
}
```

**Guard against double-begin**: Unlike MockWriter which silently overwrites, Neo4jWriter returns an error. Starting a second transaction without committing the first would leak the first Txn's connection.

### commit_transaction

```rust
async fn commit_transaction(&self) -> Result<()> {
    let txn = self.active_txn.lock().await.take().ok_or_else(|| {
        WriterError::TransactionFailed("Cannot commit: no active transaction".into())
    })?;
    txn.commit().await.map_err(|e| {
        WriterError::TransactionFailed(format!("COMMIT failed: {}", e))
    })?;
    tracing::debug!("Neo4j explicit transaction committed");
    Ok(())
}
```

**`.take()` transfers ownership**: The `Txn` is moved out of the Option, then `commit(self)` consumes it. On success, the Option is left as `None`. On failure, the Txn is dropped (implicit server-side rollback).

### rollback_transaction

```rust
async fn rollback_transaction(&self) -> Result<()> {
    let txn = self.active_txn.lock().await.take().ok_or_else(|| {
        WriterError::TransactionFailed("Cannot rollback: no active transaction".into())
    })?;
    txn.rollback().await.map_err(|e| {
        WriterError::TransactionFailed(format!("ROLLBACK failed: {}", e))
    })?;
    tracing::debug!("Neo4j explicit transaction rolled back");
    Ok(())
}
```

---

## WriterError Addition

```rust
#[derive(Error, Debug, Clone)]
pub enum WriterError {
    // ... existing variants ...

    /// An explicit transaction operation failed (begin, commit, rollback, or
    /// a query within a transaction). Not retryable — caller must rollback
    /// the entire batch and retry from the last checkpoint.
    #[error("Transaction failed: {0}")]
    TransactionFailed(String),
}
```

`is_retryable()` returns `false` for `TransactionFailed`.

---

## Methods That Also Need Transaction Routing

Beyond the 5 `write_*_fast` methods (which all go through `execute_batched`), these methods also run within the transaction scope during `process_batch_chunk`:

| Method | Current path | Change needed |
|--------|-------------|---------------|
| `write_performs()` | `execute_batched` | Covered by `execute_batched` change |
| `write_benefits_to()` | `execute_batched` | Covered by `execute_batched` change |
| `update_checkpoint()` | Direct `self.graph.run()` | Needs transaction routing |
| `mark_output_spent()` | Direct `self.graph.run()` | Needs transaction routing |

These two methods (`update_checkpoint`, `mark_output_spent`) call `self.graph.run()` directly — not through `execute_batched`. They need the same dual-path check. We can either:

**Option A**: Extract a small `run_query_single` helper for single-query methods:

```rust
async fn run_query_single(&self, q: Query, operation_name: &str) -> Result<()> {
    let mut txn_guard = self.active_txn.lock().await;
    if let Some(ref mut txn) = *txn_guard {
        txn.run(q).await.map_err(|e| {
            WriterError::TransactionFailed(format!("{}: {}", operation_name, e))
        })
    } else {
        drop(txn_guard);
        self.graph.run(q).await.map_err(|e| {
            WriterError::QueryFailed(format!("{}: {}", operation_name, e))
        })
    }
}
```

**Option B**: Inline the check in each method.

**Recommendation**: Option A — `run_query_single`. Keeps the dual-path logic in one place. All direct `self.graph.run()` calls in write methods get replaced with `self.run_query_single(q, "operation_name")`.

---

## Neo4jWriter::new() Change

```rust
pub async fn new(config: &Neo4jConfig) -> anyhow::Result<Self> {
    // ... existing graph connection setup ...

    Ok(Self {
        graph: Arc::new(graph),
        batch_size: config.write_batch_size,
        max_retries: config.max_retries,
        query_timeout: Duration::from_secs(config.query_timeout_secs),
        active_txn: TokioMutex::new(None),  // NEW
    })
}
```

---

## Summary of Changes

| File | Change |
|------|--------|
| `src/writer/neo4j/mod.rs` | Add `active_txn` field, implement 3 transaction methods, add `run_query_single` helper, modify `execute_batched` for dual-path, update `new()` |
| `src/writer/errors.rs` | Add `TransactionFailed(String)` variant, update `is_retryable()`, update `Display` |
| `src/writer/neo4j/mod.rs` | Route `update_checkpoint` and `mark_output_spent` through `run_query_single` |

**Files NOT changed**:
- `src/writer/traits.rs` — trait signatures unchanged
- `src/writer/mock.rs` — MockWriter unchanged
- `src/domain/ingestion.rs` — caller unchanged
- `src/writer/neo4j/queries.rs` — no new queries
