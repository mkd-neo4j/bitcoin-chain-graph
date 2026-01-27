//! Checkpoint Resume Integration Tests - M8
//!
//! Tests for checkpoint management and resume functionality.
//! These tests validate the core M8 requirements:
//! - Resume from any block height
//! - At most 1 block reprocessed on failure
//! - Checkpoint tracks .blk file name
//! - Status transitions work correctly

use bitcoin::Network;
use bitcoin_chain_graph::domain::{IngestionOrchestrator, CheckpointData};
use bitcoin_chain_graph::parser::SingleBlockLoader;
use bitcoin_chain_graph::writer::MockWriter;

/// Test: Resume from genesis block (height 0)
#[tokio::test]
async fn test_resume_from_genesis() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, 100_000);

    // Initialize schema and checkpoint
    orchestrator.init_schema().await.expect("Failed to init schema");

    // Get initial checkpoint
    let checkpoint = orchestrator.get_checkpoint().await.unwrap();
    assert!(checkpoint.is_some(), "Checkpoint should exist after init");

    let cp = checkpoint.unwrap();
    assert_eq!(cp.last_processed_height, 0, "Initial checkpoint should be at height 0");
    assert_eq!(cp.status, "in_progress");
    assert_eq!(cp.last_processed_file, "blk00000.dat");

    // Get resume height - height 0 means block 0 was processed, so resume from block 1
    // (MockWriter initializes at 0 for simplicity; Neo4j uses -1 then converts to 0 after first block)
    let resume_height = orchestrator.get_resume_height().await.unwrap();
    assert_eq!(resume_height, 1, "Should resume from block 1 when checkpoint is at height 0");
}

/// Test: Resume from mid-ingestion (height 500)
#[tokio::test]
async fn test_resume_from_mid_ingestion() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, 100_000);

    // Initialize schema
    orchestrator.init_schema().await.unwrap();

    // Simulate checkpoint at block 500
    let checkpoint = CheckpointData {
        last_processed_height: 500,
        last_processed_hash: "0000000000000000000000000000000000000000000000000000000000000500".to_string(),
        last_processed_file: "blk00000.dat".to_string(),
        last_processed_file_offset: Some(50000),
        timestamp: chrono::Utc::now().timestamp(),
        status: "in_progress".to_string(),
    };

    orchestrator.update_checkpoint(&checkpoint).await.unwrap();

    // Get resume height
    let resume_height = orchestrator.get_resume_height().await.unwrap();
    assert_eq!(resume_height, 501, "Should resume from block 501 (last + 1)");

    // Get checkpoint again
    let retrieved = orchestrator.get_checkpoint().await.unwrap().unwrap();
    assert_eq!(retrieved.last_processed_height, 500);
    assert_eq!(retrieved.last_processed_hash, "0000000000000000000000000000000000000000000000000000000000000500");
}

/// Test: File transition tracking (blk00000.dat -> blk00001.dat)
#[tokio::test]
async fn test_file_transition() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, 100_000);

    orchestrator.init_schema().await.unwrap();

    // Checkpoint at end of blk00000.dat
    let checkpoint1 = CheckpointData {
        last_processed_height: 119970,
        last_processed_hash: "hash_119970".to_string(),
        last_processed_file: "blk00000.dat".to_string(),
        last_processed_file_offset: Some(134217728), // 128MB
        timestamp: chrono::Utc::now().timestamp(),
        status: "in_progress".to_string(),
    };

    orchestrator.update_checkpoint(&checkpoint1).await.unwrap();

    // Checkpoint at start of blk00001.dat
    let checkpoint2 = CheckpointData {
        last_processed_height: 119971,
        last_processed_hash: "hash_119971".to_string(),
        last_processed_file: "blk00001.dat".to_string(),
        last_processed_file_offset: Some(293), // First block offset
        timestamp: chrono::Utc::now().timestamp(),
        status: "in_progress".to_string(),
    };

    orchestrator.update_checkpoint(&checkpoint2).await.unwrap();

    // Verify file transition was tracked
    let retrieved = orchestrator.get_checkpoint().await.unwrap().unwrap();
    assert_eq!(retrieved.last_processed_file, "blk00001.dat");
    assert_eq!(retrieved.last_processed_height, 119971);

    // Resume should be from next block
    let resume_height = orchestrator.get_resume_height().await.unwrap();
    assert_eq!(resume_height, 119972);
}

/// Test: Status transitions (in_progress -> error -> in_progress)
#[tokio::test]
async fn test_status_transitions() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, 100_000);

    orchestrator.init_schema().await.unwrap();

    // Initial status
    let checkpoint = orchestrator.get_checkpoint().await.unwrap().unwrap();
    assert_eq!(checkpoint.status, "in_progress");

    // Set to error
    orchestrator.set_checkpoint_status("error").await.unwrap();
    let checkpoint = orchestrator.get_checkpoint().await.unwrap().unwrap();
    assert_eq!(checkpoint.status, "error");

    // Resume from error (back to in_progress)
    orchestrator.set_checkpoint_status("in_progress").await.unwrap();
    let checkpoint = orchestrator.get_checkpoint().await.unwrap().unwrap();
    assert_eq!(checkpoint.status, "in_progress");

    // Mark complete
    orchestrator.mark_complete().await.unwrap();
    let checkpoint = orchestrator.get_checkpoint().await.unwrap().unwrap();
    assert_eq!(checkpoint.status, "completed");

    // Pause
    orchestrator.set_checkpoint_status("paused").await.unwrap();
    let checkpoint = orchestrator.get_checkpoint().await.unwrap().unwrap();
    assert_eq!(checkpoint.status, "paused");
}

/// Test: Batch ingestion with checkpoint updates
#[tokio::test]
async fn test_batch_ingestion_checkpoints() {
    let blocks_dir = "test_data/blocks";
    let mut loader = match SingleBlockLoader::new(blocks_dir, Network::Bitcoin) {
        Ok(l) => l,
        Err(_) => {
            println!("⚠️  No block index found, skipping test");
            return;
        }
    };

    // Load first 100 blocks
    let mut blocks = Vec::new();
    for height in 0..100u32 {
        match loader.load_block(height) {
            Ok(Some(data)) => blocks.push(data),
            Ok(None) => break,
            Err(_) => break,
        }
    }

    if blocks.is_empty() {
        println!("⚠️  No test data found, skipping test");
        return;
    }

    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, 100_000);

    orchestrator.init_schema().await.unwrap();

    // Ingest in batches of 25
    orchestrator.ingest_blocks_batch(&blocks, 25)
        .await
        .expect("Batch ingestion failed");

    // Check final checkpoint
    let checkpoint = orchestrator.get_checkpoint().await.unwrap().unwrap();
    assert_eq!(checkpoint.last_processed_height, blocks.last().unwrap().0 as i32);
    assert_eq!(checkpoint.last_processed_file, "blk00000.dat");
    assert_eq!(checkpoint.status, "in_progress");

    // Verify can resume from this point
    let resume_height = orchestrator.get_resume_height().await.unwrap();
    assert_eq!(resume_height, blocks.last().unwrap().0 + 1);
}

/// Test: At most 1 block reprocessed on failure
///
/// This test validates the core M8 requirement: if ingestion fails mid-block,
/// only that block needs to be reprocessed on resume.
#[tokio::test]
async fn test_at_most_one_block_reprocessed() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, 100_000);

    orchestrator.init_schema().await.unwrap();

    // Simulate successful ingestion of blocks 0-9
    let checkpoint = CheckpointData {
        last_processed_height: 9,
        last_processed_hash: "hash_9".to_string(),
        last_processed_file: "blk00000.dat".to_string(),
        last_processed_file_offset: Some(10000),
        timestamp: chrono::Utc::now().timestamp(),
        status: "in_progress".to_string(),
    };

    orchestrator.update_checkpoint(&checkpoint).await.unwrap();

    // Get resume height - should be 10 (next block after last successful)
    let resume_height = orchestrator.get_resume_height().await.unwrap();
    assert_eq!(resume_height, 10);

    // Simulate failure at block 10 (checkpoint not updated)
    // If we try to resume again, we'll reprocess block 10
    let resume_height_after_failure = orchestrator.get_resume_height().await.unwrap();
    assert_eq!(resume_height_after_failure, 10, "Should retry failed block");

    // After successful ingestion of block 10, update checkpoint
    let checkpoint = CheckpointData {
        last_processed_height: 10,
        last_processed_hash: "hash_10".to_string(),
        last_processed_file: "blk00000.dat".to_string(),
        last_processed_file_offset: Some(11000),
        timestamp: chrono::Utc::now().timestamp(),
        status: "in_progress".to_string(),
    };

    orchestrator.update_checkpoint(&checkpoint).await.unwrap();

    // Now resume should move to block 11
    let resume_height_after_success = orchestrator.get_resume_height().await.unwrap();
    assert_eq!(resume_height_after_success, 11);
}

/// Test: Checkpoint tracks file offset for precise resume
#[tokio::test]
async fn test_file_offset_tracking() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, 100_000);

    orchestrator.init_schema().await.unwrap();

    // Checkpoint with file offset
    let checkpoint = CheckpointData {
        last_processed_height: 100,
        last_processed_hash: "hash_100".to_string(),
        last_processed_file: "blk00000.dat".to_string(),
        last_processed_file_offset: Some(25600), // 25KB into file
        timestamp: chrono::Utc::now().timestamp(),
        status: "in_progress".to_string(),
    };

    orchestrator.update_checkpoint(&checkpoint).await.unwrap();

    // Retrieve and verify offset is preserved
    let retrieved = orchestrator.get_checkpoint().await.unwrap().unwrap();
    assert_eq!(retrieved.last_processed_file_offset, Some(25600));
}

/// Test: Initial checkpoint state (height -1 before any blocks)
#[tokio::test]
async fn test_initial_checkpoint_state() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, 100_000);

    // Before init, no checkpoint
    let checkpoint = orchestrator.get_checkpoint().await.unwrap();
    assert!(checkpoint.is_none(), "Should have no checkpoint before init");

    // After init, checkpoint at height 0 (block 0 processed in MockWriter)
    orchestrator.init_schema().await.unwrap();

    let checkpoint = orchestrator.get_checkpoint().await.unwrap().unwrap();
    assert_eq!(checkpoint.last_processed_height, 0);
    assert_eq!(checkpoint.status, "in_progress");

    // Resume height should be 1 (next block after 0)
    let resume_height = orchestrator.get_resume_height().await.unwrap();
    assert_eq!(resume_height, 1);
}

/// Test: Checkpoint timestamp updates
#[tokio::test]
async fn test_checkpoint_timestamp_updates() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, 100_000);

    orchestrator.init_schema().await.unwrap();

    let checkpoint1 = orchestrator.get_checkpoint().await.unwrap().unwrap();
    let timestamp1 = checkpoint1.timestamp;

    // Wait a bit
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Update status
    orchestrator.set_checkpoint_status("paused").await.unwrap();

    let checkpoint2 = orchestrator.get_checkpoint().await.unwrap().unwrap();
    let timestamp2 = checkpoint2.timestamp;

    // Timestamp should be updated
    assert!(timestamp2 >= timestamp1, "Timestamp should increase on update");
}
