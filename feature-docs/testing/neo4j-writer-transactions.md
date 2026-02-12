---
title: Neo4jWriter Transaction Methods
status: testing
priority: high
ideation-ref: feature-docs/ideation/neo4j-writer-transactions/
affected-files:
  - src/writer/neo4j/mod.rs
  - src/writer/errors.rs
---

# Neo4jWriter Transaction Methods

## Summary

The `Neo4jWriter` has three `todo!()` stubs for `begin_transaction`, `commit_transaction`, and `rollback_transaction` that panic at runtime. PR #7 added explicit transaction wrapping to the batch ingestion loop (`ingest_blocks_batch`) but only implemented `MockWriter`. This feature implements the Neo4j side using neo4rs's `Txn` API, storing the active transaction in a `tokio::sync::Mutex<Option<Txn>>` field and routing all write methods through a dual-path dispatcher that uses `txn.run()` when a transaction is active or falls back to the existing `graph.run()` with retry when not.

## Acceptance Criteria

### Transaction Lifecycle

1. GIVEN `Neo4jWriter` with no active transaction WHEN `begin_transaction()` is called THEN `active_txn` field contains `Some(Txn)` and the method returns `Ok(())`

2. GIVEN `Neo4jWriter` with an active transaction WHEN `begin_transaction()` is called again THEN it returns `Err(WriterError::TransactionFailed)` with message containing "already active" — no connection leak

3. GIVEN `Neo4jWriter` with an active transaction WHEN `commit_transaction()` is called THEN the `Txn` is `.take()`-ed from the Option (transferring ownership), `txn.commit(self)` is called consuming the Txn, `active_txn` is left as `None`, and the method returns `Ok(())`

4. GIVEN `Neo4jWriter` with no active transaction WHEN `commit_transaction()` is called THEN it returns `Err(WriterError::TransactionFailed)` with message containing "no active transaction"

5. GIVEN `Neo4jWriter` with an active transaction WHEN `rollback_transaction()` is called THEN the `Txn` is `.take()`-ed, `txn.rollback(self)` is called consuming the Txn, `active_txn` is left as `None`, and the method returns `Ok(())`

6. GIVEN `Neo4jWriter` with no active transaction WHEN `rollback_transaction()` is called THEN it returns `Err(WriterError::TransactionFailed)` with message containing "no active transaction"

7. GIVEN `commit_transaction()` fails (neo4rs returns error) WHEN the error propagates THEN `active_txn` is `None` (the Txn was moved out and dropped, triggering implicit server-side rollback)

### Dual-Path Write Routing

8. GIVEN `Neo4jWriter` with an active transaction WHEN any `execute_batched` write method runs (e.g. `write_blocks_fast`, `write_outputs_fast`, `write_transactions_fast`, `write_inputs_fast`, `write_has_output_relationships_fast`, `write_performs`, `write_benefits_to`) THEN queries execute via `txn.run()` with no client-side retry and no client-side timeout wrapping

9. GIVEN `Neo4jWriter` with no active transaction WHEN any `execute_batched` write method runs THEN queries execute via the existing `run_with_retry` path (`graph.run()` with timeout + exponential backoff) — behaviour unchanged from before this feature

10. GIVEN `Neo4jWriter` with an active transaction WHEN `update_checkpoint()` is called THEN its query runs via `txn.run()` through the `run_query_single` helper (not direct `self.graph.run()`)

11. GIVEN `Neo4jWriter` with an active transaction WHEN `mark_output_spent()` is called THEN its query runs via `txn.run()` through the `run_query_single` helper (not direct `self.graph.run()`)

12. GIVEN `Neo4jWriter` with an active transaction WHEN a query within the transaction fails THEN the error is `WriterError::TransactionFailed` (not `WriterError::QueryFailed`) and no retry is attempted

### WriterError

13. GIVEN `WriterError::TransactionFailed` WHEN `is_retryable()` is called THEN it returns `false`

### Struct and Constructor

14. GIVEN `Neo4jWriter::new()` is called THEN the returned struct has `active_txn` initialized to `tokio::sync::Mutex::new(None)` and `Neo4jWriter` remains `Send + Sync`

### Compile Validation

15. GIVEN the `Neo4jWriter` struct with the new `active_txn: tokio::sync::Mutex<Option<Txn>>` field WHEN wrapped in `Arc<Neo4jWriter>` and shared across `tokio::spawn` tasks THEN it compiles — confirming `Send + Sync` bounds are satisfied

## Edge Cases

- `begin_transaction` called while a transaction is already active — returns `TransactionFailed` error, does not leak the existing connection
- `commit_transaction` called with no active transaction — returns `TransactionFailed` error, does not panic
- `rollback_transaction` called with no active transaction — returns `TransactionFailed` error, does not panic
- `commit` fails at the neo4rs level — the `Txn` was already `.take()`-en and is dropped after the failed `.commit()` call, so the connection returns to pool and Neo4j server rolls back implicitly
- Phase 6 parallel writes (8 `tokio::spawn` tasks) contend on `active_txn` lock — they serialize, which is correct because a single `Txn` owns a single connection; no deadlock possible since all tasks acquire the same single lock
- `execute_batched` with multiple sub-chunks inside a transaction — each sub-chunk acquires and releases the `active_txn` lock independently; the transaction persists across all sub-chunks until explicit commit/rollback
- Write method called with no transaction and no retry scenario — falls through to existing `graph.run()` auto-commit path, completely unchanged behaviour

## Out of Scope

- **Do NOT modify `src/writer/traits.rs`** — the `GraphWriter` trait signatures are already correct (`&self` for all three methods). Changing them would require updating MockWriter and all test code for no benefit.
- **Do NOT modify `src/writer/mock.rs`** — MockWriter's snapshot-based transaction implementation already works. Touching it risks breaking existing domain tests that depend on its behaviour.
- **Do NOT modify `src/domain/ingestion.rs`** — the caller code in `ingest_blocks_batch` already calls begin/commit/rollback correctly. The gap where commit failure doesn't trigger rollback is a separate concern owned by the caller, not Neo4jWriter.
- **Do NOT add UTXO cache rollback** — the in-memory UTXO cache divergence on DB rollback is a domain-layer concern tracked separately in the snapshot-resilience feature. Mixing it in here would couple writer and domain layers.
- **Do NOT add timeout/retry to `mark_output_spent`, `create_checkpoint`, or other single-write methods** — those methods lack timeout/retry today and fixing that inconsistency is a separate improvement. This feature only routes them through the transaction when one is active.
- **Do NOT add nested transaction support** — Neo4j does not support nested transactions. The double-begin guard is sufficient.
- **Do NOT wrap `ingest_block()` (single-block path) in transactions** — only `ingest_blocks_batch()` uses transactions. The single-block path is used for real-time ingestion where auto-commit per phase is acceptable.

## Technical Notes

- **`tokio::sync::Mutex` exception**: The project convention is `std::sync::Mutex` for interior mutability. The `active_txn` field requires `tokio::sync::Mutex` because `Txn::run(&mut self)` is async and the guard must be held across `.await` points. `std::sync::Mutex` guards are `!Send` and cannot cross await boundaries. This is a targeted exception — all other fields remain unchanged.
- **`Option<Txn>` for ownership transfer**: `Txn::commit(self)` and `Txn::rollback(self)` consume the Txn by value. The `.take()` pattern moves it out of the Option to satisfy Rust's ownership rules. After take, the Option is `None`.
- **No client-side retry in transactional mode**: A failed query inside a Neo4j transaction puts the server-side transaction in an aborted state. Subsequent queries will fail with "transaction has been terminated." The only valid recovery is rollback + retry the entire batch. The ingestion loop already handles this via checkpoint + resume.
- **No client-side timeout in transactional mode**: The `tokio::time::timeout` wrapper would cancel the future mid-flight, leaving the Txn in an unknown state. Server-side `dbms.transaction.timeout` is the appropriate safeguard.
- **Phase 6 serialization**: The 8 parallel `tokio::spawn` tasks in Phase 6 contend on the `active_txn` lock and serialize. This is unavoidable — a neo4rs `Txn` owns a single connection and `run(&mut self)` requires exclusive access. The parallelism was only useful in auto-commit mode where each `graph.run()` gets its own pool connection. Performance impact is bounded since PERFORMS/BENEFITS_TO writes are fast UNWIND operations.
- **Double-lock in `execute_batched`**: The design locks once to check `is_some()`, drops the guard, builds the query, then locks again to run. The check-then-act is safe because only the ingestion loop controls transaction lifecycle (single-threaded begin/commit/rollback control flow).
- **`run_query_single` helper**: A small method that routes a single Query through the active transaction or falls back to direct `graph.run()`. Used by `update_checkpoint` and `mark_output_spent` which call `self.graph.run()` directly instead of going through `execute_batched`.
- **Rejected: `execute_query` as single dispatch point** — `neo4rs::Query` is not `Clone`, and `run_with_retry`'s closure is `Fn()` (re-invocable), so it must reconstruct the Query from `query_str` + `param_name` + `bolt_data` on each retry. A single `execute_query(q: Query)` method cannot support the auto-commit retry path. Instead, the dual-path check lives in `execute_batched` (for bulk) and `run_query_single` (for single).
- **Rejected: 8 separate transactions for Phase 6 buckets** — would break atomicity. If bucket 3 fails, buckets 0-2 are already committed. Not worth the complexity.
- **Rejected: `std::sync::Mutex` with take-and-replace pattern** — error-prone because the Txn could be lost if a panic occurs between `.take()` and put-back. `tokio::sync::Mutex` is cleaner and safer.
- **Rejected: no-op implementation** — would restore pre-PR#7 auto-commit behaviour, making the transaction calls in `ingest_blocks_batch` misleading. The trait exists, MockWriter implements it, and the ingestion loop depends on it.
- **Follow the pattern in** `src/writer/mock.rs:483-540` for transaction state management semantics (begin sets state, commit clears state, rollback restores + clears state), adapting for neo4rs ownership.
