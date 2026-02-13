//! Tests verifying the cache-only path works alongside the Neo4j fallback
//!
//! These tests assert that:
//! 1. UtxoCache does not require a GraphWriter generic parameter for construction
//! 2. Cache misses via `get` and `get_many_or_fail` return errors without querying Neo4j
//! 3. `get_many_or_fail` (cache-only) still works for strict cache-only callers
//! 4. Cache stats track hits and misses, and persistence works without a writer

use bitcoin_chain_graph::domain::utxo::{CachedOutput, ScriptTypeTag, UtxoCache, UtxoKey};
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
// 1. UtxoCache no longer requires GraphWriter generic parameter
// =========================================================================

/// UtxoCache can be created without a GraphWriter implementation.
#[test]
fn test_utxo_cache_created_without_writer() {
    let cache = UtxoCache::new(1000);
    assert!(cache.is_empty());
    assert!(cache.capacity() >= 992); // 1000/16 * 16
}

/// Cache insert and get work without any writer dependency.
#[test]
fn test_cache_get_returns_error_on_miss() {
    let cache = UtxoCache::new(1000);

    let key = test_key(1, 0);
    cache.insert(key, test_output(5_000_000_000));

    // Hit should work
    let result = cache.get(&key);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().amount, 5_000_000_000);

    // Miss should return Err (not query Neo4j)
    let missing_key = test_key(99, 0);
    let result = cache.get(&missing_key);
    assert!(
        result.is_err(),
        "Cache miss should return Err, not query Neo4j"
    );
}

// =========================================================================
// 2. GraphWriter trait has no lookup methods
// =========================================================================

/// Verify lookup_output is not part of GraphWriter.
/// If the trait still required it, MockWriter wouldn't compile without it.
#[test]
fn test_graph_writer_has_no_lookup_output() {
    fn assert_no_lookup<T: bitcoin_chain_graph::writer::GraphWriter>() {}
    assert_no_lookup::<bitcoin_chain_graph::writer::MockWriter>();
}

// =========================================================================
// 3. Batch get is cache-only (no fallback)
// =========================================================================

/// get_many returns all hits and misses without any Neo4j fallback.
#[test]
fn test_get_many_cache_only() {
    let cache = UtxoCache::new(1000);

    for i in 0..10u8 {
        cache.insert(test_key(i, 0), test_output(i as u64 * 100));
    }

    // Batch get: 5 hits + 5 misses
    let keys: Vec<UtxoKey> = (0..10u8)
        .map(|i| {
            if i < 5 {
                test_key(i, 0)
            } else {
                test_key(i + 100, 0)
            }
        })
        .collect();

    let (found, misses) = cache.get_many(&keys);
    assert_eq!(found.len(), 5);
    assert_eq!(misses.len(), 5);
}

/// get_many_or_fail replaces get_many_with_fallback — errors on missing keys.
#[test]
fn test_get_many_or_fail_errors_on_missing() {
    let cache = UtxoCache::new(1000);

    for i in 0..5u8 {
        cache.insert(test_key(i, 0), test_output(i as u64 * 100));
    }

    // All requested keys are in cache — should succeed
    let keys: Vec<UtxoKey> = (0..5u8).map(|i| test_key(i, 0)).collect();
    let result = cache.get_many_or_fail(&keys);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 5);

    // Some keys missing — should return error (no Neo4j fallback)
    let keys_with_misses: Vec<UtxoKey> = (0..10u8).map(|i| test_key(i, 0)).collect();
    let result = cache.get_many_or_fail(&keys_with_misses);
    assert!(
        result.is_err(),
        "Should error when UTXOs missing from cache (no Neo4j fallback)"
    );
}

// =========================================================================
// 4. Cache stats still track hits and misses
// =========================================================================

#[test]
fn test_cache_stats_no_fallback_concept() {
    let cache = UtxoCache::new(1000);

    cache.insert(test_key(1, 0), test_output(100));

    // Hit
    let _ = cache.get(&test_key(1, 0));
    // Miss
    let _ = cache.get(&test_key(99, 0));

    let stats = cache.stats();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 1);
}

// =========================================================================
// 5. Persistence still works without writer
// =========================================================================

#[test]
fn test_persistence_without_writer() {
    let cache = UtxoCache::new(1000);

    for i in 0..20u8 {
        cache.insert(test_key(i, 0), test_output(i as u64 * 1_000_000));
    }

    let dir = std::env::temp_dir().join("utxo_cache_test_no_writer");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cache.bin");

    let saved = cache.save_to_file(&path, 500).unwrap();
    assert_eq!(saved, 20);

    let cache2 = UtxoCache::new(1000);
    let loaded = cache2.load_from_file(&path, Some(500)).unwrap();
    assert_eq!(loaded, 20);
    assert_eq!(cache2.len(), 20);

    std::fs::remove_dir_all(&dir).ok();
}
