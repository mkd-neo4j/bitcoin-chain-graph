//! Neo4j integration tests
//!
//! These tests require a running Neo4j instance and are ignored by default.
//! To run them:
//!
//! 1. Start Neo4j:
//!    ```bash
//!    docker run -d --name neo4j-test \
//!      -p 7687:7687 -p 7474:7474 \
//!      -e NEO4J_AUTH=neo4j/testpassword \
//!      neo4j:latest
//!    ```
//!
//! 2. Run ignored tests:
//!    ```bash
//!    cargo test --test neo4j_integration -- --ignored
//!    ```
//!
//! 3. Clean up:
//!    ```bash
//!    docker stop neo4j-test && docker rm neo4j-test
//!    ```

use bitcoin_chain_graph::config::Neo4jConfig;
use bitcoin_chain_graph::domain::{BlockData, TransactionData, OutputData, InputData, CheckpointData};
use bitcoin_chain_graph::writer::{GraphWriter, Neo4jWriter};

/// Helper to create test Neo4j configuration
fn test_neo4j_config() -> Neo4jConfig {
    Neo4jConfig {
        uri: "bolt://localhost:7687".to_string(),
        user: "neo4j".to_string(),
        password: "testpassword".to_string(),
        database: "neo4j".to_string(),
        max_connections: 5,
        min_connections: 1,
        connection_timeout_secs: 10,
        fetch_size: 500,
        max_retries: 3,
    }
}

/// Helper to create test block data
fn test_block_data(height: u32) -> BlockData {
    BlockData {
        hash: format!("test_block_{}", height),
        height,
        previous_hash: if height > 0 {
            format!("test_block_{}", height - 1)
        } else {
            "0000000000000000000000000000000000000000000000000000000000000000".to_string()
        },
        merkle_root: format!("merkle_root_{}", height),
        timestamp: 1609459200 + (height as i64 * 600), // ~10 min blocks
        bits: "1d00ffff".to_string(),
        difficulty: 1.0,
        nonce: height * 1000,
        version: 1,
        tx_count: 1,
        size: 285,
        weight: 1000,
    }
}

/// Helper to create test transaction data
fn test_transaction_data(block_height: u32, index: u32) -> TransactionData {
    TransactionData {
        txid: format!("test_tx_{}_{}", block_height, index),
        block_height,
        block_hash: format!("test_block_{}", block_height),
        timestamp: 1609459200 + (block_height as i64 * 600),
        version: 1,
        locktime: 0,
        size: 250,
        vsize: 250,
        weight: 1000,
        is_coinbase: index == 0,
        total_input: Some(50000),
        total_output: Some(49000),
        fee: Some(1000),
    }
}

/// Helper to create test output data
fn test_output_data(txid: &str, index: u32) -> OutputData {
    OutputData {
        output_id: format!("{}:{}", txid, index),
        output_index: index,
        txid: txid.to_string(),
        amount: 5_000_000_000, // 50 BTC in satoshis
        script_pubkey: "76a914".to_string(),
        script_type: "P2PKH".to_string(),
        address: Some(format!("1TestAddress{}", index)),
    }
}

/// Helper to create test input data
#[allow(dead_code)]
fn test_input_data(txid: &str, index: u32, prev_txid: &str, prev_index: u32) -> InputData {
    InputData {
        input_id: format!("{}:{}", txid, index),
        input_index: index,
        txid: txid.to_string(),
        previous_txid: prev_txid.to_string(),
        previous_output_index: prev_index,
        script_sig: "".to_string(),
        sequence: 0xffffffff,
        witness: vec![],
    }
}

#[tokio::test]
#[ignore]
async fn test_neo4j_connection() {
    let config = test_neo4j_config();
    let writer = Neo4jWriter::new(config).await;

    assert!(writer.is_ok(), "Failed to connect to Neo4j: {:?}", writer.err());
}

#[tokio::test]
#[ignore]
async fn test_init_schema() {
    let config = test_neo4j_config();
    let writer = Neo4jWriter::new(config).await.expect("Failed to create writer");

    let result = writer.init_schema().await;
    assert!(result.is_ok(), "Failed to initialize schema: {:?}", result.err());
}

#[tokio::test]
#[ignore]
async fn test_write_blocks() {
    let config = test_neo4j_config();
    let writer = Neo4jWriter::new(config).await.expect("Failed to create writer");

    writer.init_schema().await.expect("Failed to init schema");

    let blocks = vec![
        test_block_data(0),
        test_block_data(1),
        test_block_data(2),
    ];

    let result = writer.write_blocks(&blocks).await;
    assert!(result.is_ok(), "Failed to write blocks: {:?}", result.err());
}

#[tokio::test]
#[ignore]
async fn test_write_transactions() {
    let config = test_neo4j_config();
    let writer = Neo4jWriter::new(config).await.expect("Failed to create writer");

    writer.init_schema().await.expect("Failed to init schema");

    // Create block first
    let blocks = vec![test_block_data(1)];
    writer.write_blocks(&blocks).await.expect("Failed to write blocks");

    // Create transactions
    let transactions = vec![
        test_transaction_data(1, 0),
        test_transaction_data(1, 1),
    ];

    let result = writer.write_transactions(&transactions).await;
    assert!(result.is_ok(), "Failed to write transactions: {:?}", result.err());
}

#[tokio::test]
#[ignore]
async fn test_write_outputs() {
    let config = test_neo4j_config();
    let writer = Neo4jWriter::new(config).await.expect("Failed to create writer");

    writer.init_schema().await.expect("Failed to init schema");

    // Create block and transaction first
    let blocks = vec![test_block_data(1)];
    writer.write_blocks(&blocks).await.expect("Failed to write blocks");

    let transactions = vec![test_transaction_data(1, 0)];
    writer.write_transactions(&transactions).await.expect("Failed to write transactions");

    // Create outputs
    let txid = "test_tx_1_0";
    let outputs = vec![
        test_output_data(txid, 0),
        test_output_data(txid, 1),
    ];

    let result = writer.write_outputs(&outputs).await;
    assert!(result.is_ok(), "Failed to write outputs: {:?}", result.err());
}

#[tokio::test]
#[ignore]
async fn test_checkpoint_operations() {
    let config = test_neo4j_config();
    let writer = Neo4jWriter::new(config).await.expect("Failed to create writer");

    writer.init_schema().await.expect("Failed to init schema");

    // Create checkpoint
    let result = writer.create_checkpoint().await;
    assert!(result.is_ok(), "Failed to create checkpoint: {:?}", result.err());

    // Get checkpoint
    let checkpoint = writer.get_checkpoint().await.expect("Failed to get checkpoint");
    assert!(checkpoint.is_some(), "Checkpoint should exist");

    let checkpoint_data = checkpoint.unwrap();
    assert_eq!(checkpoint_data.last_processed_height, 0);
    assert_eq!(checkpoint_data.status, "in_progress");

    // Update checkpoint
    let updated_checkpoint = CheckpointData {
        last_processed_height: 100,
        last_processed_hash: "test_block_100".to_string(),
        last_processed_file: "blk00001.dat".to_string(),
        last_processed_file_offset: Some(12345),
        timestamp: chrono::Utc::now().timestamp(),
        status: "in_progress".to_string(),
    };

    let result = writer.update_checkpoint(&updated_checkpoint).await;
    assert!(result.is_ok(), "Failed to update checkpoint: {:?}", result.err());

    // Verify update
    let checkpoint = writer.get_checkpoint().await.expect("Failed to get checkpoint");
    assert!(checkpoint.is_some());

    let checkpoint_data = checkpoint.unwrap();
    assert_eq!(checkpoint_data.last_processed_height, 100);
    assert_eq!(checkpoint_data.last_processed_hash, "test_block_100");

    // Mark complete
    let result = writer.mark_checkpoint_complete().await;
    assert!(result.is_ok(), "Failed to mark checkpoint complete: {:?}", result.err());

    // Verify complete
    let checkpoint = writer.get_checkpoint().await.expect("Failed to get checkpoint");
    assert!(checkpoint.is_some());
    assert_eq!(checkpoint.unwrap().status, "completed");
}

#[tokio::test]
#[ignore]
async fn test_idempotent_operations() {
    let config = test_neo4j_config();
    let writer = Neo4jWriter::new(config).await.expect("Failed to create writer");

    writer.init_schema().await.expect("Failed to init schema");

    let blocks = vec![test_block_data(5)];

    // Write blocks twice - should not error due to MERGE
    let result1 = writer.write_blocks(&blocks).await;
    assert!(result1.is_ok(), "First write failed: {:?}", result1.err());

    let result2 = writer.write_blocks(&blocks).await;
    assert!(result2.is_ok(), "Second write failed (not idempotent): {:?}", result2.err());
}
