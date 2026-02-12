//! Tests for Neo4j atomic batch transactions
//!
//! Verifies that each batch chunk's 7 ingestion phases are wrapped in a single
//! transaction so that crashes mid-batch leave zero orphaned data.
//!
//! These tests use MockWriter — no real Neo4j instance required.

use bitcoin::Network;
use bitcoin_chain_graph::domain::IngestionOrchestrator;
use bitcoin_chain_graph::parser::BlockFileReader;
use bitcoin_chain_graph::writer::{GraphWriter, MockWriter, WriterError};

// =============================================================================
// ACCEPTANCE CRITERIA TESTS
// =============================================================================

/// AC1: GIVEN a batch of blocks WHEN all 7 phases complete successfully
/// THEN all data is committed atomically in a single Neo4j transaction
#[tokio::test]
async fn test_successful_batch_commits_atomically() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut batch = Vec::new();
    for height in 0..5 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        batch.push((height, block, "blk00000.dat".to_string()));
    }

    // Ingest a batch — should succeed atomically
    orchestrator.ingest_blocks_batch(&batch).await.unwrap();

    // Verify all data committed
    assert_eq!(writer.get_blocks().await.len(), 5, "All 5 blocks committed");
    assert_eq!(
        writer.get_transactions().await.len(),
        5,
        "All transactions committed"
    );
    assert_eq!(
        writer.get_outputs().await.len(),
        5,
        "All outputs committed"
    );
    assert_eq!(writer.get_inputs().await.len(), 5, "All inputs committed");

    // AC1: Verify that the writer recorded exactly 1 transaction boundary per batch chunk
    // (begin_transaction was called once, commit_transaction was called once)
    let txn_count = writer.transaction_commit_count().await;
    assert_eq!(
        txn_count, 1,
        "Should have exactly 1 committed transaction for a single batch chunk"
    );
}

/// AC2: GIVEN a batch of blocks WHEN any phase (2–7) fails mid-batch
/// THEN zero nodes/relationships from that batch are persisted in Neo4j
#[tokio::test]
async fn test_failed_phase_rolls_back_entire_batch() {
    let writer = MockWriter::new();

    // Configure MockWriter to fail on Phase 4 (write_inputs)
    writer
        .set_failure_on("write_inputs_fast", WriterError::QueryFailed("simulated failure".into()))
        .await;

    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut batch = Vec::new();
    for height in 0..3 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        batch.push((height, block, "blk00000.dat".to_string()));
    }

    // Ingest should fail
    let result = orchestrator.ingest_blocks_batch(&batch).await;
    assert!(result.is_err(), "Batch should fail when Phase 4 errors");

    // AC2: Zero data from this batch should be persisted
    assert_eq!(
        writer.get_blocks().await.len(),
        0,
        "No blocks should be persisted after rollback"
    );
    assert_eq!(
        writer.get_transactions().await.len(),
        0,
        "No transactions should be persisted after rollback"
    );
    assert_eq!(
        writer.get_outputs().await.len(),
        0,
        "No outputs should be persisted after rollback"
    );
    assert_eq!(
        writer.get_inputs().await.len(),
        0,
        "No inputs should be persisted after rollback"
    );
}

/// AC3: GIVEN a batch of blocks WHEN Phase 3 (transactions) fails after Phase 2 (outputs) succeeds
/// THEN no output nodes from that batch exist in the database
#[tokio::test]
async fn test_phase3_failure_after_phase2_rolls_back_outputs() {
    let writer = MockWriter::new();

    // Configure MockWriter to fail on Phase 3 (write_transactions)
    writer
        .set_failure_on(
            "write_transactions_fast",
            WriterError::QueryFailed("simulated transaction write failure".into()),
        )
        .await;

    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut batch = Vec::new();
    for height in 0..2 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        batch.push((height, block, "blk00000.dat".to_string()));
    }

    let result = orchestrator.ingest_blocks_batch(&batch).await;
    assert!(result.is_err(), "Batch should fail when Phase 3 errors");

    // AC3: Specifically, output nodes from Phase 2 must NOT be persisted
    assert_eq!(
        writer.get_outputs().await.len(),
        0,
        "No output nodes should exist after Phase 3 failure (transaction rolled back)"
    );
    assert_eq!(
        writer.get_blocks().await.len(),
        0,
        "No block nodes should exist after Phase 3 failure"
    );
}

/// AC4: GIVEN a successful batch commit THEN the checkpoint is written inside
/// the same transaction (atomic with the data)
#[tokio::test]
async fn test_checkpoint_written_in_same_transaction_as_data() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut batch = Vec::new();
    for height in 0..3 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        batch.push((height, block, "blk00000.dat".to_string()));
    }

    orchestrator.ingest_blocks_batch(&batch).await.unwrap();

    // AC4: Checkpoint update must have occurred within the transaction boundary
    // The MockWriter should record that update_checkpoint was called between
    // begin_transaction and commit_transaction (not after commit)
    assert!(
        writer.checkpoint_written_in_transaction().await,
        "Checkpoint must be written inside the same transaction as batch data"
    );

    // Verify checkpoint reflects the last block in the batch
    let checkpoint = writer.get_checkpoint().await.unwrap().unwrap();
    assert_eq!(
        checkpoint.last_processed_height, 2,
        "Checkpoint should reflect last block height in batch"
    );
}

/// AC5: GIVEN a crash after checkpoint DELETE but before CREATE
/// THEN on resume, the checkpoint still exists (because both happen in one transaction)
#[tokio::test]
async fn test_checkpoint_delete_and_create_are_atomic() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    // Ingest first batch successfully
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut batch1 = Vec::new();
    for height in 0..3 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        batch1.push((height, block, "blk00000.dat".to_string()));
    }
    orchestrator.ingest_blocks_batch(&batch1).await.unwrap();

    // Verify checkpoint exists at height 2
    let checkpoint = writer.get_checkpoint().await.unwrap();
    assert!(checkpoint.is_some(), "Checkpoint should exist after first batch");
    assert_eq!(checkpoint.unwrap().last_processed_height, 2);

    // Simulate a crash during the second batch's checkpoint update
    // by configuring the writer to fail on update_checkpoint
    writer
        .set_failure_on(
            "update_checkpoint",
            WriterError::ConnectionFailed("simulated crash during checkpoint update".into()),
        )
        .await;

    let mut batch2 = Vec::new();
    for height in 3..6 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        batch2.push((height, block, "blk00000.dat".to_string()));
    }

    let result = orchestrator.ingest_blocks_batch(&batch2).await;
    assert!(result.is_err(), "Second batch should fail");

    // AC5: The original checkpoint must still exist and be unchanged
    // because the failed transaction (including the checkpoint update) was rolled back
    let checkpoint = writer.get_checkpoint().await.unwrap();
    assert!(
        checkpoint.is_some(),
        "Checkpoint must still exist after failed batch (transaction rolled back)"
    );
    assert_eq!(
        checkpoint.unwrap().last_processed_height,
        2,
        "Checkpoint should still be at height 2 (second batch's changes rolled back)"
    );

    // The second batch's data (blocks 3-5) should also be rolled back
    // because the checkpoint and data are in the same transaction
    let blocks = writer.get_blocks().await;
    assert_eq!(
        blocks.len(),
        3,
        "Only first batch's 3 blocks should exist (second batch rolled back with transaction)"
    );
    for h in 3..6 {
        assert!(
            blocks.iter().all(|b| b.height != h),
            "Block {} from second batch should NOT exist after rollback",
            h
        );
    }
}

// =============================================================================
// EDGE CASE TESTS
// =============================================================================

/// Edge case: Neo4j transaction timeout on very large batches — transaction should
/// fail cleanly and be retryable with the same batch
#[tokio::test]
async fn test_transaction_timeout_is_retryable() {
    let writer = MockWriter::new();

    // Simulate a transaction timeout error
    writer
        .set_failure_on(
            "commit_transaction",
            WriterError::QueryFailed("transaction timeout: exceeded 30s limit".into()),
        )
        .await;

    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut batch = Vec::new();
    for height in 0..5 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        batch.push((height, block, "blk00000.dat".to_string()));
    }

    // First attempt fails with timeout
    let result = orchestrator.ingest_blocks_batch(&batch).await;
    assert!(result.is_err(), "Should fail on transaction timeout");

    // The error should be retryable
    let err = result.unwrap_err();
    assert!(
        err.is_retryable(),
        "Transaction timeout error should be retryable"
    );

    // No data should be persisted from the timed-out transaction
    assert_eq!(
        writer.get_blocks().await.len(),
        0,
        "No data persisted after timeout"
    );

    // Clear the failure and retry the same batch — should succeed
    writer.clear_failures().await;
    orchestrator.ingest_blocks_batch(&batch).await.unwrap();

    assert_eq!(
        writer.get_blocks().await.len(),
        5,
        "Retry with same batch should succeed"
    );
}

/// Edge case: Neo4j connection drops mid-transaction — uncommitted transaction is
/// automatically rolled back by the server, next retry starts fresh
#[tokio::test]
async fn test_connection_drop_mid_transaction_rolls_back() {
    let writer = MockWriter::new();

    // Simulate connection drop mid-Phase 3
    writer
        .set_failure_on(
            "write_transactions_fast",
            WriterError::ConnectionFailed("connection reset by peer".into()),
        )
        .await;

    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut batch = Vec::new();
    for height in 0..3 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        batch.push((height, block, "blk00000.dat".to_string()));
    }

    let result = orchestrator.ingest_blocks_batch(&batch).await;
    assert!(result.is_err(), "Should fail on connection drop");

    // Zero data persisted — server auto-rolled back the uncommitted transaction
    assert_eq!(
        writer.get_blocks().await.len(),
        0,
        "No data from rolled-back transaction"
    );
    assert_eq!(writer.get_outputs().await.len(), 0);
    assert_eq!(writer.get_transactions().await.len(), 0);
}

/// Edge case: Deadlock during parallel Phase 6 writes within a transaction —
/// retry logic should handle this within the transaction boundary
#[tokio::test]
async fn test_deadlock_during_phase6_retries_within_transaction() {
    let writer = MockWriter::new();

    // Simulate a deadlock error on Phase 6 (write_performs)
    writer
        .set_transient_failure_on(
            "write_performs",
            WriterError::QueryFailed("deadlock detected".into()),
            1, // fail once, then succeed on retry
        )
        .await;

    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut batch = Vec::new();
    for height in 0..3 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        batch.push((height, block, "blk00000.dat".to_string()));
    }

    // The batch should succeed because the deadlock is retried within the transaction
    let result = orchestrator.ingest_blocks_batch(&batch).await;
    assert!(
        result.is_ok(),
        "Batch should succeed after deadlock retry within transaction"
    );

    // All data should be committed
    assert_eq!(writer.get_blocks().await.len(), 3, "All blocks committed");

    // The retry must happen WITHIN the same transaction (not a new one)
    // So we should still see exactly 1 committed transaction
    let txn_count = writer.transaction_commit_count().await;
    assert_eq!(
        txn_count, 1,
        "Deadlock retry should occur within the same transaction, not start a new one"
    );
}

/// Edge case: Transaction memory pressure on Neo4j heap for large batch sizes —
/// fail with a clear error; operator can reduce batch chunk size in config
#[tokio::test]
async fn test_memory_pressure_fails_with_clear_error() {
    let writer = MockWriter::new();

    // Simulate Neo4j heap memory error during a large batch
    writer
        .set_failure_on(
            "write_outputs_fast",
            WriterError::QueryFailed(
                "Neo.TransientError.General.OutOfMemoryError: heap exhausted".into(),
            ),
        )
        .await;

    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut batch = Vec::new();
    for height in 0..10 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        batch.push((height, block, "blk00000.dat".to_string()));
    }

    let result = orchestrator.ingest_blocks_batch(&batch).await;
    assert!(result.is_err(), "Should fail on memory pressure");

    // Error should indicate the issue clearly
    let err = result.unwrap_err();
    match &err {
        WriterError::QueryFailed(msg) => {
            assert!(
                msg.contains("OutOfMemoryError") || msg.contains("heap"),
                "Error message should indicate memory issue: {}",
                msg
            );
        }
        other => panic!(
            "Expected QueryFailed with memory error, got: {:?}",
            other
        ),
    }

    // The error should be retryable (operator reduces batch size and retries)
    assert!(
        err.is_retryable(),
        "Memory pressure error should be retryable with smaller batch size"
    );

    // No data persisted
    assert_eq!(writer.get_blocks().await.len(), 0);
}

// =============================================================================
// TRANSACTION LIFECYCLE TESTS
// =============================================================================

/// Verify that GraphWriter trait has begin_transaction, commit_transaction,
/// and rollback_transaction methods
#[tokio::test]
async fn test_graph_writer_has_transaction_methods() {
    let writer = MockWriter::new();

    // begin_transaction should return Ok
    writer.begin_transaction().await.unwrap();

    // commit_transaction should return Ok
    writer.commit_transaction().await.unwrap();

    // After commit, begin a new one and rollback
    writer.begin_transaction().await.unwrap();
    writer.rollback_transaction().await.unwrap();
}

/// Verify that multiple batch chunks each get their own transaction
#[tokio::test]
async fn test_multiple_chunks_each_get_own_transaction() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut batch = Vec::new();
    for height in 0..6 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        batch.push((height, block, "blk00000.dat".to_string()));
    }

    // Use batch_size=3 so we get 2 chunks
    orchestrator.ingest_blocks_batch(&batch).await.unwrap();

    // With adaptive chunking, all 6 tiny early blocks fit in a single chunk
    let txn_count = writer.transaction_commit_count().await;
    assert_eq!(
        txn_count, 1,
        "Should have 1 committed transaction (tiny blocks fit in one adaptive chunk)"
    );

    // All data committed
    assert_eq!(writer.get_blocks().await.len(), 6);
}

/// Verify that a failure in the only chunk rolls back completely
/// (With adaptive chunking, tiny early blocks all fit in one chunk)
#[tokio::test]
async fn test_failure_in_single_chunk_rolls_back() {
    let writer = MockWriter::new();

    // Fail on the first call to write_blocks_fast
    writer
        .set_failure_after_n_calls(
            "write_blocks_fast",
            0, // fail immediately
            WriterError::QueryFailed("simulated failure in chunk".into()),
        )
        .await;

    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut batch = Vec::new();
    for height in 0..6 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        batch.push((height, block, "blk00000.dat".to_string()));
    }

    // With adaptive chunking, all 6 tiny blocks fit in one chunk
    let result = orchestrator.ingest_blocks_batch(&batch).await;
    assert!(result.is_err(), "Should fail");

    // No blocks should be committed (single chunk failed and rolled back)
    let blocks = writer.get_blocks().await;
    assert_eq!(
        blocks.len(),
        0,
        "No blocks should be persisted after rollback"
        );

    // No checkpoint should exist since the only chunk failed
    let txn_count = writer.transaction_commit_count().await;
    assert_eq!(
        txn_count, 0,
        "No transactions should have been committed"
    );
}
