//! Multi-file ingestion test
//!
//! This test processes ALL block files in the test_data directory,
//! demonstrating production-ready multi-file ingestion with checkpoint resume.
//!
//! Run with: cargo test --test neo4j_multi_file_test -- --ignored --nocapture

use bitcoin::Network;
use bitcoin_chain_graph::config::ConfigLoader;
use bitcoin_chain_graph::domain::IngestionOrchestrator;
use bitcoin_chain_graph::parser::BlockFileReader;
use bitcoin_chain_graph::writer::Neo4jWriter;
use std::path::Path;

/// Load all available block files with checkpoint resume
#[tokio::test]
#[ignore]
async fn test_ingest_all_block_files() {
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

    println!("Checking resume height from checkpoint...");
    let mut current_height = orchestrator.get_resume_height().await.expect("Failed to get resume height");
    println!("  Resume from block height: {}", current_height);

    // Find all block files
    let mut block_files: Vec<String> = std::fs::read_dir(&config.bitcoin.blocks_dir)
        .expect("Failed to read blocks directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let filename = entry.file_name().to_str()?.to_string();
            if filename.starts_with("blk") && filename.ends_with(".dat") {
                Some(filename)
            } else {
                None
            }
        })
        .collect();

    // Sort files numerically (blk00000.dat, blk00001.dat, etc.)
    block_files.sort();

    println!("\nFound {} block files in {}", block_files.len(), config.bitcoin.blocks_dir);

    let mut total_blocks_ingested = 0;
    let mut files_processed = 0;

    // Process each file
    for filename in block_files.iter() {
        let block_path = format!("{}/{}", config.bitcoin.blocks_dir, filename);

        println!("\n[File {}/{}] Processing: {}", files_processed + 1, block_files.len(), filename);

        let mut reader = match BlockFileReader::new(&block_path, Network::Bitcoin) {
            Ok(r) => r,
            Err(e) => {
                println!("  ⚠ Skipping {} - cannot open: {}", filename, e);
                continue;
            }
        };

        // Read all blocks from this file
        let mut blocks_in_file = Vec::new();
        while let Some(block) = reader.next_block().expect("Failed to read block") {
            blocks_in_file.push(block);
        }

        println!("  File contains {} blocks", blocks_in_file.len());

        if blocks_in_file.is_empty() {
            println!("  ⚠ Skipping empty file");
            continue;
        }

        // Ingest blocks
        let mut blocks_ingested_from_file = 0;
        for block in blocks_in_file.iter() {
            let block_height = current_height;

            orchestrator.ingest_block(block, block_height, filename.as_str(), None)
                .await
                .expect(&format!("Failed to ingest block {}", block_height));

            blocks_ingested_from_file += 1;
            total_blocks_ingested += 1;
            current_height += 1;

            // Progress report every 100 blocks
            if blocks_ingested_from_file % 100 == 0 {
                println!("    Ingested {} blocks from this file (height {})", blocks_ingested_from_file, block_height);
            }
        }

        println!("  ✓ Completed {} - ingested {} blocks (heights {} to {})",
                 filename,
                 blocks_ingested_from_file,
                 current_height - blocks_ingested_from_file,
                 current_height - 1);
        files_processed += 1;
    }

    println!("\n✅ Ingestion complete!");
    println!("  Files processed: {}", files_processed);
    println!("  Total blocks ingested: {}", total_blocks_ingested);
    println!("  Final block height: {}", current_height - 1);

    println!("\nQuery the checkpoint:");
    println!("  MATCH (c:IngestionCheckpoint) RETURN c");
    println!("\nQuery blocks:");
    println!("  MATCH (b:Block) RETURN b ORDER BY b.height DESC LIMIT 10");
}

/// Test ingesting just the first 4 block files to verify filename tracking
#[tokio::test]
#[ignore]
async fn test_ingest_first_4_files() {
    println!("Loading configuration...");
    let config = ConfigLoader::from_file("config/default.toml")
        .expect("Failed to load config");

    println!("Connecting to Neo4j...");
    let writer = Neo4jWriter::new(config.neo4j.clone()).await.expect("Failed to connect");
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin);

    println!("Initializing schema and checkpoint...");
    orchestrator.init_schema().await.expect("Failed to initialize schema");

    let resume_height = orchestrator.get_resume_height().await.expect("Failed to get resume height");
    println!("  Resume from block height: {}", resume_height);

    let mut current_height = resume_height;
    let files_to_process = vec!["blk00000.dat", "blk00001.dat", "blk00002.dat", "blk00003.dat"];

    for (file_idx, filename) in files_to_process.iter().enumerate() {
        let block_path = format!("{}/{}", config.bitcoin.blocks_dir, filename);

        println!("\n[File {}/{}] Processing: {}", file_idx + 1, files_to_process.len(), filename);

        let mut reader = BlockFileReader::new(&block_path, Network::Bitcoin)
            .expect(&format!("Failed to open {}", filename));

        let mut blocks_in_file = Vec::new();
        while let Some(block) = reader.next_block().expect("Failed to read block") {
            blocks_in_file.push(block);
        }

        println!("  File contains {} blocks", blocks_in_file.len());

        for block in blocks_in_file.iter() {
            orchestrator.ingest_block(block, current_height, filename, None)
                .await
                .expect(&format!("Failed to ingest block {}", current_height));
            current_height += 1;
        }

        println!("  ✓ Completed {} (heights {} to {})", filename, current_height - blocks_in_file.len() as u32, current_height - 1);

        // Check checkpoint after each file
        let checkpoint_height = orchestrator.get_resume_height().await.expect("Failed to get checkpoint");
        println!("  📍 Checkpoint now at height: {} (ready to resume from here)", checkpoint_height);
    }

    println!("\n✅ Test complete!");
    println!("  Total blocks ingested: {}", current_height - resume_height);
    println!("  Final height: {}", current_height - 1);
    println!("\nQuery checkpoint to verify filename updated:");
    println!("  MATCH (c:IngestionCheckpoint) RETURN c.lastProcessedFile, c.lastProcessedHeight");
}

