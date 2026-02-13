---
title: UTXO Cache Neo4j Fallback
status: completed
priority: high
affected-files:
  - src/writer/traits.rs
  - src/writer/neo4j/mod.rs
  - src/writer/neo4j/queries.rs
  - src/writer/neo4j/conversions.rs
  - src/writer/mock.rs
  - src/domain/utxo/cache.rs
  - src/domain/ingestion.rs
  - tests/domain/utxo/no_neo4j_fallback.rs
---

# UTXO Cache Neo4j Fallback

## Summary

When the UTXO cache has misses (due to LRU eviction or stale snapshots), fall back to querying Neo4j Output nodes instead of hard-failing. The cache is ~100K entries but Bitcoin has millions of UTXOs; live mode loads a stale cache snapshot and has no pre-warming, causing 30%+ miss rates on resume. The data exists in Neo4j — it just needs to be fetched on cache miss.

This reverses the deliberate "no fallback" design from the cache-only refactor. That design assumed the cache would always be complete, which breaks when the LRU evicts old entries and live mode resumes from a checkpoint.

## Acceptance Criteria

1. GIVEN a `GraphWriter` implementation WHEN `lookup_outputs_batch` is called with a list of output IDs THEN it returns a `Vec<OutputLookupResult>` containing `output_id: String`, `output_index: u32`, `amount: u64` (satoshis as i64 in Neo4j, cast back), `script_type: String`, and `address: Option<String>` for each found output

2. GIVEN `UtxoCache` with some entries missing WHEN `get_many_with_fallback(&self, keys: &[UtxoKey], writer: &W)` is called THEN it first checks the cache, collects misses, calls `writer.lookup_outputs_batch()` for the misses, converts results to `CachedOutput`, inserts them into the cache, and returns the combined `HashMap<UtxoKey, CachedOutput>`

3. GIVEN `get_many_with_fallback` is called and Neo4j returns results for some but not all misses WHEN there are still-missing keys after the fallback THEN it returns `Err(WriterError::QueryFailed(...))` listing the count and sample IDs of truly missing outputs

4. GIVEN `get_many_with_fallback` is called and ALL misses are resolved by Neo4j THEN it returns `Ok(HashMap)` with all requested entries and logs a `tracing::info!` with `cache_hits`, `neo4j_fallbacks`, and `total_keys` fields

5. GIVEN `IngestionOrchestrator::process_transactions_phase` at line ~955 of `ingestion.rs` WHEN it needs UTXO data for inputs THEN it calls `self.utxo_cache.get_many_with_fallback(&all_input_keys, self.writer.as_ref()).await` instead of `self.utxo_cache.get_many_or_fail(&all_input_keys)`

6. GIVEN `MockWriter` WHEN `lookup_outputs_batch` is called THEN it returns an empty `Vec` (no outputs found), allowing existing tests using `get_many_or_fail` to continue working unchanged

7. GIVEN `UtxoCache::get_many_with_fallback` resolves misses from Neo4j WHEN those same keys are requested again THEN they are served from cache (the fallback inserted them)

8. GIVEN the Neo4j query for output lookup WHEN it runs THEN it uses a single `UNWIND $outputIds AS oid MATCH (o:Output {outputId: oid}) OPTIONAL MATCH (o)-[:LOCKED_TO]->(a:Address) RETURN o.outputId, o.outputIndex, o.amount, o.scriptType, a.address` query (batch, not individual queries in a loop)

## Edge Cases

- Neo4j connection failure during fallback — `lookup_outputs_batch` returns `WriterError::ConnectionFailed` which propagates up; do not silently skip the fallback
- Output exists in Neo4j but has no LOCKED_TO relationship (NULL_DATA, UNKNOWN script types) — `address` field is `None`, `ScriptTypeTag` parsed from `scriptType` string via `FromStr`
- Empty miss list after cache check — skip the Neo4j call entirely, return cache results only
- Neo4j returns `amount` as `i64` (all Neo4j integers are i64) — cast to `u64` in the conversion layer
- `get_many_or_fail` must continue to exist unchanged for callers that want strict cache-only behaviour

## Out of Scope

- Do NOT remove `get_many_or_fail` — it is still valid for contexts where the cache is guaranteed complete (e.g., forward-only ingestion from genesis). Removing it would break the `no_neo4j_fallback.rs` tests and any code path that relies on strict cache-only mode.
- Do NOT add pre-warming to live mode — that is a separate optimisation. This feature makes live mode work without pre-warming.
- Do NOT change the cache persistence format or LRU eviction policy — the cache design is fine; this feature adds resilience when the cache is incomplete.
- Do NOT make `UtxoCache` generic over `GraphWriter` — the fallback writer is passed as a parameter to `get_many_with_fallback`, keeping the cache struct simple and non-generic.

## Technical Notes

- The `IngestionOrchestrator` already holds `writer: Arc<W>` so passing `self.writer.as_ref()` to the cache method is zero-cost.
- `CachedOutput` reconstruction from Neo4j: `output_index` (u32), `amount` (i64→u64 cast), `script_type` via `ScriptTypeTag::from_str(&script_type_string)`, `address` as `Option<Arc<str>>`.
- The new `GraphWriter` method signature: `async fn lookup_outputs_batch(&self, output_ids: &[String]) -> Result<Vec<OutputLookupResult>>` where `OutputLookupResult` is a new struct in `src/domain/models.rs`.
- The Cypher query constant goes in `queries.rs` as `LOOKUP_OUTPUTS_BATCH_QUERY`.
- `get_many_with_fallback` is `async` (calls writer) — this changes the call site in `ingestion.rs` from sync to `.await`. The method already runs in an async context so this is straightforward.
- `UtxoCache` stats should track a new `neo4j_fallbacks: AtomicU64` counter for observability.
- **Rejected**: Making `UtxoCache` generic over `W: GraphWriter` again — this was the original design and was removed because it complicated construction, testing, and the 16-shard LRU. Passing the writer as a method parameter keeps the cache simple.
- **Rejected**: Querying Neo4j one-by-one per miss — UNWIND batch is required per project conventions (never individual queries in loops).
- Existing test file `tests/domain/utxo/no_neo4j_fallback.rs` must be updated: rename to reflect the new dual-mode design, keep tests for `get_many_or_fail` (cache-only), add tests for `get_many_with_fallback` (cache + Neo4j).
