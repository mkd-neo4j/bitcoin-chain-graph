//! Live Neo4j integration test
//!
//! This test actually loads Bitcoin data into a real Neo4j database
//! so you can verify the ingestion pipeline works end-to-end.
//!
//! Run with: cargo test --test neo4j_live_test -- --ignored --nocapture

use bitcoin::Network;
use bitcoin_chain_graph::config::ConfigLoader;
use bitcoin_chain_graph::domain::IngestionOrchestrator;
use bitcoin_chain_graph::parser::BlockFileReader;
use bitcoin_chain_graph::writer::Neo4jWriter;
use std::path::Path;

/// Live test that loads the first 10 Bitcoin blocks into Neo4j
///
/// This is ignored by default. Run with:
/// cargo test --test neo4j_live_test -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_load_blocks_into_neo4j() {
    println!("Loading configuration...");
    let config = ConfigLoader::from_file("config/default.toml")
        .expect("Failed to load config - make sure config/default.toml exists");

    println!("Connecting to Neo4j at {}...", config.neo4j.uri);
    let writer = Neo4jWriter::new(config.neo4j.clone())
        .await
        .expect("Failed to connect to Neo4j");

    println!("Creating ingestion orchestrator...");
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin);

    println!("Initializing Neo4j schema...");
    orchestrator.init_schema().await.expect("Failed to initialize schema");

    println!("Opening block file: {}/blk00000.dat", config.bitcoin.blocks_dir);
    let block_path = format!("{}/blk00000.dat", config.bitcoin.blocks_dir);
    let file_name = Path::new(&block_path)
        .file_name()
        .and_then(|n| n.to_str())
        .expect("Invalid file path");

    let mut reader = BlockFileReader::new(&block_path, Network::Bitcoin)
        .expect("Failed to open block file");

    let num_blocks = 10;
    println!("\nIngesting first {} blocks from {}...", num_blocks, file_name);

    for height in 0..num_blocks {
        let block = reader.next_block()
            .expect("Failed to read block")
            .expect(&format!("Block {} should exist", height));

        let block_hash = block.block_hash().to_string();

        println!("  Block {}: {}", height, block_hash);

        orchestrator.ingest_block(&block, height, file_name, None)
            .await
            .expect(&format!("Failed to ingest block {}", height));
    }

    println!("\n✅ Successfully loaded {} blocks into Neo4j!", num_blocks);
    println!("\nYou can now query Neo4j to see the data:");
    println!("  MATCH (b:Block) RETURN b ORDER BY b.height LIMIT 10");
    println!("  MATCH (t:Transaction) RETURN t LIMIT 10");
    println!("  MATCH (o:Output) RETURN o LIMIT 10");
}

/// Load just the genesis block for quick verification
#[tokio::test]
#[ignore]
async fn test_load_genesis_block() {
    println!("Loading configuration...");
    let config = ConfigLoader::from_file("config/default.toml")
        .expect("Failed to load config");

    println!("Connecting to Neo4j...");
    let writer = Neo4jWriter::new(config.neo4j.clone()).await.expect("Failed to connect");
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin);

    println!("Initializing schema...");
    orchestrator.init_schema().await.expect("Failed to initialize schema");

    println!("Reading genesis block...");
    let block_path = format!("{}/blk00000.dat", config.bitcoin.blocks_dir);
    let file_name = Path::new(&block_path)
        .file_name()
        .and_then(|n| n.to_str())
        .expect("Invalid file path");

    let mut reader = BlockFileReader::new(&block_path, Network::Bitcoin)
        .expect("Failed to open block file");

    let genesis = reader.next_block().unwrap().expect("Genesis block should exist");

    println!("Ingesting genesis block from {}...", file_name);
    orchestrator.ingest_block(&genesis, 0, file_name, None).await.expect("Failed to ingest");

    println!("\n✅ Genesis block loaded!");
    println!("\nQuery to view it:");
    println!("  MATCH (b:Block {{height: 0}}) RETURN b");
}

/// Test checkpoint resume functionality - load blocks, then resume from checkpoint
#[tokio::test]
#[ignore]
async fn test_checkpoint_resume() {
    println!("Loading configuration...");
    let config = ConfigLoader::from_file("config/default.toml")
        .expect("Failed to load config");

    println!("Connecting to Neo4j...");
    let writer = Neo4jWriter::new(config.neo4j.clone()).await.expect("Failed to connect");
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin);

    println!("Initializing schema and checkpoint...");
    orchestrator.init_schema().await.expect("Failed to initialize schema");

    println!("Checking resume height from checkpoint...");
    let resume_height = orchestrator.get_resume_height().await.expect("Failed to get resume height");
    println!("  Resume from block: {}", resume_height);

    println!("\nReading blocks from file...");
    let block_path = format!("{}/blk00000.dat", config.bitcoin.blocks_dir);
    let file_name = Path::new(&block_path)
        .file_name()
        .and_then(|n| n.to_str())
        .expect("Invalid file path");

    let mut reader = BlockFileReader::new(&block_path, Network::Bitcoin)
        .expect("Failed to open block file");

    // Read all blocks we need (up to block 9)
    let mut all_blocks = Vec::new();
    while let Some(block) = reader.next_block().expect("Failed to read block") {
        all_blocks.push(block);
        if all_blocks.len() >= 10 {
            break;
        }
    }

    // Ingest ONLY from resume_height onwards (skip already-processed blocks)
    let target_height = 10;
    println!("\nIngesting blocks {} to {} from {}...", resume_height, target_height - 1, file_name);

    for height in resume_height..target_height {
        let block = &all_blocks[height as usize];
        println!("  Block {}: {}", height, block.block_hash());
        orchestrator.ingest_block(block, height, file_name, None)
            .await
            .expect(&format!("Failed to ingest block {}", height));
    }

    println!("\nChecking final checkpoint...");
    let final_height = orchestrator.get_resume_height().await.expect("Failed to get resume height");
    println!("  Final resume height: {}", final_height);
    assert_eq!(final_height, target_height, "Checkpoint should be at block {}", target_height);

    let blocks_ingested = target_height - resume_height;
    println!("\n✅ Checkpoint resume works!");
    println!("  Started from checkpoint at block {}", resume_height);
    println!("  Ingested {} new blocks ({} to {})", blocks_ingested, resume_height, target_height - 1);
    println!("  Ready to resume from block {}", final_height);
    println!("\nQuery checkpoint in Neo4j:");
    println!("  MATCH (c:IngestionCheckpoint) RETURN c");
}
