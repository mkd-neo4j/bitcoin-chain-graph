# Code Review: GraphWriter Trait & MockWriter Transaction Implementation

## 1. GraphWriter Trait (`src/writer/traits.rs`)

### Transaction Method Signatures

```rust
async fn begin_transaction(&self) -> Result<()>;
async fn commit_transaction(&self) -> Result<()>;
async fn rollback_transaction(&self) -> Result<()>;
```

### Key Observations

- **Stateless signature**: All three methods take `&self` and return `Result<()>`. There is no transaction handle/token returned from `begin_transaction` -- the implementation must track transaction state internally.
- **Implicit single-transaction model**: The trait assumes at most one active transaction per writer instance. There is no support for concurrent/nested transactions.
- **No transaction ID**: Callers cannot distinguish between transactions. If `begin_transaction` is called twice without commit/rollback, behavior is implementation-defined (no compile-time enforcement).
- **Doc comments** document the expected semantics:
  - `begin_transaction`: "All subsequent write operations will be part of this transaction"
  - `commit_transaction`: "Atomically persists all writes since begin_transaction()"
  - `rollback_transaction`: "Discards all writes since begin_transaction()"
- **No default implementations**: Unlike the `_fast` methods which delegate to regular methods, the transaction methods have no defaults -- every `GraphWriter` implementor MUST provide them.

### Trait Design Implications for Neo4j Implementation

1. **`&self` not `&mut self`**: Interior mutability required. Neo4jWriter will need some form of `Mutex<Option<Transaction>>` or similar to hold the neo4rs transaction handle.
2. **No lifetime coupling**: The transaction is not tied to a borrow of the writer, so it can live across multiple `.await` points freely.
3. **Error semantics on commit**: Doc says commit can fail due to "timeout, memory pressure" -- caller must handle this (likely by retrying the entire batch).

---

## 2. MockWriter (`src/writer/mock.rs`) Transaction Implementation

### Internal State

```rust
struct MockStorage {
    // ... data vectors ...
    in_transaction: bool,
    checkpoint_written_in_txn: bool,
    snapshot: Option<MockSnapshot>,
    // ... failure injection ...
}
```

### MockSnapshot (Rollback Support)

```rust
struct MockSnapshot {
    blocks_len: usize,
    transactions_len: usize,
    outputs_len: usize,
    inputs_len: usize,
    performs_len: usize,
    benefits_to_len: usize,
    checkpoint: Option<CheckpointData>,
}
```

Captures vector lengths + checkpoint state at `begin_transaction` time. Rollback truncates vectors back to snapshot lengths. This works because writes only append (no in-place mutations during a transaction).

### begin_transaction (lines 483-498)

```rust
async fn begin_transaction(&self) -> Result<()> {
    let mut storage = self.storage.lock().unwrap();
    if let Some(err) = Self::check_failure(&mut storage, "begin_transaction") {
        return Err(err);
    }
    storage.in_transaction = true;
    storage.snapshot = Some(MockSnapshot { /* capture lengths */ });
    Ok(())
}
```

- Sets `in_transaction = true`
- Saves snapshot of all vector lengths + checkpoint
- Supports failure injection
- **No guard against double-begin** (calling begin while already in transaction silently overwrites snapshot)

### commit_transaction (lines 501-521)

```rust
async fn commit_transaction(&self) -> Result<()> {
    let mut storage = self.storage.lock().unwrap();
    if let Some(err) = Self::check_failure(&mut storage, "commit_transaction") {
        // On commit failure, auto-rollback
        if let Some(snap) = storage.snapshot.take() {
            // truncate all vectors to snapshot lengths
            // restore checkpoint
        }
        storage.in_transaction = false;
        return Err(err);
    }
    storage.in_transaction = false;
    storage.snapshot = None;
    storage.transaction_commit_count += 1;
    Ok(())
}
```

- **Auto-rollback on commit failure**: If check_failure triggers, the mock rolls back all data to the snapshot. This simulates Neo4j behavior where a failed commit discards the transaction.
- Increments `transaction_commit_count` on success (observable via test accessor).
- Clears snapshot on success.

### rollback_transaction (lines 523-539)

```rust
async fn rollback_transaction(&self) -> Result<()> {
    let mut storage = self.storage.lock().unwrap();
    if let Some(err) = Self::check_failure(&mut storage, "rollback_transaction") {
        return Err(err);
    }
    if let Some(snap) = storage.snapshot.take() {
        // truncate all vectors to snapshot lengths
        // restore checkpoint
    }
    storage.in_transaction = false;
    Ok(())
}
```

- Restores all vectors to snapshot lengths via `truncate()`
- Restores checkpoint to pre-transaction state
- **Silent no-op if no snapshot** (rollback without begin is not an error)

### Test Observability

The MockWriter provides these test accessors for transaction behavior verification:

| Method | Returns |
|--------|---------|
| `transaction_commit_count()` | Number of successful commits |
| `checkpoint_written_in_transaction()` | Whether checkpoint was updated inside a txn |

### Failure Injection for Transactions

Three failure modes available via MockWriter API:

1. **`set_failure_on(method, error)`** -- Permanent failure on any method including `begin_transaction`, `commit_transaction`, `rollback_transaction`
2. **`set_transient_failure_on(method, error, times)`** -- Fails N times then succeeds
3. **`set_failure_after_n_calls(method, n, error)`** -- Succeeds N times then fails permanently

---

## 3. WriterError Variants (`src/writer/error.rs`)

| Variant | Fields | Retryable | Description |
|---------|--------|-----------|-------------|
| `OutputNotFound` | `String` | No | Referenced output doesn't exist |
| `ConnectionFailed` | `String` | **Yes** | Database connection issue |
| `QueryFailed` | `String` | **Yes** | Query execution failure |
| `CheckpointError` | `String` | No | Checkpoint operation failure |
| `SerializationError` | `String` | No | Data serialization failure |
| `DatabaseError` | `String` | No | General database error |
| `ConstraintViolation` | `String` | No | Unique constraint violation (e.g., duplicate node) |
| `ReorgDetected` | `{ height: u32, expected: String, actual: String }` | No | Chain reorganization detected |

### Retryability

`is_retryable()` returns `true` only for `QueryFailed` and `ConnectionFailed`. All other variants are considered deterministic/non-transient.

### Trait Derivations

`WriterError` derives `Error`, `Debug`, and `Clone`. The `Clone` derive is notable -- it enables the MockWriter's failure injection to store and return cloned errors multiple times.

---

## 4. Design Gaps Relevant to Neo4j Transaction Implementation

### 4.1 No Transaction State Validation

The trait has no mechanism to enforce the state machine: `begin -> (writes) -> commit|rollback`. Specifically:
- Double-begin overwrites snapshot silently in MockWriter
- Commit/rollback without begin is a silent no-op in MockWriter
- Writes outside a transaction are allowed (no enforcement)

**Recommendation**: The Neo4j implementation should track state and return errors for invalid transitions, even if the trait doesn't enforce it.

### 4.2 No Transaction Timeout

The trait has no timeout parameter on `begin_transaction`. Neo4j transactions have server-side timeouts (`dbms.transaction.timeout`). The Neo4j implementation will need to handle timeout errors, likely mapping them to `QueryFailed` or a new variant.

### 4.3 Checkpoint-in-Transaction Coupling

`update_checkpoint` sets `checkpoint_written_in_txn = true` when `in_transaction` is true. This suggests the orchestrator calls `update_checkpoint` inside the transaction scope. The Neo4j implementation must ensure checkpoint updates participate in the same database transaction.

### 4.4 Commit Failure = Auto-Rollback

MockWriter auto-rolls back on commit failure. Neo4j also discards the transaction on commit failure. This behavior is consistent, but callers must be aware that after a commit failure, the transaction is gone -- you cannot retry the commit, only retry the entire begin-writes-commit sequence.

### 4.5 No WriterError Variant for Transaction State Errors

There is no `TransactionError` or `InvalidTransactionState` variant. If the Neo4j implementation detects invalid state transitions (e.g., commit without begin), it would need to use `DatabaseError` or a new variant.
