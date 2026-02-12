# Writer Layer

Database abstraction with trait-based design. Isolates database-specific concerns from domain logic.

## Key Files

- `traits.rs` — `GraphWriter` trait (~25 async methods, the core abstraction)
- `error.rs` — `WriterError` enum with thiserror, `Result<T>` type alias, `is_retryable()`
- `mock.rs` — `MockWriter` in-memory implementation for testing
- `neo4j/mod.rs` — `Neo4jWriter` production implementation using neo4rs
- `neo4j/queries.rs` — ALL Cypher query constants (centralized, no inline Cypher elsewhere)
- `neo4j/schema.rs` — Constraint and index creation (idempotent with IF NOT EXISTS)
- `neo4j/conversions.rs` — Domain models → BoltType/BoltMap conversions

## GraphWriter Trait

The trait requires `Send + Sync` and uses `#[async_trait]`. Key method groups:

1. **Schema**: `init_schema()`
2. **Phase writes**: `write_blocks()`, `write_outputs()`, `write_transactions()`, `write_has_output_relationships()`, `write_inputs()`, `write_performs()`, `write_benefits_to()`
3. **UTXO ops**: `mark_output_spent()`
4. **Checkpoint**: `create_checkpoint()`, `update_checkpoint()`, `get_checkpoint()`, `mark_checkpoint_complete()`, `set_checkpoint_status()`
5. **Block lookup**: `lookup_block_hash()`
6. **Rollback**: `rollback_block()`
7. **Recovery**: `get_max_block_height()`, `check_block_complete()`
8. **Fast variants**: `write_blocks_fast()`, etc. (default impls delegate to regular methods)

## WriterError Conventions

- Specific variants: `OutputNotFound`, `ConnectionFailed`, `QueryFailed`, `CheckpointError`, `DatabaseError`, `ReorgDetected`
- `is_retryable()` returns true for `QueryFailed` and `ConnectionFailed`
- Format messages with context: `"operation_name failed (param): error_details"`

## Neo4jWriter Patterns

- **Connection pool**: neo4rs `ConfigBuilder` → `Graph::connect()`
- **Bulk writes**: `execute_batched()` generic helper — chunks items, converts to BoltType, UNWIND query
- **Retries**: `run_with_retry()` — timeout + exponential backoff (200ms, 400ms, 800ms...)
- **All queries use UNWIND** with parameterized BoltType lists — never individual queries in loops
- **MERGE** for idempotent reprocessing, **CREATE** for fast forward-only ingestion
- **All Cypher is in `queries.rs`** as `pub const` — never construct query strings at runtime

### BoltType Conversions

- All integer params must be `i64` (cast u32/u64 with `as i64`)
- BoltMap keys are BoltString (use `.into()`)
- Known quirk: i64 `-1` may be misread as 255 — use sentinel `-999` instead
- Optional values: use conditional map insertion or `BoltType::Null`

## MockWriter for Testing

- Stores data in `Arc<Mutex<MockStorage>>` with `Vec` fields for each entity type
- All trait methods succeed by default (append to vectors)
- Test accessor methods: `get_blocks()`, `get_transactions()`, etc.
- `clear()` for test cleanup
- Implements rollback by filtering vectors
- No external dependencies required

## Adding a New GraphWriter Implementation

1. Create module under `writer/` (e.g., `writer/memgraph/`)
2. Implement ALL GraphWriter trait methods
3. Handle `Result<T>` with appropriate `WriterError` variants
4. Add to `writer/mod.rs` exports
5. Test with existing integration tests by swapping writer
6. Ensure both MERGE and fast (CREATE) variants work correctly
