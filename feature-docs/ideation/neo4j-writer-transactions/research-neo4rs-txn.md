# neo4rs 0.8 Txn API Research

## Version
- **neo4rs 0.8.0** (from Cargo.toml line 23: `neo4rs = "0.8"`)

## Txn Struct (source: `neo4rs-0.8.0/src/txn.rs`)

```rust
pub struct Txn {
    db: Database,
    fetch_size: usize,
    connection: ManagedConnection,  // owns a dedicated pooled connection
}
```

### Key Methods

| Method | Signature | Notes |
|--------|-----------|-------|
| `run` | `pub async fn run(&mut self, q: Query) -> Result<()>` | Runs query, discards stream |
| `execute` | `pub async fn execute(&mut self, q: Query) -> Result<RowStream>` | Runs query, returns rows |
| `run_queries` | `pub async fn run_queries<Q: Into<Query>>(&mut self, queries: impl IntoIterator<Item = Q>) -> Result<()>` | Sequential multi-query |
| `commit` | `pub async fn commit(mut self) -> Result<()>` | **Consumes self** |
| `rollback` | `pub async fn rollback(mut self) -> Result<()>` | **Consumes self** |
| `handle` | `pub fn handle(&mut self) -> &mut impl TransactionHandle` | Returns trait object ref |

### Creation

```rust
// Graph method (graph.rs:55)
pub async fn start_txn(&self) -> Result<Txn>
pub async fn start_txn_on(&self, db: impl Into<Database>) -> Result<Txn>
```

`start_txn` gets a connection from the pool and sends a Bolt BEGIN message. The connection is **reserved** for the lifetime of the `Txn`.

## Send + Sync

**Yes, Txn is Send + Sync.** Confirmed by compile-time assertion in txn.rs:82-85:

```rust
const _: () = {
    const fn assert_send_sync<T: ?Sized + Send + Sync>() {}
    assert_send_sync::<Txn>();
};
```

This means `Txn` can be stored behind `Arc<Mutex<Txn>>` or `Option<Txn>` in a `Mutex`.

## Ownership Semantics

Critical design constraints:

1. **`run()` takes `&mut self`** — requires exclusive mutable access
2. **`commit()` takes `mut self`** — **consumes** the Txn (moved, not borrowed)
3. **`rollback()` takes `mut self`** — **consumes** the Txn (moved, not borrowed)
4. On drop without commit/rollback, the connection is returned to pool (implicit rollback by Neo4j server)

### Implications for Arc<Mutex<>> Storage

Because `commit(self)` and `rollback(self)` consume the Txn by value, you **cannot** call them through `&mut` reference from a Mutex lock. You must **take** the Txn out of the Option/container first:

```rust
// This pattern works:
struct Neo4jWriter {
    graph: Arc<Graph>,
    active_txn: std::sync::Mutex<Option<Txn>>,
}

// begin:
let txn = self.graph.start_txn().await?;
*self.active_txn.lock().unwrap() = Some(txn);

// run query within txn:
let mut guard = self.active_txn.lock().unwrap();
if let Some(txn) = guard.as_mut() {
    txn.run(query).await?;
}

// commit (must take ownership):
let txn = self.active_txn.lock().unwrap().take()
    .ok_or(WriterError::DatabaseError("no active transaction".into()))?;
txn.commit().await?;

// rollback (must take ownership):
let txn = self.active_txn.lock().unwrap().take()
    .ok_or(WriterError::DatabaseError("no active transaction".into()))?;
txn.rollback().await?;
```

### Problem: Mutex Guard Across Await

`std::sync::Mutex` guard is `!Send`, so you **cannot hold it across `.await`** points. This means you cannot do:

```rust
// WILL NOT COMPILE:
let mut guard = self.active_txn.lock().unwrap();
guard.as_mut().unwrap().run(query).await?;  // await while holding guard
```

**Solutions:**

1. **`tokio::sync::Mutex`** — its guard IS Send, so it can be held across await. But the project convention uses `std::sync::Mutex`.

2. **Take-and-replace pattern** — take the Txn out, use it, put it back:
   ```rust
   let mut txn = self.active_txn.lock().unwrap().take().unwrap();
   // guard dropped here (lock released)
   txn.run(query).await?;
   *self.active_txn.lock().unwrap() = Some(txn);
   ```

3. **Dedicated async Mutex for txn only** — use `tokio::Mutex<Option<Txn>>` specifically for the transaction field, while keeping `std::sync::Mutex` elsewhere.

## Graph::run vs Txn::run

Important difference: `Graph::run()` has **built-in retry with exponential backoff** (up to 60s). `Txn::run()` does **not** retry — any failure must be handled by the caller. This means the existing `run_with_retry()` helper in Neo4jWriter would need adaptation for transactional writes.

## Existing Codebase Usage

- `GraphWriter` trait already defines `begin_transaction()`, `commit_transaction()`, `rollback_transaction()` (traits.rs:401-417)
- `MockWriter` implements these with snapshot/restore pattern (mock.rs:483+)
- `Neo4jWriter` has `todo!()` stubs for all three (neo4j/mod.rs:770-779)
- `IngestionOrchestrator` calls `begin_transaction()` at ingestion.rs:589
- Current Neo4jWriter stores `graph: Arc<Graph>` — all writes use `self.graph.run()` (auto-commit mode)

## Recommended Storage Pattern

```rust
pub struct Neo4jWriter {
    graph: Arc<Graph>,
    batch_size: usize,
    max_retries: usize,
    query_timeout: Duration,
    active_txn: tokio::sync::Mutex<Option<Txn>>,  // tokio Mutex for await-safety
}
```

Use `tokio::sync::Mutex` for the `active_txn` field only. This is the cleanest approach because:
- Txn methods require `&mut self` + `.await` — std::sync::Mutex cannot hold guard across await
- Take-and-replace is error-prone (Txn could be lost on panic between take and put-back)
- The Txn is only accessed during write operations, not in hot contention paths

### Dual-Path Write Methods

Each write method needs to check for active transaction:

```rust
async fn run_query(&self, q: Query) -> Result<()> {
    let mut txn_guard = self.active_txn.lock().await;
    if let Some(txn) = txn_guard.as_mut() {
        txn.run(q).await.map_err(|e| WriterError::QueryFailed(e.to_string()))?;
    } else {
        self.graph.run(q).await.map_err(|e| WriterError::QueryFailed(e.to_string()))?;
    }
    Ok(())
}
```

This preserves backward compatibility — without an active transaction, writes use auto-commit with retries as before.
