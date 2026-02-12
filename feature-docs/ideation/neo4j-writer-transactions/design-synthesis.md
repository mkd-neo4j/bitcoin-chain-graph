# Design Synthesis: Neo4jWriter Transaction Methods

Distilled from four parallel code reviews. This captures the key constraints, design decisions, and open questions.

---

## The Problem

`Neo4jWriter` has three `todo!()` stubs that panic at runtime:
- `begin_transaction(&self)` — line 770
- `commit_transaction(&self)` — line 774
- `rollback_transaction(&self)` — line 778

The ingestion loop (`ingest_blocks_batch`) calls these to wrap each batch chunk in a transaction. Without implementation, the service panics on startup.

## Constraints Discovered

### 1. Ownership Mismatch (core challenge)

| GraphWriter trait | neo4rs Txn API |
|---|---|
| `begin_transaction(&self)` | `Graph::start_txn(&self) -> Result<Txn>` |
| all writes via `&self` | `Txn::run(&mut self, q)` |
| `commit_transaction(&self)` | `Txn::commit(mut self)` — **consumes** |
| `rollback_transaction(&self)` | `Txn::rollback(mut self)` — **consumes** |

The trait uses `&self` (interior mutability needed). Commit/rollback consume the Txn by value, so it must be `.take()`-ed from an `Option`.

### 2. Async + Mutex (Rust constraint)

`Txn::run(&mut self)` requires `.await`. A `std::sync::Mutex` guard is `!Send` and cannot be held across await points. Options:

| Approach | Pros | Cons |
|---|---|---|
| **`tokio::sync::Mutex<Option<Txn>>`** | Clean, holds guard across await | Breaks project convention (`std::sync::Mutex` everywhere else) |
| **Take-and-replace with `std::sync::Mutex`** | Follows convention | Error-prone: Txn lost if panic between take and put-back |
| **Dedicated `tokio::Mutex` for txn only** | Targeted exception | Mixed mutex types in one struct |

**Recommendation**: `tokio::sync::Mutex<Option<Txn>>` for the `active_txn` field only. The project convention note in CLAUDE.md says `std::sync::Mutex` for interior mutability, but that guidance assumes short critical sections. Transaction usage spans async operations by design.

### 3. Phase 6 Parallelism (concurrency constraint)

Phase 6 spawns 8 parallel `tokio::spawn` tasks that call `writer.write_performs()` and `writer.write_benefits_to()` concurrently. All execute within the same transaction.

With `tokio::sync::Mutex<Option<Txn>>`, these 8 tasks would serialize on the lock — only one can run `txn.run()` at a time. This is correct for Neo4j (a single Txn owns a single connection), but it means Phase 6 loses its parallelism benefit when inside a transaction.

**This is acceptable**: Neo4j serializes queries on a single connection anyway. The parallelism was only useful in auto-commit mode where each `graph.run()` gets its own connection from the pool.

### 4. Retry Semantics Change

| Mode | Retry behavior |
|---|---|
| Auto-commit (no txn) | `run_with_retry` retries individual queries with exponential backoff |
| Transactional | Retries are **dangerous** — a failed query may abort the server-side transaction. Must fail-fast and let the caller rollback + retry the entire batch |

**Decision**: Disable per-query retries when inside a transaction. The ingestion loop already handles batch-level recovery via checkpoint + resume.

### 5. Dual-Path Write Methods

Every write method must check for an active transaction:

```
if active_txn exists:
    txn.run(query)       # no retry, fail-fast
else:
    graph.run(query)     # with retry (existing behavior)
```

The cleanest approach is modifying `run_with_retry` (or adding a parallel method) to route through the active transaction when present.

### 6. WriterError Gap

No `TransactionFailed` variant exists. Current candidates for transaction errors:
- `DatabaseError(String)` — too generic
- `QueryFailed(String)` — semantically wrong for begin/commit/rollback

**Decision**: Add `TransactionFailed(String)` variant. Mark it non-retryable (caller must rollback + retry the full batch).

## Scope

### In Scope
1. Add `active_txn: tokio::sync::Mutex<Option<Txn>>` to `Neo4jWriter`
2. Implement `begin_transaction`, `commit_transaction`, `rollback_transaction`
3. Route write methods through active txn when present
4. Add `TransactionFailed` variant to `WriterError`
5. Disable per-query retries when inside a transaction

### Out of Scope
- UTXO cache rollback on transaction failure (separate feature, in-memory concern)
- Rollback on commit failure in ingestion.rs (caller-side gap, not Neo4jWriter's responsibility)
- Adding timeout/retry to single-write methods (`mark_output_spent`, `create_checkpoint`, etc.)
- Nested transactions
- `single-block ingest_block()` transaction wrapping

## Two Implementation Approaches

### Approach A: Full Transaction Support
Implement everything above. Write methods detect active txn and route queries accordingly. Significant change to `run_with_retry`/`execute_batched`.

**Effort**: ~10 acceptance criteria, touches Neo4jWriter struct + all write paths + error types.

### Approach B: No-Op Unblock
```rust
async fn begin_transaction(&self) -> Result<()> { Ok(()) }
async fn commit_transaction(&self) -> Result<()> { Ok(()) }
async fn rollback_transaction(&self) -> Result<()> { Ok(()) }
```

Restores pre-PR#7 auto-commit behavior. Each query commits independently. Checkpoint mechanism handles crash recovery. No atomicity across a batch chunk.

**Effort**: 3 lines changed. Risk: partial batch writes on failure (already handled by resume).

### Recommendation

**Approach A** — full implementation. The trait exists, MockWriter implements it, and the ingestion loop depends on it. No-ops would make the transaction calls misleading and leave the codebase in a half-designed state.
