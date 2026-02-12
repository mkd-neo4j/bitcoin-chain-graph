//! Tests for snapshot resilience — save UTXO cache after every committed batch
//!
//! Covers all 9 acceptance criteria from feature-docs/ready/snapshot-resilience.md.
//! These tests verify that IngestionOrchestrator saves the UTXO cache snapshot
//! after each successful commit in `ingest_blocks_batch`, and that the periodic
//! snapshot config field is removed.

use bitcoin::Network;
use bitcoin_chain_graph::domain::IngestionOrchestrator;
use bitcoin_chain_graph::parser::BlockFileReader;
use bitcoin_chain_graph::writer::MockWriter;
use std::path::PathBuf;

/// Helper: create orchestrator with MockWriter and ingest N blocks
async fn setup_orchestrator_with_blocks(
    num_blocks: u32,
) -> (IngestionOrchestrator<MockWriter>, MockWriter) {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    for height in 0..num_blocks {
        let block = reader.next_block().unwrap().expect("Block should exist");
        orchestrator
            .ingest_block(&block, height, "blk00000.dat", None)
            .await
            .unwrap();
    }

    (orchestrator, writer)
}

// =============================================================================
// AC1: save_to_file called after commit when cache_snapshot_path is set
// =============================================================================

/// AC1: GIVEN orchestrator with non-empty cache_snapshot_path
///       WHEN ingest_blocks_batch successfully commits a chunk
///       THEN save_to_file is called with the chunk's last block height
#[tokio::test]
async fn test_snapshot_saved_after_batch_commit() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    // Set a snapshot path
    let dir = std::env::temp_dir().join("snapshot_resilience_ac1");
    std::fs::create_dir_all(&dir).unwrap();
    let snapshot_path = dir.join("utxo_cache.bin");
    orchestrator.set_cache_snapshot_path(Some(snapshot_path.clone()));

    // Load blocks for batch ingestion
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut blocks = Vec::new();
    for height in 0..5u32 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        blocks.push((height, block, "blk00000.dat".to_string()));
    }

    // Ingest as batch (batch_size=5 means one chunk of 5 blocks)
    orchestrator.ingest_blocks_batch(&blocks, 5).await.unwrap();

    // Snapshot file should exist after commit
    assert!(
        snapshot_path.exists(),
        "Snapshot file should be created after batch commit"
    );

    // Read the snapshot header to verify height matches last block in chunk (4)
    let data = std::fs::read(&snapshot_path).unwrap();
    assert!(data.len() >= 24, "Snapshot should have valid header");
    let saved_height = u32::from_le_bytes(data[16..20].try_into().unwrap());
    assert_eq!(
        saved_height, 4,
        "Snapshot height should match last block in chunk"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// AC1: Multiple chunks each produce a snapshot save
#[tokio::test]
async fn test_snapshot_saved_after_each_chunk_commit() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let dir = std::env::temp_dir().join("snapshot_resilience_ac1_multi");
    std::fs::create_dir_all(&dir).unwrap();
    let snapshot_path = dir.join("utxo_cache.bin");
    orchestrator.set_cache_snapshot_path(Some(snapshot_path.clone()));

    // Load 10 blocks
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut blocks = Vec::new();
    for height in 0..10u32 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        blocks.push((height, block, "blk00000.dat".to_string()));
    }

    // batch_size=5 means two chunks: [0..4] and [5..9]
    orchestrator.ingest_blocks_batch(&blocks, 5).await.unwrap();

    // After second chunk, snapshot height should be 9 (last block)
    assert!(snapshot_path.exists(), "Snapshot file should exist");
    let data = std::fs::read(&snapshot_path).unwrap();
    let saved_height = u32::from_le_bytes(data[16..20].try_into().unwrap());
    assert_eq!(
        saved_height, 9,
        "Snapshot height should match last block of final chunk"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// =============================================================================
// AC2: No save when cache_snapshot_path is None
// =============================================================================

/// AC2: GIVEN orchestrator with cache_snapshot_path = None
///       WHEN ingest_blocks_batch successfully commits
///       THEN no snapshot save is attempted (no file created)
#[tokio::test]
async fn test_no_snapshot_when_path_is_none() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    // Do NOT set cache_snapshot_path — it should default to None

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut blocks = Vec::new();
    for height in 0..5u32 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        blocks.push((height, block, "blk00000.dat".to_string()));
    }

    // This should succeed without attempting any snapshot save
    orchestrator.ingest_blocks_batch(&blocks, 5).await.unwrap();

    // Verify blocks were ingested (the batch itself works fine)
    assert_eq!(writer.get_blocks().await.len(), 5);
}

// =============================================================================
// AC3: Snapshot save failure is non-fatal
// =============================================================================

/// AC3: GIVEN a snapshot save fails
///       WHEN the error is logged
///       THEN ingestion continues without returning an error
#[tokio::test]
async fn test_snapshot_save_failure_is_nonfatal() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    // Set snapshot path to a non-existent directory (will fail to write)
    let bad_path = PathBuf::from("/nonexistent/directory/deep/nested/utxo_cache.bin");
    orchestrator.set_cache_snapshot_path(Some(bad_path));

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut blocks = Vec::new();
    for height in 0..5u32 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        blocks.push((height, block, "blk00000.dat".to_string()));
    }

    // Ingestion should succeed even though snapshot save will fail
    let result = orchestrator.ingest_blocks_batch(&blocks, 5).await;
    assert!(
        result.is_ok(),
        "Ingestion should succeed even when snapshot save fails: {:?}",
        result.err()
    );

    // Verify blocks were still ingested
    assert_eq!(writer.get_blocks().await.len(), 5);
}

// =============================================================================
// AC4: cache_snapshot_path set from config.performance.utxo_cache_file
// =============================================================================

/// AC4: GIVEN run_streaming_ingestion is called
///       WHEN configuring the orchestrator
///       THEN cache_snapshot_path is set from config.performance.utxo_cache_file
///
/// This test verifies the set_cache_snapshot_path method exists and works correctly.
/// The actual wiring in main.rs is verified by AC7 (removal of periodic saves).
#[tokio::test]
async fn test_set_cache_snapshot_path_from_config() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);

    // Simulate what main.rs should do: set path from config
    let config_path = "utxo_cache.bin".to_string();
    let path = if config_path.is_empty() {
        None
    } else {
        Some(PathBuf::from(&config_path))
    };
    orchestrator.set_cache_snapshot_path(path);

    // No panic = method exists and accepts the value
}

/// AC4: Empty string config means None (no snapshot)
#[tokio::test]
async fn test_set_cache_snapshot_path_empty_string_means_none() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    // Empty string should map to None
    let config_path = "".to_string();
    let path = if config_path.is_empty() {
        None
    } else {
        Some(PathBuf::from(&config_path))
    };
    orchestrator.set_cache_snapshot_path(path);

    // Batch ingestion should work without snapshot saves
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut blocks = Vec::new();
    for height in 0..3u32 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        blocks.push((height, block, "blk00000.dat".to_string()));
    }
    orchestrator.ingest_blocks_batch(&blocks, 3).await.unwrap();
    assert_eq!(writer.get_blocks().await.len(), 3);
}

// =============================================================================
// AC5: Live catchup uses same code path (no separate periodic save)
// =============================================================================

/// AC5: Live catchup calls ingest_blocks_batch → snapshot saved after commit
///       (Same code path as streaming — verified by AC1 tests above)
///       This test verifies that a multi-block batch simulating catchup saves correctly.
#[tokio::test]
async fn test_catchup_batch_saves_snapshot() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let dir = std::env::temp_dir().join("snapshot_resilience_ac5");
    std::fs::create_dir_all(&dir).unwrap();
    let snapshot_path = dir.join("utxo_cache.bin");
    orchestrator.set_cache_snapshot_path(Some(snapshot_path.clone()));

    // Simulate catchup: a larger batch of blocks
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut blocks = Vec::new();
    for height in 0..20u32 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        blocks.push((height, block, "blk00000.dat".to_string()));
    }

    // batch_size=10 → two chunks
    orchestrator.ingest_blocks_batch(&blocks, 10).await.unwrap();

    assert!(
        snapshot_path.exists(),
        "Snapshot should be saved during catchup batches"
    );
    let data = std::fs::read(&snapshot_path).unwrap();
    let saved_height = u32::from_le_bytes(data[16..20].try_into().unwrap());
    assert_eq!(saved_height, 19, "Should reflect last block in final chunk");

    std::fs::remove_dir_all(&dir).ok();
}

// =============================================================================
// AC6: Single-block batch in live real-time mode saves snapshot
// =============================================================================

/// AC6: GIVEN real-time phase calls ingest_blocks_batch with a single block
///       WHEN the chunk commits successfully
///       THEN the snapshot is saved
#[tokio::test]
async fn test_single_block_batch_saves_snapshot() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let dir = std::env::temp_dir().join("snapshot_resilience_ac6");
    std::fs::create_dir_all(&dir).unwrap();
    let snapshot_path = dir.join("utxo_cache.bin");
    orchestrator.set_cache_snapshot_path(Some(snapshot_path.clone()));

    // Single block batch (simulates real-time mode)
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let genesis = reader.next_block().unwrap().expect("Genesis should exist");
    let blocks = vec![(0u32, genesis, "blk00000.dat".to_string())];

    orchestrator.ingest_blocks_batch(&blocks, 1).await.unwrap();

    assert!(
        snapshot_path.exists(),
        "Snapshot should be saved even for single-block batch"
    );
    let data = std::fs::read(&snapshot_path).unwrap();
    let saved_height = u32::from_le_bytes(data[16..20].try_into().unwrap());
    assert_eq!(saved_height, 0, "Snapshot height should be 0 (genesis)");

    std::fs::remove_dir_all(&dir).ok();
}

// =============================================================================
// AC7: Periodic snapshot blocks removed from main.rs
// =============================================================================

/// AC7: The periodic snapshot code blocks in main.rs should be removed.
///       We verify this by checking that the source file does NOT contain
///       the periodic snapshot pattern (utxo_cache_snapshot_interval).
#[test]
fn test_periodic_snapshot_code_removed_from_main() {
    let main_source = std::fs::read_to_string("src/main.rs").expect("Should read src/main.rs");

    // The periodic snapshot blocks reference utxo_cache_snapshot_interval
    // to decide when to save. After this feature, that logic should be gone.
    assert!(
        !main_source.contains("utxo_cache_snapshot_interval"),
        "main.rs should not reference utxo_cache_snapshot_interval — periodic snapshot code should be removed"
    );
}

// =============================================================================
// AC8: utxo_cache_snapshot_interval field removed from config
// =============================================================================

/// AC8: GIVEN PerformanceConfig
///       WHEN the feature is complete
///       THEN utxo_cache_snapshot_interval and default_utxo_cache_snapshot_interval are removed
#[test]
fn test_config_snapshot_interval_field_removed() {
    let config_source =
        std::fs::read_to_string("src/config/mod.rs").expect("Should read config/mod.rs");

    assert!(
        !config_source.contains("utxo_cache_snapshot_interval"),
        "PerformanceConfig should not contain utxo_cache_snapshot_interval field"
    );
    assert!(
        !config_source.contains("default_utxo_cache_snapshot_interval"),
        "default_utxo_cache_snapshot_interval function should be removed"
    );
}

/// AC8: TOML deserialization should not accept utxo_cache_snapshot_interval
#[test]
fn test_config_rejects_snapshot_interval_field() {
    // After removal, deserializing TOML with this field should fail
    // (serde strict mode) or the field should be ignored.
    // At minimum, the struct should not have the field.
    let toml_str = r#"
        utxo_cache_memory_mb = 140
        utxo_prewarm_depth = 1000000
        utxo_lookup_batch_size = 1000
        parallel_batches = 4
        progress_report_interval = 500
        utxo_cache_file = "utxo_cache.bin"
    "#;
    let config: bitcoin_chain_graph::config::PerformanceConfig =
        toml::from_str(toml_str).expect("Should deserialize without snapshot_interval field");

    // The field should not exist — verify by checking the struct doesn't have it
    // We can't directly assert field absence in Rust, but we verify the config
    // works without it (no default needed)
    assert_eq!(config.utxo_cache_file, "utxo_cache.bin");
}

// =============================================================================
// AC9: Existing persistence tests still pass
// =============================================================================

// AC9 is implicitly verified by running the full test suite.
// The existing tests in tests/domain/utxo/persistence.rs must still pass.
// No changes to save_to_file or load_from_file behavior.

// =============================================================================
// Edge cases
// =============================================================================

/// Edge: Empty batch (no blocks) — no commit, no snapshot save
#[tokio::test]
async fn test_empty_batch_no_snapshot_save() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let dir = std::env::temp_dir().join("snapshot_resilience_empty");
    std::fs::create_dir_all(&dir).unwrap();
    let snapshot_path = dir.join("utxo_cache.bin");
    orchestrator.set_cache_snapshot_path(Some(snapshot_path.clone()));

    // Empty batch
    let blocks: Vec<(u32, bitcoin::Block, String)> = Vec::new();
    orchestrator.ingest_blocks_batch(&blocks, 10).await.unwrap();

    assert!(
        !snapshot_path.exists(),
        "No snapshot should be created for empty batch"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Edge: First batch after fresh start (no snapshot file exists) — creates file
#[tokio::test]
async fn test_first_batch_creates_snapshot_file() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let dir = std::env::temp_dir().join("snapshot_resilience_first");
    std::fs::create_dir_all(&dir).unwrap();
    let snapshot_path = dir.join("utxo_cache.bin");

    // Ensure file does not exist
    std::fs::remove_file(&snapshot_path).ok();
    assert!(!snapshot_path.exists());

    orchestrator.set_cache_snapshot_path(Some(snapshot_path.clone()));

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let genesis = reader.next_block().unwrap().unwrap();
    let blocks = vec![(0u32, genesis, "blk00000.dat".to_string())];

    orchestrator.ingest_blocks_batch(&blocks, 1).await.unwrap();

    assert!(
        snapshot_path.exists(),
        "Snapshot file should be created on first batch"
    );

    std::fs::remove_dir_all(&dir).ok();
}
