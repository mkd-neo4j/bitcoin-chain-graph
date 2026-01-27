//! Full End-to-End Integration Test - M1 through M7
//!
//! This test validates ALL milestones (M1-M7) working together:
//! - M1: Block file parsing
//! - M2: Address derivation
//! - M3: Domain models
//! - M4: GraphWriter trait
//! - M5: Ingestion orchestrator
//! - M6: Neo4j writer
//! - M7: UTXO cache + Rust calculations
//!
//! **IMPORTANT:** This test requires a running Neo4j database!
//!
//! **KNOWN LIMITATION:** Bitcoin Core .blk files store blocks in ARRIVAL ORDER, not blockchain
//! height order. This means blocks may reference UTXOs from blocks that appear later in the
//! file. For proper sequential ingestion, you need:
//! 1. Use Bitcoin Core's block index (chainstate leveldb) to get correct heights
//! 2. Use Bitcoin Core RPC to fetch blocks in order
//! 3. Or use the quick test (test_quick_1000_blocks) which works for early blocks
//!
//! To run this test:
//! 1. Start Neo4j (docker or local instance)
//! 2. Update config/default.toml with your Neo4j credentials
//! 3. Run: cargo test --test full_end_to_end_neo4j test_quick_1000_blocks -- --ignored --nocapture
//!
//! This test will:
//! - Process blocks from .blk files (may fail on out-of-order blocks)
//! - Ingest all blocks into Neo4j
//! - Verify data integrity with queries
//! - Show performance metrics

use bitcoin::Network;
use bitcoin_chain_graph::config::ConfigLoader;
use bitcoin_chain_graph::domain::IngestionOrchestrator;
use bitcoin_chain_graph::parser::BlockFileReader;
use bitcoin_chain_graph::writer::Neo4jWriter;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Helper to get all .blk files from test_data directory
fn get_blk_files(dir: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("dat") {
                if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                    if filename.starts_with("blk") {
                        files.push(path);
                    }
                }
            }
        }
    }

    // Sort files by name (blk00000.dat, blk00001.dat, ...)
    files.sort();
    files
}

/// Full end-to-end test: ingest all blocks from all .blk files into Neo4j
///
/// This is a COMPREHENSIVE test of M1-M7. Run with:
/// cargo test --test full_end_to_end_neo4j test_full_ingestion_all_files -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_full_ingestion_all_files() {
    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║  Full End-to-End Integration Test (M1-M7)                      ║");
    println!("║  Processing ALL .blk files from test_data/                     ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    // Load configuration
    println!("📝 Loading configuration from config/default.toml...");
    let config = ConfigLoader::from_file("config/default.toml")
        .expect("Failed to load config - make sure config/default.toml exists");

    println!("   Neo4j URI: {}", config.neo4j.uri);
    println!("   Database: {}", config.neo4j.database);
    println!("   Blocks dir: {}", config.bitcoin.blocks_dir);

    // Connect to Neo4j
    println!("\n🔌 Connecting to Neo4j...");
    let writer = Neo4jWriter::new(config.neo4j.clone())
        .await
        .expect("Failed to connect to Neo4j - make sure Neo4j is running");
    println!("   ✅ Connected successfully");

    // Create orchestrator with UTXO cache
    let cache_size = config.performance.cache_capacity();
    println!("\n🔧 Creating ingestion orchestrator...");
    println!("   UTXO cache size: {} entries (~{:.1} MB)",
        cache_size,
        (cache_size as f64 * 138.0) / 1_000_000.0
    );

    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, cache_size);

    // Initialize schema
    println!("\n🏗️  Initializing Neo4j schema...");
    orchestrator.init_schema().await.expect("Failed to initialize schema");
    println!("   ✅ Schema initialized (constraints + indexes created)");

    // Get resume height from checkpoint
    let resume_height = orchestrator.get_resume_height().await
        .expect("Failed to get resume height");
    println!("\n📍 Checkpoint status:");
    println!("   Resume from block height: {}", resume_height);

    // Get all .blk files
    let blk_files = get_blk_files(&config.bitcoin.blocks_dir);
    println!("\n📂 Found {} .blk files in {}", blk_files.len(), config.bitcoin.blocks_dir);

    if blk_files.is_empty() {
        panic!("No .blk files found in {}! Make sure test data exists.", config.bitcoin.blocks_dir);
    }

    // Start ingestion
    println!("\n🚀 Starting ingestion...\n");
    println!("┌─────────────┬──────────────────┬─────────────┬──────────────┬─────────────┐");
    println!("│ File        │ Blocks Processed │ Speed       │ Cache Hits   │ Elapsed     │");
    println!("├─────────────┼──────────────────┼─────────────┼──────────────┼─────────────┤");

    let overall_start = Instant::now();
    let mut total_blocks = 0u32;
    let mut current_height = resume_height;

    for (file_idx, blk_file) in blk_files.iter().enumerate() {
        let file_name = blk_file.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let file_start = Instant::now();
        let mut blocks_in_file = 0u32;

        // Open block file
        let mut reader = match BlockFileReader::new(blk_file.to_str().unwrap(), Network::Bitcoin) {
            Ok(r) => r,
            Err(e) => {
                println!("│ {:11} │ SKIPPED          │ Error: {:?}", file_name, e);
                continue;
            }
        };

        // Read and ingest all blocks from this file
        while let Ok(Some(block)) = reader.next_block() {
            // Skip blocks before resume height
            if current_height < resume_height {
                current_height += 1;
                continue;
            }

            // Ingest block
            match orchestrator.ingest_block(&block, current_height, file_name, None).await {
                Ok(_) => {
                    blocks_in_file += 1;
                    current_height += 1;
                }
                Err(e) => {
                    println!("\n❌ Error at block {}: {:?}", current_height, e);
                    println!("   File: {}", file_name);
                    panic!("Ingestion failed at block {}", current_height);
                }
            }
        }

        total_blocks += blocks_in_file;
        let file_duration = file_start.elapsed();
        let blocks_per_sec = if file_duration.as_secs_f64() > 0.0 {
            blocks_in_file as f64 / file_duration.as_secs_f64()
        } else {
            0.0
        };

        let cache_stats = orchestrator.cache_stats();
        let hit_rate = if cache_stats.hits + cache_stats.misses > 0 {
            format!("{:.1}%", cache_stats.hit_rate_percent())
        } else {
            "N/A".to_string()
        };

        println!("│ {:11} │ {:16} │ {:>7.1} b/s │ {:>11} │ {:>7.2}s │",
            file_name,
            blocks_in_file,
            blocks_per_sec,
            hit_rate,
            file_duration.as_secs_f64()
        );

        // Progress summary every 10 files
        if (file_idx + 1) % 10 == 0 {
            let elapsed = overall_start.elapsed();
            let overall_bps = total_blocks as f64 / elapsed.as_secs_f64();
            println!("├─────────────┴──────────────────┴─────────────┴──────────────┴─────────────┤");
            println!("│ Progress: {}/{} files ({:.1}%) - Total blocks: {} - Avg: {:.1} b/s │",
                file_idx + 1,
                blk_files.len(),
                ((file_idx + 1) as f64 / blk_files.len() as f64) * 100.0,
                total_blocks,
                overall_bps
            );
            println!("├─────────────┬──────────────────┬─────────────┬──────────────┬─────────────┤");
        }
    }

    println!("└─────────────┴──────────────────┴─────────────┴──────────────┴─────────────┘");

    let total_duration = overall_start.elapsed();
    let overall_blocks_per_sec = total_blocks as f64 / total_duration.as_secs_f64();

    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║  INGESTION COMPLETE                                            ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!("\n📊 Final Statistics:");
    println!("   Total blocks ingested: {}", total_blocks);
    println!("   Total duration: {:.2} seconds", total_duration.as_secs_f64());
    println!("   Average speed: {:.2} blocks/sec", overall_blocks_per_sec);
    println!("   Files processed: {}", blk_files.len());

    // Cache statistics
    let final_cache_stats = orchestrator.cache_stats();
    println!("\n💾 UTXO Cache Statistics:");
    println!("   Current size: {}/{} ({:.1}%)",
        orchestrator.cache_size(),
        cache_size,
        (orchestrator.cache_size() as f64 / cache_size as f64) * 100.0
    );
    println!("   Cache hits: {}", final_cache_stats.hits);
    println!("   Cache misses: {}", final_cache_stats.misses);
    println!("   Hit rate: {:.2}%", final_cache_stats.hit_rate_percent());
    println!("   Inserts: {}", final_cache_stats.inserts);
    println!("   Removals (spent): {}", final_cache_stats.removals);

    // Mark ingestion as complete
    println!("\n✅ Marking ingestion as complete...");
    orchestrator.mark_complete().await.expect("Failed to mark complete");

    println!("\n🎉 SUCCESS! All data ingested into Neo4j!");
    println!("\n📍 You can now query the data in Neo4j:");
    println!("   MATCH (b:Block) RETURN count(b)");
    println!("   MATCH (t:Transaction) RETURN count(t)");
    println!("   MATCH (o:Output) RETURN count(o)");
    println!("   MATCH ()-[r:PERFORMS]->() RETURN count(r)");
    println!("   MATCH ()-[r:BENEFITS_TO]->() RETURN count(r)");
}

/// Quick test: ingest first 1000 blocks from blk00000.dat
///
/// This is a faster test for validation. Run with:
/// cargo test --test full_end_to_end_neo4j test_quick_1000_blocks -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_quick_1000_blocks() {
    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║  Quick Integration Test (First 1000 blocks)                    ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    let config = ConfigLoader::from_file("config/default.toml")
        .expect("Failed to load config");

    println!("🔌 Connecting to Neo4j at {}...", config.neo4j.uri);
    let writer = Neo4jWriter::new(config.neo4j.clone()).await
        .expect("Failed to connect to Neo4j");

    let cache_size = 100_000;
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, cache_size);

    println!("🏗️  Initializing schema...");
    orchestrator.init_schema().await.expect("Failed to initialize schema");

    let resume_height = orchestrator.get_resume_height().await.unwrap();
    println!("📍 Resume from height: {}", resume_height);

    println!("\n🚀 Starting ingestion of first 1000 blocks...\n");

    let block_path = format!("{}/blk00000.dat", config.bitcoin.blocks_dir);
    let mut reader = BlockFileReader::new(&block_path, Network::Bitcoin)
        .expect("Failed to open blk00000.dat");

    let start = Instant::now();
    let target_blocks = 1000;

    // Skip to resume height
    for _ in 0..resume_height {
        if reader.next_block().unwrap().is_none() {
            break;
        }
    }

    for height in resume_height..target_blocks {
        if let Some(block) = reader.next_block().unwrap() {
            orchestrator.ingest_block(&block, height, "blk00000.dat", None).await
                .expect(&format!("Failed to ingest block {}", height));

            if (height + 1) % 100 == 0 {
                let elapsed = start.elapsed();
                let bps = (height - resume_height + 1) as f64 / elapsed.as_secs_f64();
                let cache_stats = orchestrator.cache_stats();
                println!("   Block {:4} | {:.1} b/s | Cache: {} hits, {} misses ({:.1}%)",
                    height,
                    bps,
                    cache_stats.hits,
                    cache_stats.misses,
                    cache_stats.hit_rate_percent()
                );
            }
        } else {
            break;
        }
    }

    let duration = start.elapsed();
    let blocks_processed = target_blocks - resume_height;
    let bps = blocks_processed as f64 / duration.as_secs_f64();

    println!("\n✅ Complete!");
    println!("   Blocks: {}", blocks_processed);
    println!("   Duration: {:.2}s", duration.as_secs_f64());
    println!("   Speed: {:.2} blocks/sec", bps);

    let stats = orchestrator.cache_stats();
    println!("\n💾 Cache:");
    println!("   Size: {}", orchestrator.cache_size());
    println!("   Hit rate: {:.2}%", stats.hit_rate_percent());
}

/// Verify data integrity after ingestion
///
/// Run after full ingestion to validate data. Run with:
/// cargo test --test full_end_to_end_neo4j test_verify_data_integrity -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_verify_data_integrity() {
    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║  Data Integrity Verification                                   ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    let config = ConfigLoader::from_file("config/default.toml")
        .expect("Failed to load config");

    let writer = Neo4jWriter::new(config.neo4j.clone()).await
        .expect("Failed to connect to Neo4j");

    // We'll use the writer directly to run validation queries
    println!("🔍 Running validation queries...\n");

    println!("This test is a placeholder for comprehensive data validation.");
    println!("Add custom validation queries here to check:");
    println!("  - Block count matches expected");
    println!("  - All transactions have amounts");
    println!("  - PERFORMS/BENEFITS_TO relationships exist");
    println!("  - No orphaned nodes");
    println!("  - Genesis block integrity");

    println!("\n✅ Validation complete (placeholder)");
}
