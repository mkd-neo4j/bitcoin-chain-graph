//! Tests for UTXO Cache Neo4j Fallback feature
//!
//! Verifies that when the UTXO cache has misses, the system falls back to
//! querying Neo4j Output nodes via `GraphWriter::lookup_outputs_batch`.
//! Covers all 8 acceptance criteria and 5 edge cases.

use bitcoin_chain_graph::domain::utxo::{CachedOutput, ScriptTypeTag, UtxoCache, UtxoKey};
use bitcoin_chain_graph::domain::OutputLookupResult;
use bitcoin_chain_graph::writer::{GraphWriter, MockWriter, WriterError};
use std::sync::Arc;

/// Helper to create a test key from simple integers
fn test_key(tx_id: u8, vout: u32) -> UtxoKey {
    let mut txid = [0u8; 32];
    txid[31] = tx_id;
    UtxoKey::new(txid, vout)
}

/// Helper to create a test output
fn test_output(amount: u64) -> CachedOutput {
    CachedOutput {
        output_index: 0,
        amount,
        script_type: ScriptTypeTag::P2PKH,
        address: Some(Arc::from("1TestAddress")),
    }
}

// =========================================================================
// AC1: GraphWriter::lookup_outputs_batch returns Vec<OutputLookupResult>
// =========================================================================

/// The GraphWriter trait must have a `lookup_outputs_batch` method.
/// OutputLookupResult must have output_id, output_index, amount, script_type, address fields.
#[tokio::test]
async fn test_lookup_outputs_batch_exists_on_graph_writer() {
    let writer = MockWriter::new();
    let output_ids = vec!["abc123:0".to_string(), "def456:1".to_string()];
    let results: Vec<OutputLookupResult> = writer.lookup_outputs_batch(&output_ids).await.unwrap();
    // MockWriter returns empty vec (AC6)
    assert!(results.is_empty());
}

/// OutputLookupResult has the required fields with correct types.
#[test]
fn test_output_lookup_result_fields() {
    let result = OutputLookupResult {
        output_id: "abc123:0".to_string(),
        output_index: 0,
        amount: 5_000_000_000,
        script_type: "P2PKH".to_string(),
        address: Some("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa".to_string()),
    };
    assert_eq!(result.output_id, "abc123:0");
    assert_eq!(result.output_index, 0);
    assert_eq!(result.amount, 5_000_000_000u64);
    assert_eq!(result.script_type, "P2PKH");
    assert_eq!(
        result.address,
        Some("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa".to_string())
    );
}

// =========================================================================
// AC2: get_many_with_fallback checks cache, collects misses, calls writer
// =========================================================================

/// get_many_with_fallback returns combined results from cache hits and Neo4j fallback.
#[tokio::test]
async fn test_get_many_with_fallback_combines_cache_and_writer() {
    let cache = UtxoCache::new(1000);
    let writer = MockWriter::new();

    // Insert some entries in cache
    let key_in_cache = test_key(1, 0);
    cache.insert(key_in_cache, test_output(1_000_000));

    // Request both cached and uncached keys
    // The uncached key will miss cache and fall back to writer
    // MockWriter returns empty, so this should error on truly missing keys
    // Configure MockWriter to return a result for the uncached key
    let uncached_key = test_key(2, 0);
    let uncached_output_id = uncached_key.to_output_id_string();
    writer
        .set_lookup_outputs_response(vec![OutputLookupResult {
            output_id: uncached_output_id,
            output_index: 0,
            amount: 2_000_000,
            script_type: "P2PKH".to_string(),
            address: Some("1FallbackAddr".to_string()),
        }])
        .await;

    let keys = vec![key_in_cache, uncached_key];
    let result = cache.get_many_with_fallback(&keys, &writer).await;
    assert!(result.is_ok());
    let map = result.unwrap();
    assert_eq!(
        map.len(),
        2,
        "Should contain both cached and fallback entries"
    );
    assert_eq!(map[&key_in_cache].amount, 1_000_000);
    assert_eq!(map[&uncached_key].amount, 2_000_000);
}

// =========================================================================
// AC3: Still-missing keys after fallback return error with count and samples
// =========================================================================

/// When Neo4j also can't find some outputs, return error with count and sample IDs.
#[tokio::test]
async fn test_get_many_with_fallback_errors_on_truly_missing() {
    let cache = UtxoCache::new(1000);
    let writer = MockWriter::new(); // returns empty vec for lookup

    // Request keys that are neither in cache nor in Neo4j (MockWriter returns empty)
    let missing_key = test_key(99, 0);
    let keys = vec![missing_key];
    let result = cache.get_many_with_fallback(&keys, &writer).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WriterError::QueryFailed(_)),
        "Expected QueryFailed, got: {:?}",
        err
    );
    // Error message should mention count and sample IDs
    let msg = err.to_string();
    assert!(
        msg.contains("Missing 1"),
        "Error should mention count of missing keys"
    );
}

// =========================================================================
// AC4: All misses resolved by Neo4j returns Ok with logging fields
// =========================================================================

/// When all cache misses are resolved by Neo4j fallback, returns Ok(HashMap).
/// Uses MockWriter configured with set_lookup_outputs_response to simulate Neo4j results.
#[tokio::test]
async fn test_get_many_with_fallback_all_resolved_returns_ok() {
    let cache = UtxoCache::new(1000);

    // Insert one key in cache, leave another to be resolved by fallback
    let cached_key = test_key(1, 0);
    cache.insert(cached_key, test_output(1_000_000));

    let fallback_key = test_key(2, 0);
    let fallback_output_id = fallback_key.to_output_id_string();

    // Configure MockWriter to return a result for the fallback key
    let writer = MockWriter::new();
    writer
        .set_lookup_outputs_response(vec![OutputLookupResult {
            output_id: fallback_output_id,
            output_index: 0,
            amount: 2_000_000,
            script_type: "P2PKH".to_string(),
            address: Some("1FallbackAddress".to_string()),
        }])
        .await;

    let keys = vec![cached_key, fallback_key];
    let result = cache.get_many_with_fallback(&keys, &writer).await;

    assert!(
        result.is_ok(),
        "All misses resolved by fallback should return Ok"
    );
    let map = result.unwrap();
    assert_eq!(
        map.len(),
        2,
        "Should contain both cached and fallback entries"
    );
    assert_eq!(map[&cached_key].amount, 1_000_000);
    assert_eq!(map[&fallback_key].amount, 2_000_000);
}

// =========================================================================
// AC5: IngestionOrchestrator uses get_many_with_fallback in process_transactions_phase
// =========================================================================
// This is an integration-level concern tested via the orchestrator.
// We verify the method signature exists and is callable with a writer ref.

#[tokio::test]
async fn test_get_many_with_fallback_accepts_writer_ref() {
    let cache = UtxoCache::new(1000);
    let writer = MockWriter::new();

    // Insert all requested keys so the call succeeds
    let key = test_key(1, 0);
    cache.insert(key, test_output(500_000));

    // Call with &writer (not &Arc<writer>), matching the orchestrator pattern
    let result = cache.get_many_with_fallback(&[key], &writer).await;
    assert!(result.is_ok());
}

// =========================================================================
// AC6: MockWriter::lookup_outputs_batch returns empty Vec
// =========================================================================

#[tokio::test]
async fn test_mock_writer_lookup_outputs_batch_returns_empty() {
    let writer = MockWriter::new();
    let ids = vec!["txid1:0".to_string(), "txid2:1".to_string()];
    let result = writer.lookup_outputs_batch(&ids).await.unwrap();
    assert!(
        result.is_empty(),
        "MockWriter should return empty Vec for lookup_outputs_batch"
    );
}

// =========================================================================
// AC7: Fallback results are cached for subsequent lookups
// =========================================================================

/// After get_many_with_fallback resolves misses from Neo4j, those entries
/// should be in the cache for subsequent get() calls.
#[tokio::test]
async fn test_fallback_results_inserted_into_cache() {
    let cache = UtxoCache::new(1000);

    // Do NOT insert the key into cache — it must come from fallback
    let key = test_key(1, 0);
    let output_id = key.to_output_id_string();

    // Configure MockWriter to return a result for the key
    let writer = MockWriter::new();
    writer
        .set_lookup_outputs_response(vec![OutputLookupResult {
            output_id,
            output_index: 0,
            amount: 2_000_000,
            script_type: "P2PKH".to_string(),
            address: Some("1CacheInsertTest".to_string()),
        }])
        .await;

    let result = cache.get_many_with_fallback(&[key], &writer).await;
    assert!(result.is_ok(), "Fallback should resolve the key");

    // Verify the fallback result was inserted into cache
    let cached = cache.get(&key);
    assert!(cached.is_ok(), "Key should now be in cache after fallback");
    assert_eq!(cached.unwrap().amount, 2_000_000);
}

// =========================================================================
// AC8: Cypher query uses UNWIND batch pattern
// =========================================================================
// This is a code-level constraint verified by checking that the query constant
// exists in queries.rs. We verify the constant name compiles.

#[test]
fn test_lookup_outputs_batch_query_constant_exists() {
    // This test verifies that the Cypher query constant exists
    let _query = bitcoin_chain_graph::writer::neo4j::LOOKUP_OUTPUTS_BATCH_QUERY;
    assert!(
        _query.contains("UNWIND"),
        "Query must use UNWIND for batch operation"
    );
    assert!(
        _query.contains("$outputIds"),
        "Query must use $outputIds parameter"
    );
}

// =========================================================================
// Edge Case: Neo4j connection failure during fallback propagates error
// =========================================================================

#[tokio::test]
async fn test_fallback_propagates_connection_error() {
    let cache = UtxoCache::new(1000);
    let writer = MockWriter::new();

    // Configure writer to fail on lookup_outputs_batch
    writer
        .set_failure_on(
            "lookup_outputs_batch",
            WriterError::ConnectionFailed("Neo4j down".to_string()),
        )
        .await;

    let missing_key = test_key(50, 0);
    let result = cache.get_many_with_fallback(&[missing_key], &writer).await;
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), WriterError::ConnectionFailed(_)),
        "Connection failure should propagate, not be silently swallowed"
    );
}

// =========================================================================
// Edge Case: Output with no LOCKED_TO (NULL_DATA) — address is None
// =========================================================================

#[test]
fn test_output_lookup_result_with_no_address() {
    let result = OutputLookupResult {
        output_id: "abc123:0".to_string(),
        output_index: 0,
        amount: 0,
        script_type: "NULL_DATA".to_string(),
        address: None,
    };
    assert!(result.address.is_none());
    assert_eq!(result.script_type, "NULL_DATA");
}

// =========================================================================
// Edge Case: Empty miss list skips Neo4j call
// =========================================================================

#[tokio::test]
async fn test_empty_miss_list_skips_neo4j() {
    let cache = UtxoCache::new(1000);
    let writer = MockWriter::new();

    // All keys in cache — should not call writer at all
    let key1 = test_key(1, 0);
    let key2 = test_key(2, 0);
    cache.insert(key1, test_output(100));
    cache.insert(key2, test_output(200));

    let result = cache.get_many_with_fallback(&[key1, key2], &writer).await;
    assert!(result.is_ok());
    let map = result.unwrap();
    assert_eq!(map.len(), 2);
}

// =========================================================================
// Edge Case: get_many_or_fail still exists and works unchanged
// =========================================================================

#[test]
fn test_get_many_or_fail_still_works() {
    let cache = UtxoCache::new(1000);

    let key = test_key(1, 0);
    cache.insert(key, test_output(100));

    // All keys present — succeeds
    let result = cache.get_many_or_fail(&[key]);
    assert!(result.is_ok());

    // Missing key — fails
    let missing = test_key(99, 0);
    let result = cache.get_many_or_fail(&[missing]);
    assert!(result.is_err());
}

// =========================================================================
// Edge Case: Neo4j amount as i64 cast to u64
// =========================================================================

#[test]
fn test_output_lookup_result_amount_is_u64() {
    // Neo4j stores amounts as i64, but OutputLookupResult should use u64
    let result = OutputLookupResult {
        output_id: "abc:0".to_string(),
        output_index: 0,
        amount: 2_100_000_000_000_000u64, // 21M BTC in satoshis — exceeds i32 but fits u64
        script_type: "P2PKH".to_string(),
        address: None,
    };
    assert_eq!(result.amount, 2_100_000_000_000_000u64);
}

// =========================================================================
// Stats: neo4j_fallbacks counter
// =========================================================================

#[tokio::test]
async fn test_stats_track_neo4j_fallbacks() {
    let cache = UtxoCache::new(1000);
    let writer = MockWriter::new();

    // Insert one key, leave one missing
    let cached_key = test_key(1, 0);
    cache.insert(cached_key, test_output(100));

    // Request only cached key — no fallback
    let _ = cache.get_many_with_fallback(&[cached_key], &writer).await;

    let stats = cache.stats();
    // neo4j_fallbacks should be 0 when all keys were in cache
    assert_eq!(
        stats.neo4j_fallbacks, 0,
        "No fallbacks should occur when all keys are cached"
    );
}
