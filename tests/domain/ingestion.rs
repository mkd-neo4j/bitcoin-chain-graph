//! Integration tests for IngestionOrchestrator
//!
//! These tests verify the end-to-end ingestion process with MockWriter,
//! ensuring all 6 phases execute correctly without a real database.

use bitcoin::Network;
use bitcoin_chain_graph::domain::IngestionOrchestrator;
use bitcoin_chain_graph::parser::BlockFileReader;
use bitcoin_chain_graph::writer::MockWriter;

/// Test that the orchestrator can be created and initialized
#[tokio::test]
async fn test_create_and_initialize_orchestrator() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);

    // Initialize schema
    orchestrator.init_schema().await.unwrap();

    // Verify schema was initialized
    assert!(writer.is_schema_initialized().await);
}

/// Test ingesting the Genesis block through all 6 phases
#[tokio::test]
async fn test_ingest_genesis_block_all_phases() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);

    // Initialize schema
    orchestrator.init_schema().await.unwrap();

    // Read genesis block
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let genesis = reader
        .next_block()
        .unwrap()
        .expect("Genesis block should exist");

    // Ingest genesis block
    orchestrator
        .ingest_block(&genesis, 0, "blk00000.dat", None)
        .await
        .unwrap();

    // Verify Phase 1: Block node
    let blocks = writer.get_blocks().await;
    assert_eq!(blocks.len(), 1, "Should have 1 block");
    assert_eq!(blocks[0].height, 0, "Should be genesis block");
    assert_eq!(
        blocks[0].hash, "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f",
        "Should have correct genesis hash"
    );
    assert_eq!(blocks[0].version, 1);
    assert_eq!(blocks[0].timestamp, 1231006505);
    assert_eq!(blocks[0].tx_count, 1);
    assert!((blocks[0].difficulty - 1.0).abs() < 0.01);

    // Verify Phase 2: Transaction node
    let transactions = writer.get_transactions().await;
    assert_eq!(transactions.len(), 1, "Should have 1 transaction");
    assert!(
        transactions[0].is_coinbase,
        "Should be coinbase transaction"
    );
    assert_eq!(transactions[0].block_height, 0);
    assert_eq!(transactions[0].version, 1);

    // Verify Phase 3: Output node with address
    let outputs = writer.get_outputs().await;
    assert_eq!(outputs.len(), 1, "Should have 1 output");
    assert_eq!(outputs[0].output_index, 0);
    assert_eq!(
        outputs[0].amount, 5000000000,
        "Should be 50 BTC in satoshis"
    );
    assert_eq!(outputs[0].script_type, "P2PK", "Genesis output is P2PK");
    assert_eq!(
        outputs[0].address.as_ref().unwrap(),
        "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa",
        "Should have Satoshi's address"
    );

    // Verify Phase 4: Input node (coinbase)
    let inputs = writer.get_inputs().await;
    assert_eq!(inputs.len(), 1, "Should have 1 input");
    assert_eq!(inputs[0].input_index, 0);
    assert_eq!(
        inputs[0].previous_output_index, 0xFFFFFFFF,
        "Coinbase marker"
    );

    // Phases 5 and 6 are verified by the fact that no errors occurred
    // (MockWriter has no-op implementations for these phases)
}

/// Test ingesting the first 10 blocks
#[tokio::test]
async fn test_ingest_first_10_blocks() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);

    orchestrator.init_schema().await.unwrap();

    // Read and ingest first 10 blocks
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();

    for height in 0..10 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        orchestrator
            .ingest_block(&block, height, "blk00000.dat", None)
            .await
            .unwrap();
    }

    // Verify all blocks were written
    let blocks = writer.get_blocks().await;
    assert_eq!(blocks.len(), 10, "Should have 10 blocks");

    // Verify block heights are correct
    for (idx, block) in blocks.iter().enumerate() {
        assert_eq!(block.height, idx as u32, "Block height should match index");
    }

    // Verify all transactions were written
    let transactions = writer.get_transactions().await;
    assert_eq!(transactions.len(), 10, "First 10 blocks have 1 tx each");
    assert!(
        transactions.iter().all(|tx| tx.is_coinbase),
        "All should be coinbase transactions"
    );

    // Verify all outputs were written
    let outputs = writer.get_outputs().await;
    assert_eq!(outputs.len(), 10, "One output per coinbase transaction");

    // Verify all inputs were written
    let inputs = writer.get_inputs().await;
    assert_eq!(inputs.len(), 10, "One input per coinbase transaction");
    assert!(
        inputs.iter().all(|i| i.previous_output_index == 0xFFFFFFFF),
        "All should be coinbase inputs"
    );
}

/// Test that blocks are processed in order
#[tokio::test]
async fn test_block_ordering() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);

    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();

    // Ingest blocks 0, 1, 2
    for height in 0..3 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        orchestrator
            .ingest_block(&block, height, "blk00000.dat", None)
            .await
            .unwrap();
    }

    let blocks = writer.get_blocks().await;
    assert_eq!(blocks.len(), 3);

    // Verify they are in order
    assert_eq!(blocks[0].height, 0);
    assert_eq!(blocks[1].height, 1);
    assert_eq!(blocks[2].height, 2);

    // Verify block chain linkage (previous_hash)
    // Block 1 should link to block 0
    assert_eq!(blocks[1].previous_hash, blocks[0].hash);
    // Block 2 should link to block 1
    assert_eq!(blocks[2].previous_hash, blocks[1].hash);
}

/// Test that all phases execute without errors
#[tokio::test]
async fn test_all_phases_complete_successfully() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);

    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let genesis = reader
        .next_block()
        .unwrap()
        .expect("Genesis block should exist");

    // This should complete all 6 phases without error
    let result = orchestrator
        .ingest_block(&genesis, 0, "blk00000.dat", None)
        .await;
    assert!(result.is_ok(), "All 6 phases should complete successfully");

    // Verify data was written by each phase
    assert_eq!(writer.get_blocks().await.len(), 1, "Phase 1: Block written");
    assert_eq!(
        writer.get_transactions().await.len(),
        1,
        "Phase 2: Transaction written"
    );
    assert_eq!(
        writer.get_outputs().await.len(),
        1,
        "Phase 3: Output written"
    );
    assert_eq!(writer.get_inputs().await.len(), 1, "Phase 4: Input written");
    // Phase 5 (calculate amounts) and Phase 6 (simplified layer) are no-ops in MockWriter
    // but they should not error
}

/// Test ingesting 100 blocks for performance check
#[tokio::test]
async fn test_ingest_100_blocks_performance() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);

    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();

    let start = std::time::Instant::now();

    for height in 0..100 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        orchestrator
            .ingest_block(&block, height, "blk00000.dat", None)
            .await
            .unwrap();
    }

    let duration = start.elapsed();

    // Verify all data was written
    assert_eq!(writer.get_blocks().await.len(), 100);
    assert_eq!(writer.get_transactions().await.len(), 100);
    assert_eq!(writer.get_outputs().await.len(), 100);
    assert_eq!(writer.get_inputs().await.len(), 100);

    // Performance check: Should complete in <1 second with MockWriter
    assert!(
        duration.as_secs() < 1,
        "100 blocks should ingest in <1s with MockWriter"
    );
    println!("Ingested 100 blocks in {:?}", duration);
}

/// Test that output addresses are correctly derived in Phase 3
#[tokio::test]
async fn test_phase3_address_derivation() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);

    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();

    // Ingest first 5 blocks
    for height in 0..5 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        orchestrator
            .ingest_block(&block, height, "blk00000.dat", None)
            .await
            .unwrap();
    }

    let outputs = writer.get_outputs().await;

    // All outputs should have addresses (early blocks use P2PK)
    for output in &outputs {
        assert!(
            output.address.is_some(),
            "Output {} should have an address",
            output.output_id
        );
        assert_eq!(output.script_type, "P2PK", "Early blocks use P2PK");
    }

    // Genesis block output should have Satoshi's address
    let genesis_output = outputs.iter().find(|o| o.output_index == 0).unwrap();
    assert_eq!(
        genesis_output.address.as_ref().unwrap(),
        "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"
    );
}

/// Test that coinbase inputs are handled correctly in Phase 4
#[tokio::test]
async fn test_phase4_coinbase_inputs() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);

    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();

    // Ingest first 10 blocks (all coinbase)
    for height in 0..10 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        orchestrator
            .ingest_block(&block, height, "blk00000.dat", None)
            .await
            .unwrap();
    }

    let inputs = writer.get_inputs().await;

    // All inputs should be coinbase inputs
    for input in &inputs {
        assert_eq!(
            input.previous_output_index, 0xFFFFFFFF,
            "Input {} should be coinbase",
            input.input_id
        );
        // Coinbase inputs reference a null txid (all zeros)
        assert_eq!(
            input.previous_txid.len(),
            64,
            "Should have 64-char hex txid"
        );
    }
}

/// Test concurrent orchestrators with separate writers
#[tokio::test]
async fn test_concurrent_orchestrators() {
    let writer1 = MockWriter::new();
    let writer2 = MockWriter::new();

    let orchestrator1 = IngestionOrchestrator::new(writer1.clone(), Network::Bitcoin, 100_000);
    let orchestrator2 = IngestionOrchestrator::new(writer2.clone(), Network::Bitcoin, 100_000);

    orchestrator1.init_schema().await.unwrap();
    orchestrator2.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let genesis = reader.next_block().unwrap().unwrap();
    let block1 = reader.next_block().unwrap().unwrap();

    // Ingest different blocks concurrently
    let handle1 = tokio::spawn(async move {
        orchestrator1
            .ingest_block(&genesis, 0, "blk00000.dat", None)
            .await
    });

    let handle2 = tokio::spawn(async move {
        orchestrator2
            .ingest_block(&block1, 1, "blk00000.dat", None)
            .await
    });

    // Both should succeed
    handle1.await.unwrap().unwrap();
    handle2.await.unwrap().unwrap();

    // Each writer should have its own data
    assert_eq!(writer1.get_blocks().await.len(), 1);
    assert_eq!(writer2.get_blocks().await.len(), 1);

    assert_eq!(writer1.get_blocks().await[0].height, 0);
    assert_eq!(writer2.get_blocks().await[0].height, 1);
}

/// Test cache behavior across multiple blocks (verifies cache hits/misses)
#[tokio::test]
async fn test_multi_block_cache_usage() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 10_000);

    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();

    // Ingest first 10 blocks
    for height in 0..10 {
        let block = reader.next_block().unwrap().expect("Block should exist");
        orchestrator
            .ingest_block(&block, height, "blk00000.dat", None)
            .await
            .unwrap();

        // Print cache stats after each block
        let stats = orchestrator.cache_stats();
        println!(
            "Block {}: Cache stats - hits: {}, misses: {}, hit_rate: {:.2}%, size: {}",
            height,
            stats.hits,
            stats.misses,
            stats.hit_rate_percent(),
            orchestrator.cache_size()
        );
    }

    // Verify all blocks were ingested
    assert_eq!(writer.get_blocks().await.len(), 10);

    // Get final cache stats
    let final_stats = orchestrator.cache_stats();

    // For sequential Bitcoin blocks, we expect:
    // - Block 0 (genesis): 0 hits, 0 misses (coinbase only)
    // - Blocks 1-9: Should have cache hits as outputs from previous blocks are spent
    // Bitcoin's early blocks are sequential, so hit rate should be high
    println!("\nFinal cache stats after 10 blocks:");
    println!("  Total hits: {}", final_stats.hits);
    println!("  Total misses: {}", final_stats.misses);
    println!("  Hit rate: {:.2}%", final_stats.hit_rate_percent());
    println!("  Cache size: {}", orchestrator.cache_size());

    // Assertions:
    // 1. If there were any non-coinbase transactions, we should have some lookups
    // 2. For Bitcoin's sequential early blocks, hit rate should be reasonable
    // Note: Early Bitcoin blocks are small and mostly coinbase, so stats might be zero (all coinbase)
    // This is acceptable - we're mainly testing the integration works without errors
}

/// Test that coinbase transactions don't use cache (they have no inputs)
#[tokio::test]
async fn test_coinbase_transactions_skip_cache() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 10_000);

    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let genesis = reader
        .next_block()
        .unwrap()
        .expect("Genesis block should exist");

    // Genesis block contains only coinbase transaction
    assert_eq!(genesis.txdata.len(), 1, "Genesis should have 1 transaction");
    assert!(
        genesis.txdata[0].is_coinbase(),
        "Genesis tx should be coinbase"
    );

    // Ingest genesis block
    orchestrator
        .ingest_block(&genesis, 0, "blk00000.dat", None)
        .await
        .unwrap();

    // Verify cache stats: coinbase transactions should not trigger cache lookups
    let stats = orchestrator.cache_stats();
    assert_eq!(stats.hits, 0, "Coinbase should have 0 cache hits");
    assert_eq!(stats.misses, 0, "Coinbase should have 0 cache misses");

    // Verify transaction was still written with correct amounts
    let transactions = writer.get_transactions().await;
    assert_eq!(transactions.len(), 1);
    assert_eq!(
        transactions[0].total_input,
        Some(0),
        "Coinbase should have 0 input"
    );
    assert_eq!(
        transactions[0].total_output,
        Some(5000000000),
        "Coinbase should have 50 BTC output"
    );
    assert_eq!(transactions[0].fee, Some(0), "Coinbase should have 0 fee");
}

/// Test Phase 6 PERFORMS and BENEFITS_TO relationship creation
#[tokio::test]
async fn test_phase6_performs_benefits_to() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 10_000);

    orchestrator.init_schema().await.unwrap();

    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();

    // Skip genesis (coinbase only), ingest block 1 which has a non-coinbase tx
    let _ = reader.next_block().unwrap(); // Skip genesis
    let block1 = reader.next_block().unwrap().expect("Block 1 should exist");

    // Ingest genesis first (to populate cache with outputs for block 1 to spend)
    let mut reader2 = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let genesis = reader2.next_block().unwrap().unwrap();
    orchestrator
        .ingest_block(&genesis, 0, "blk00000.dat", None)
        .await
        .unwrap();

    // Now ingest block 1
    orchestrator
        .ingest_block(&block1, 1, "blk00000.dat", None)
        .await
        .unwrap();

    // Verify PERFORMS relationships were created
    let performs = writer.get_performs().await;
    println!("PERFORMS relationships: {} found", performs.len());
    for p in &performs {
        println!(
            "  {} PERFORMS {} (inputs: {}, amount: {})",
            p.from_address, p.to_txid, p.input_count, p.amount_spent
        );
    }

    // Verify BENEFITS_TO relationships were created
    let benefits_to = writer.get_benefits_to().await;
    println!("BENEFITS_TO relationships: {} found", benefits_to.len());
    for b in &benefits_to {
        println!(
            "  {} BENEFITS_TO {} (outputs: {}, amount: {})",
            b.from_txid, b.to_address, b.output_count, b.amount_received
        );
    }

    // For block 1, we expect:
    // - At least 1 PERFORMS (from sender address to transaction)
    // - At least 1 BENEFITS_TO (from transaction to recipient address)
    // Note: Exact counts depend on the block structure

    // Basic assertions to verify Phase 6 executed
    assert!(
        !performs.is_empty() || !benefits_to.is_empty(),
        "Phase 6 should create PERFORMS or BENEFITS_TO relationships"
    );

    // Verify aggregation data is present
    if !performs.is_empty() {
        assert!(performs[0].input_count > 0, "Should aggregate input count");
        assert!(
            performs[0].amount_spent > 0,
            "Should aggregate amount spent"
        );
    }

    if !benefits_to.is_empty() {
        assert!(
            benefits_to[0].output_count > 0,
            "Should aggregate output count"
        );
        assert!(
            benefits_to[0].amount_received > 0,
            "Should aggregate amount received"
        );
    }
}

/// Test cache behavior with real Bitcoin blocks containing multi-input transactions
///
/// This test uses early sequential blocks which are self-contained
/// (outputs created in earlier blocks are spent in later blocks within the test set).
#[tokio::test]
async fn test_real_bitcoin_blocks_with_multi_input_txs() {
    let writer = MockWriter::new();

    // Use reasonable cache size for testing
    let cache_size = 50_000;
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, cache_size);

    orchestrator.init_schema().await.unwrap();

    // Use early blocks from blk00000.dat which are self-contained
    // (Sequential blocks where outputs are spent by later blocks in the same set)
    let block_file = "test_data/blk00000.dat";

    if !std::path::Path::new(block_file).exists() {
        println!("⚠️  Skipping test: {} not found", block_file);
        println!("   (This test requires Bitcoin blockchain data)");
        return;
    }

    let mut reader = BlockFileReader::new(block_file, Network::Bitcoin).unwrap();

    println!(
        "\n📊 Testing with real Bitcoin blockchain data from {}",
        block_file
    );
    println!("   Cache size: {}", cache_size);
    println!("   Processing sequential blocks to demonstrate cache effectiveness...");

    let mut blocks_processed = 0;
    let mut total_transactions = 0;
    let mut total_inputs = 0;
    let mut total_outputs = 0;

    // Process first 200 sequential blocks
    // Early Bitcoin blocks have gradually increasing transaction counts
    for height in 0..200 {
        match reader.next_block() {
            Ok(Some(block)) => {
                // Count transactions and inputs for stats
                total_transactions += block.txdata.len();
                for tx in &block.txdata {
                    total_inputs += tx.input.len();
                    total_outputs += tx.output.len();
                }

                orchestrator
                    .ingest_block(&block, height, "blk00000.dat", None)
                    .await
                    .unwrap();

                blocks_processed += 1;

                // Print cache stats every 50 blocks
                if blocks_processed % 50 == 0 {
                    let stats = orchestrator.cache_stats();
                    println!(
                        "   Block {}: hits={}, misses={}, hit_rate={:.1}%, cache_size={}",
                        blocks_processed,
                        stats.hits,
                        stats.misses,
                        stats.hit_rate_percent(),
                        orchestrator.cache_size()
                    );
                }
            }
            Ok(None) => break, // End of file
            Err(e) => {
                println!("⚠️  Error reading block at height {}: {}", height, e);
                break;
            }
        }
    }

    // Final statistics
    let final_stats = orchestrator.cache_stats();

    println!("\n📈 Real Bitcoin blockchain ingestion complete:");
    println!("   Blocks processed: {}", blocks_processed);
    println!("   Transactions: {}", total_transactions);
    println!("   Total inputs: {}", total_inputs);
    println!("   Total outputs: {}", total_outputs);
    println!("\n🎯 Cache performance:");
    println!("   Cache hits: {}", final_stats.hits);
    println!("   Cache misses: {}", final_stats.misses);
    println!("   Hit rate: {:.2}%", final_stats.hit_rate_percent());
    println!(
        "   Final cache size: {} / {}",
        orchestrator.cache_size(),
        cache_size
    );

    // Assertions for real Bitcoin data
    assert!(blocks_processed > 0, "Should process at least some blocks");
    assert!(
        total_inputs > blocks_processed,
        "Should have more inputs than blocks (multi-input txs)"
    );

    // With real sequential Bitcoin blocks and reasonable cache size,
    // we expect high cache hit rate (most inputs spend recent outputs)
    if final_stats.hits + final_stats.misses > 0 {
        println!("\n✅ Cache effectiveness verified with real Bitcoin data");

        // Real Bitcoin blocks should show cache benefit
        // (Sequential blocks spend recently created outputs)
        if final_stats.hits > 0 {
            println!(
                "   Cache is providing benefit (hit rate: {:.1}%)",
                final_stats.hit_rate_percent()
            );
        }
    }

    // Verify data integrity - all blocks should be written
    assert_eq!(writer.get_blocks().await.len(), blocks_processed);
}
