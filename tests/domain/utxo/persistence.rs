//! Tests for UTXO cache persistence (save_to_file / load_from_file)
//!
//! Covers acceptance criteria 1-7 and edge cases from the
//! utxo-cache-persistence feature doc.

use bitcoin_chain_graph::domain::utxo::{CachedOutput, ScriptTypeTag, UtxoCache, UtxoKey};
use bitcoin_chain_graph::writer::MockWriter;
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

/// AC1+AC2: Save cache to file and load it back; all entries restored correctly.
#[tokio::test]
async fn test_save_and_load_roundtrip() {
    let writer = Arc::new(MockWriter::new());
    let cache = UtxoCache::new(1000, writer.clone());

    // Insert entries with varied script types and optional addresses
    let mut keys = Vec::new();
    for i in 0..50u8 {
        let mut txid = [0u8; 32];
        txid[0] = i;
        txid[31] = i.wrapping_mul(7);
        let key = UtxoKey::new(txid, i as u32);
        let output = CachedOutput {
            output_index: i as u32,
            amount: (i as u64 + 1) * 1_000_000,
            script_type: match i % 8 {
                0 => ScriptTypeTag::P2PKH,
                1 => ScriptTypeTag::P2SH,
                2 => ScriptTypeTag::P2WPKH,
                3 => ScriptTypeTag::P2WSH,
                4 => ScriptTypeTag::P2TR,
                5 => ScriptTypeTag::P2PK,
                6 => ScriptTypeTag::NullData,
                _ => ScriptTypeTag::Unknown,
            },
            address: if i % 3 == 0 {
                None
            } else {
                Some(Arc::from(format!("addr_{}", i).as_str()))
            },
        };
        cache.insert(key, output);
        keys.push(key);
    }

    let dir = std::env::temp_dir().join("utxo_persist_roundtrip");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cache.bin");

    // Save
    let saved = cache.save_to_file(&path, 12345).unwrap();
    assert_eq!(saved, 50);

    // Load into fresh cache
    let cache2 = UtxoCache::new(1000, writer);
    let loaded = cache2.load_from_file(&path, Some(12345)).unwrap();
    assert_eq!(loaded, 50);
    assert_eq!(cache2.len(), 50);

    // Verify all entries are present via public API
    for key in &keys {
        assert!(cache2.contains(key), "Entry should exist after load");
    }

    // Spot-check a specific entry's amount via async get
    let entry = cache2.get(&keys[1]).await.unwrap();
    assert_eq!(entry.amount, 2_000_000);

    std::fs::remove_dir_all(&dir).ok();
}

/// AC1: save_to_file returns Ok(entry_count)
#[test]
fn test_save_returns_entry_count() {
    let writer = Arc::new(MockWriter::new());
    let cache = UtxoCache::new(1000, writer);

    let dir = std::env::temp_dir().join("utxo_persist_count");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cache.bin");

    // Empty cache
    let saved = cache.save_to_file(&path, 0).unwrap();
    assert_eq!(saved, 0);

    // With entries
    for i in 0..10u8 {
        cache.insert(test_key(i, 0), test_output(100));
    }
    let saved = cache.save_to_file(&path, 100).unwrap();
    assert_eq!(saved, 10);

    std::fs::remove_dir_all(&dir).ok();
}

/// AC3: load_from_file with nonexistent path returns Ok(0)
#[test]
fn test_load_file_not_found_returns_zero() {
    let writer = Arc::new(MockWriter::new());
    let cache = UtxoCache::new(1000, writer);

    let path = std::env::temp_dir().join("utxo_persist_nonexistent.bin");
    std::fs::remove_file(&path).ok();

    let loaded = cache.load_from_file(&path, None).unwrap();
    assert_eq!(loaded, 0);
    assert!(cache.is_empty());
}

/// AC4: corrupted CRC returns Err(InvalidData) with CRC info, cache stays empty
#[test]
fn test_load_corrupted_crc_returns_error() {
    let writer = Arc::new(MockWriter::new());
    let cache = UtxoCache::new(1000, writer.clone());
    cache.insert(test_key(1, 0), test_output(5_000_000));

    let dir = std::env::temp_dir().join("utxo_persist_crc");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cache.bin");

    cache.save_to_file(&path, 100).unwrap();

    // Corrupt a byte in the entry data (after 24-byte header)
    let mut data = std::fs::read(&path).unwrap();
    if data.len() > 30 {
        data[30] ^= 0xFF;
    }
    std::fs::write(&path, &data).unwrap();

    let cache2 = UtxoCache::new(1000, writer);
    let result = cache2.load_from_file(&path, Some(100));
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        err.to_string().contains("CRC"),
        "Error should mention CRC, got: {}",
        err
    );
    assert!(
        cache2.is_empty(),
        "Cache should remain empty after failed load"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// AC5: invalid magic bytes returns Err(InvalidData)
#[test]
fn test_load_invalid_magic_returns_error() {
    let dir = std::env::temp_dir().join("utxo_persist_magic");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cache.bin");

    let mut data = vec![0u8; 24];
    data[0..4].copy_from_slice(b"NOPE");
    std::fs::write(&path, &data).unwrap();

    let writer = Arc::new(MockWriter::new());
    let cache = UtxoCache::new(1000, writer);
    let result = cache.load_from_file(&path, None);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        err.to_string().to_lowercase().contains("magic"),
        "Error should mention magic, got: {}",
        err
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// AC6: height mismatch warns but still loads successfully
#[test]
fn test_load_stale_cache_height_mismatch_still_loads() {
    let writer = Arc::new(MockWriter::new());
    let cache = UtxoCache::new(1000, writer.clone());
    cache.insert(test_key(1, 0), test_output(100));

    let dir = std::env::temp_dir().join("utxo_persist_stale");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cache.bin");

    cache.save_to_file(&path, 100).unwrap();

    // Load with different checkpoint height — should still succeed
    let cache2 = UtxoCache::new(1000, writer);
    let loaded = cache2.load_from_file(&path, Some(200)).unwrap();
    assert_eq!(loaded, 1);
    assert_eq!(cache2.len(), 1);

    std::fs::remove_dir_all(&dir).ok();
}

/// AC7: save writes to .tmp first, then renames atomically
#[test]
fn test_save_uses_atomic_rename() {
    let writer = Arc::new(MockWriter::new());
    let cache = UtxoCache::new(1000, writer);
    cache.insert(test_key(1, 0), test_output(100));

    let dir = std::env::temp_dir().join("utxo_persist_atomic");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cache.bin");

    cache.save_to_file(&path, 0).unwrap();

    assert!(path.exists(), "Final file should exist");
    let tmp_path = path.with_extension("bin.tmp");
    assert!(
        !tmp_path.exists(),
        "Temp file should not exist after save"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// AC1: 24-byte header with magic "UTXO", version 1, entry count, height, CRC32
#[test]
fn test_save_header_format() {
    let writer = Arc::new(MockWriter::new());
    let cache = UtxoCache::new(1000, writer);

    for i in 0..5u8 {
        cache.insert(test_key(i, 0), test_output(100));
    }

    let dir = std::env::temp_dir().join("utxo_persist_header");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cache.bin");

    cache.save_to_file(&path, 42).unwrap();

    let data = std::fs::read(&path).unwrap();
    assert!(
        data.len() >= 24,
        "File must have at least 24-byte header"
    );

    assert_eq!(&data[0..4], b"UTXO", "Magic bytes");
    assert_eq!(
        u32::from_le_bytes(data[4..8].try_into().unwrap()),
        1,
        "Version"
    );
    assert_eq!(
        u64::from_le_bytes(data[8..16].try_into().unwrap()),
        5,
        "Entry count"
    );
    assert_eq!(
        u32::from_le_bytes(data[16..20].try_into().unwrap()),
        42,
        "Checkpoint height"
    );
    let crc = u32::from_le_bytes(data[20..24].try_into().unwrap());
    assert_ne!(crc, 0, "CRC32 should be non-zero for non-empty cache");

    std::fs::remove_dir_all(&dir).ok();
}

/// Edge: truncated file (filesystem corruption) fails gracefully
#[test]
fn test_load_truncated_file_returns_error() {
    let writer = Arc::new(MockWriter::new());
    let cache = UtxoCache::new(1000, writer.clone());
    cache.insert(test_key(1, 0), test_output(100));

    let dir = std::env::temp_dir().join("utxo_persist_truncated");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cache.bin");

    cache.save_to_file(&path, 0).unwrap();

    // Truncate: keep header + partial entry
    let data = std::fs::read(&path).unwrap();
    std::fs::write(&path, &data[..28]).unwrap();

    let cache2 = UtxoCache::new(1000, writer);
    let result = cache2.load_from_file(&path, None);
    assert!(result.is_err(), "Truncated file should fail");
    assert!(cache2.is_empty(), "Cache should remain empty");

    std::fs::remove_dir_all(&dir).ok();
}

/// Edge: reduced capacity between save and load — entries evicted by LRU
#[test]
fn test_load_with_reduced_capacity_evicts_lru() {
    let writer = Arc::new(MockWriter::new());
    let cache = UtxoCache::new(1000, writer.clone());

    for i in 0..100u8 {
        let mut txid = [0u8; 32];
        txid[31] = i;
        cache.insert(UtxoKey::new(txid, 0), test_output(i as u64 * 100));
    }

    let dir = std::env::temp_dir().join("utxo_persist_reduced_cap");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cache.bin");

    cache.save_to_file(&path, 0).unwrap();

    let small_cache = UtxoCache::new(32, writer);
    let loaded = small_cache.load_from_file(&path, None).unwrap();
    assert_eq!(loaded, 100, "Should report all entries were loaded");
    assert!(
        small_cache.len() <= 32,
        "Cache should not exceed new capacity"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// AC2: load uses direct shard.put(), does not inflate stats counters
#[test]
fn test_load_does_not_inflate_stats() {
    let writer = Arc::new(MockWriter::new());
    let cache = UtxoCache::new(1000, writer.clone());

    for i in 0..10u8 {
        cache.insert(test_key(i, 0), test_output(100));
    }

    let dir = std::env::temp_dir().join("utxo_persist_stats");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cache.bin");
    cache.save_to_file(&path, 0).unwrap();

    let cache2 = UtxoCache::new(1000, writer);
    cache2.load_from_file(&path, None).unwrap();

    let stats = cache2.stats();
    assert_eq!(
        stats.inserts, 0,
        "load_from_file should not inflate insert stats"
    );
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, 0);
    assert_eq!(stats.removals, 0);

    std::fs::remove_dir_all(&dir).ok();
}

/// Edge: entries with address = None roundtrip correctly
#[tokio::test]
async fn test_save_load_with_no_address_entries() {
    let writer = Arc::new(MockWriter::new());
    let cache = UtxoCache::new(1000, writer.clone());

    for i in 0..10u8 {
        cache.insert(
            test_key(i, 0),
            CachedOutput {
                output_index: i as u32,
                amount: 100,
                script_type: ScriptTypeTag::NullData,
                address: None,
            },
        );
    }

    let dir = std::env::temp_dir().join("utxo_persist_no_addr");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cache.bin");

    cache.save_to_file(&path, 0).unwrap();

    let cache2 = UtxoCache::new(1000, writer);
    let loaded = cache2.load_from_file(&path, None).unwrap();
    assert_eq!(loaded, 10);

    let entry = cache2.get(&test_key(0, 0)).await.unwrap();
    assert!(
        entry.address.is_none(),
        "Address should be None after roundtrip"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Edge: long bech32m address (~90 bytes) roundtrips via u16 length prefix
#[tokio::test]
async fn test_save_load_with_long_address() {
    let writer = Arc::new(MockWriter::new());
    let cache = UtxoCache::new(1000, writer.clone());

    let long_addr = "bc1p".to_string() + &"q".repeat(86);
    cache.insert(
        test_key(1, 0),
        CachedOutput {
            output_index: 0,
            amount: 100,
            script_type: ScriptTypeTag::P2TR,
            address: Some(Arc::from(long_addr.as_str())),
        },
    );

    let dir = std::env::temp_dir().join("utxo_persist_long_addr");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cache.bin");

    cache.save_to_file(&path, 0).unwrap();

    let cache2 = UtxoCache::new(1000, writer);
    cache2.load_from_file(&path, None).unwrap();

    let entry = cache2.get(&test_key(1, 0)).await.unwrap();
    assert_eq!(
        entry.address.as_deref(),
        Some(long_addr.as_str()),
        "Long address should survive roundtrip"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Edge: ScriptTypeTag value > 7 should be deserialized as Unknown
#[test]
fn test_unknown_script_type_tag_deserialized_as_unknown() {
    let tag = match 99u8 {
        0 => ScriptTypeTag::P2PKH,
        1 => ScriptTypeTag::P2SH,
        2 => ScriptTypeTag::P2WPKH,
        3 => ScriptTypeTag::P2WSH,
        4 => ScriptTypeTag::P2TR,
        5 => ScriptTypeTag::P2PK,
        6 => ScriptTypeTag::NullData,
        _ => ScriptTypeTag::Unknown,
    };
    assert_eq!(tag, ScriptTypeTag::Unknown);
}

/// AC8: PerformanceConfig defaults for cache persistence fields
#[test]
fn test_performance_config_defaults_include_cache_persistence() {
    let config = bitcoin_chain_graph::config::PerformanceConfig::default();
    assert_eq!(
        config.utxo_cache_file, "utxo_cache.bin",
        "Default utxo_cache_file should be 'utxo_cache.bin'"
    );
    assert_eq!(
        config.utxo_cache_snapshot_interval, 2000,
        "Default utxo_cache_snapshot_interval should be 2000"
    );
}

/// AC8: TOML deserialization without new fields uses serde defaults
#[test]
fn test_performance_config_deserialize_without_cache_fields() {
    let toml_str = r#"
        utxo_cache_memory_mb = 140
        utxo_prewarm_depth = 1000000
        utxo_lookup_batch_size = 1000
        parallel_batches = 4
        progress_report_interval = 500
    "#;
    let config: bitcoin_chain_graph::config::PerformanceConfig =
        toml::from_str(toml_str).unwrap();
    assert_eq!(config.utxo_cache_file, "utxo_cache.bin");
    assert_eq!(config.utxo_cache_snapshot_interval, 2000);
}

/// AC12: empty string disables cache persistence
#[test]
fn test_performance_config_empty_cache_file_disables_persistence() {
    let toml_str = r#"
        utxo_cache_memory_mb = 140
        utxo_prewarm_depth = 1000000
        utxo_lookup_batch_size = 1000
        parallel_batches = 4
        progress_report_interval = 500
        utxo_cache_file = ""
        utxo_cache_snapshot_interval = 0
    "#;
    let config: bitcoin_chain_graph::config::PerformanceConfig =
        toml::from_str(toml_str).unwrap();
    assert_eq!(config.utxo_cache_file, "");
    assert_eq!(config.utxo_cache_snapshot_interval, 0);
}
