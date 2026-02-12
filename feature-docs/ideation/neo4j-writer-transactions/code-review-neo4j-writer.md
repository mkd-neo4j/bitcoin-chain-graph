# Code Review: Neo4jWriter (`src/writer/neo4j/mod.rs`)

## 1. Neo4jWriter Struct Fields

```rust
pub struct Neo4jWriter {
    graph: Arc<Graph>,        // neo4rs connection pool (shared via Arc)
    batch_size: usize,        // chunk size for UNWIND batches (from config.write_batch_size)
    max_retries: usize,       // retry attempts on transient failure (from config.max_retries)
    query_timeout: Duration,  // per-query timeout (from config.query_timeout_secs)
}
```

All fields are set once in `Neo4jWriter::new()` from `Neo4jConfig` and never mutated. The `graph` field is the only one wrapped in `Arc` — it's the neo4rs connection pool handle.

---

## 2. `run_with_retry` Method (lines 180-245)

### Signature
```rust
async fn run_with_retry<F, Fut>(
    &self,
    operation_name: &str,   // for logging ("write_blocks", etc.)
    f: F,                   // closure that produces the future (called on each attempt)
    batch_num: usize,       // current batch number (for error messages)
    total_batches: usize,   // total batch count (for error messages)
    record_count: usize,    // records in this batch (for error messages)
) -> Result<()>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<(), neo4rs::Error>>,
```

### Retry Logic
- **Loop**: Infinite loop with `attempt` counter starting at 0.
- **Timeout wrapping**: Each attempt is wrapped with `tokio::time::timeout(self.query_timeout, f())`.
- **Three match arms**:
  1. `Ok(Ok(()))` — Success, return immediately.
  2. `Ok(Err(e))` — Query error. Converts to `WriterError::QueryFailed`. If `attempt < max_retries` AND `writer_err.is_retryable()`, increments attempt and sleeps. Otherwise returns the error.
  3. `Err(_elapsed)` — Timeout. If `attempt < max_retries`, increments and sleeps. Otherwise returns timeout error.

### Backoff Schedule
```
delay = 200ms * 2^(attempt-1)
```
- Attempt 1: 200ms
- Attempt 2: 400ms
- Attempt 3: 800ms
- Attempt 4: 1600ms
- etc.

### Key Observations
- The closure `f` is `Fn()` (not `FnOnce`), so it's re-invoked on each retry attempt — this recreates the query and borrows `bolt_data` again.
- Retryable errors: `WriterError::QueryFailed` and `WriterError::ConnectionFailed` (per `is_retryable()` in `error.rs`).
- Timeouts are ALWAYS retried (no `is_retryable()` check), up to `max_retries`.
- **No transaction awareness**: There is no concept of "am I inside an explicit transaction?" — retries happen at the individual `graph.run()` level.

---

## 3. `execute_batched` Method (lines 117-173)

### Signature
```rust
async fn execute_batched<T, F>(
    &self,
    items: &[T],            // full slice of domain objects
    query_str: &str,        // Cypher query constant
    param_name: &str,       // UNWIND parameter name ("blocks", "transactions", etc.)
    operation_name: &str,   // human-readable name for logging
    convert: F,             // closure: &[T] -> Vec<BoltType>
) -> Result<()>
where
    F: Fn(&[T]) -> Vec<BoltType>,
```

### How It Works
1. Early return if `items.is_empty()`.
2. Calculates `total_batches = items.len().div_ceil(self.batch_size)`.
3. Iterates over `items.chunks(self.batch_size)`.
4. For each chunk:
   - Calls `convert(chunk)` to get `Vec<BoltType>`.
   - Logs debug message (skips first batch to reduce noise).
   - Calls `self.run_with_retry(...)` with a closure that builds the query via `query(query_str).param(param_name, bolt_data.as_slice())` and calls `self.graph.run(q)`.
   - Logs elapsed time after success.
5. Returns `Ok(())` after all chunks complete.

### Key Observations
- **Each chunk is an independent auto-commit query** — there is no transaction wrapping multiple chunks.
- If chunk N fails after chunks 0..N-1 succeeded, those earlier chunks are already committed. There is no rollback of partial progress.
- The `bolt_data` variable is created per-chunk and captured by the retry closure via reference (`bolt_data.as_slice()`).

---

## 4. ALL `write_*_fast` Methods (Inherent Impl, lines 255-329)

Each fast method follows the exact same pattern — a single `execute_batched` call:

| Method | Line | Query Constant | Param Name | Conversion Fn |
|--------|------|----------------|------------|---------------|
| `write_blocks_fast` | 259 | `CREATE_BLOCKS_FAST_QUERY` | `"blocks"` | `blocks_to_bolt_list` |
| `write_transactions_fast` | 274 | `CREATE_TRANSACTIONS_FAST_QUERY` | `"transactions"` | `transactions_to_bolt_list` |
| `write_outputs_fast` | 292 | `CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY` | `"outputs"` | `outputs_to_bolt_list` |
| `write_has_output_relationships_fast` | 304 | `CREATE_HAS_OUTPUT_FAST_QUERY` | `"outputs"` | `outputs_to_bolt_list` |
| `write_inputs_fast` | 319 | `CREATE_INPUTS_FAST_QUERY` | `"inputs"` | `inputs_to_bolt_list` |

### How Each Calls `graph.run()`
None of the fast methods call `graph.run()` directly. They all delegate to `execute_batched`, which internally calls `run_with_retry`, which calls the closure containing `self.graph.run(q)`.

Call chain: `write_*_fast` -> `execute_batched` -> `run_with_retry` -> `self.graph.run(q)`

### Notable: `write_outputs_fast` vs `write_outputs`
- `write_outputs_fast` (line 292) uses a **single** combined query `CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY` that creates both Output nodes and LOCKED_TO relationships in one pass.
- `write_outputs` (trait impl, line 359) uses **two separate queries** per chunk: `CREATE_OUTPUTS_QUERY` + `CREATE_LOCKED_TO_QUERY` (with address filtering). This is the only write method with custom chunk handling instead of `execute_batched`.

---

## 5. The Three `todo!()` Stubs

All three are in the `GraphWriter` trait impl block (lines 770-781):

### Line 770-772: `begin_transaction`
```rust
async fn begin_transaction(&self) -> Result<()> {
    todo!("Neo4jWriter::begin_transaction — wrap batch in explicit neo4rs transaction")
}
```

### Line 774-776: `commit_transaction`
```rust
async fn commit_transaction(&self) -> Result<()> {
    todo!("Neo4jWriter::commit_transaction — commit explicit neo4rs transaction")
}
```

### Line 778-780: `rollback_transaction`
```rust
async fn rollback_transaction(&self) -> Result<()> {
    todo!("Neo4jWriter::rollback_transaction — rollback explicit neo4rs transaction")
}
```

These are the **only** unimplemented methods in `Neo4jWriter`. They will panic at runtime if called.

---

## 6. Other Methods That Interact with `self.graph`

Beyond the write methods that go through `execute_batched`/`run_with_retry`, these methods call `self.graph` directly:

| Method | Line | Graph Call | Pattern |
|--------|------|------------|---------|
| `health_check` | 89-95 | `self.graph.run(query("RETURN 1"))` | Timeout-wrapped, no retry |
| `graph()` | 98-100 | Returns `&Graph` reference | Accessor only |
| `mark_output_spent` | 483-500 | `self.graph.run(...)` | **No timeout, no retry** |
| `create_checkpoint` | 502-516 | `self.graph.run(...)` x2 | **No timeout, no retry** |
| `update_checkpoint` | 518-543 | `self.graph.run(...)` | Timeout-wrapped, no retry |
| `get_checkpoint` | 545-593 | `self.graph.execute(...)` | Timeout-wrapped, no retry |
| `mark_checkpoint_complete` | 595-604 | `self.graph.run(...)` | **No timeout, no retry** |
| `set_checkpoint_status` | 606-615 | `self.graph.run(...)` | **No timeout, no retry** |
| `lookup_block_hash` | 617-648 | `self.graph.execute(...)` | Timeout-wrapped, no retry |
| `rollback_block` | 650-699 | `self.graph.run(...)` x4 | **No timeout, no retry** (4 sequential steps) |
| `get_max_block_height` | 701-722 | `self.graph.execute(...)` | Timeout-wrapped, no retry |
| `check_block_complete` | 724-746 | `self.graph.execute(...)` | Timeout-wrapped, no retry |
| `write_outputs` (trait) | 359-437 | `run_with_retry` x2 per chunk | Custom loop (not `execute_batched`) |

### Observations on Consistency
- **Bulk write methods** (via `execute_batched`): Have both timeout AND retry.
- **Read/query methods** (`get_checkpoint`, `lookup_block_hash`, etc.): Have timeout but NO retry.
- **Single-write methods** (`mark_output_spent`, `create_checkpoint`, `mark_checkpoint_complete`, `set_checkpoint_status`, `rollback_block`): Have NEITHER timeout NOR retry.
- This inconsistency is worth noting for the transaction implementation — `mark_output_spent` in particular runs inside the hot ingestion loop without any resilience.

---

## 7. Summary of Current Transaction State

### What Exists
- Each `graph.run()` call is an **implicit auto-commit transaction** in Neo4j.
- `run_with_retry` provides retry at the individual query level.
- `execute_batched` chunks data and runs each chunk as a separate auto-commit query.

### What's Missing (the `todo!()` stubs)
- No way to group multiple queries into an atomic unit.
- A single block's ingestion spans 7 phases with multiple `graph.run()` calls — if phase 4 fails after phases 1-3 succeeded, the block is partially written.
- The `rollback_block` method exists as a cleanup mechanism but relies on 4 separate non-atomic deletes.

### neo4rs Transaction API
neo4rs provides explicit transactions via `graph.start_txn()` which returns a `Txn` object with `.run()`, `.commit()`, and `.rollback()` methods. The `todo!()` stubs indicate the intent to use this API but the challenge is threading the `Txn` through the existing `execute_batched`/`run_with_retry` infrastructure, which currently calls `self.graph.run()` directly.
