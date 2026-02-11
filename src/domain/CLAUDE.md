# Domain Layer

Business logic and type-safe models bridging Parser (bitcoin crate types) and Writer (database operations). Uses only primitive types.

## Key Files

- `models.rs` — All domain structs: BlockData, TransactionData, OutputData, InputData, CheckpointData, PerformsData, BenefitsToData
- `ingestion.rs` — `IngestionOrchestrator<W: GraphWriter>` with 7-phase pipeline
- `conversions.rs` — Conversion functions from bitcoin crate types to domain models (the type boundary)
- `utxo/cache.rs` — 16-shard LRU cache with compact 36-byte UtxoKey

## Model Conventions

- All models derive `Clone, Debug` at minimum
- Use only primitive types: String, u32, u64, i64, f64, bool, Option<T>, Vec<T>
- NO bitcoin crate types in struct fields
- NO neo4rs types in struct fields
- `///` doc comment on every field
- Identifier patterns: `"{txid}:{index}"` for output_id and input_id

## IngestionOrchestrator Pattern

```rust
pub struct IngestionOrchestrator<W: GraphWriter> {
    writer: Arc<W>,
    network: Network,
    utxo_cache: UtxoCache<W>,
}
```

- Generic over `W: GraphWriter + 'static`
- Writer wrapped in `Arc<W>` for shared ownership across async tasks
- All public methods take `&self` (shared reference)
- Interior mutability via Mutex in the UTXO cache shards

## CRITICAL INVARIANT: Phase Ordering

Phases 2 and 3 are SWAPPED because Bitcoin allows transactions to spend outputs from earlier transactions in the SAME block. Outputs must exist in cache before transaction amounts are calculated.

**Phase 2 (Outputs) must run BEFORE Phase 3 (Transactions).**

## UTXO Cache Design

- **16-shard** LRU for concurrent access (key hash mod 16 selects shard)
- **UtxoKey**: 36 bytes stack-allocated (32-byte txid + 4-byte vout index) — no heap allocation
- **CachedOutput**: ~36 bytes with ScriptTypeTag enum and optional `Arc<str>` for address sharing
- **Atomic counters**: Lock-free statistics for hits/misses
- **Neo4j fallback**: `lookup_outputs_batch()` on cache miss (1-5% for recent blocks)
- **Batch operations**: `get_many_with_fallback()`, `remove_many()`

## Adding a New Domain Model

1. Define struct in `models.rs` with all derives and `///` doc comments on every field
2. Add conversion in `conversions.rs` for bitcoin crate type → domain type
3. Export from `mod.rs`
4. Add corresponding write method to `GraphWriter` trait in `writer/traits.rs`
5. Implement in `MockWriter` (`writer/mock.rs`)
6. Implement in `Neo4jWriter` (`writer/neo4j/mod.rs`)
7. Add Cypher query constant in `writer/neo4j/queries.rs`
8. Add BoltType conversion in `writer/neo4j/conversions.rs`
9. Write tests using MockWriter in `tests/domain/`
