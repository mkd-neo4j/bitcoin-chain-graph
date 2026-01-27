//! Integration test: Ingest first 4 .blk files with batch processing
//!
//! Tests the complete ingestion pipeline with batch loading and batch database writes.
//! Loads blk00000.dat through blk00003.dat entirely into memory, sorts by blockchain height,
//! and processes in batches of 1000 blocks for maximum performance.
//!
//! **Performance Expectations:**
//! - Batch mode should achieve 500-2,000 blocks/sec (vs 19 blocks/sec sequential)
//! - First 4 files should complete in under 1 minute
//!
//! **To Run:**
//! 1. Clear database: `cargo test --test clear_neo4j -- --ignored --nocapture`
//! 2. Run test: `cargo test --test ingest_first_4_files -- --ignored --nocapture`

use bitcoin::Network;
use bitcoin_chain_graph::config::ConfigLoader;
use bitcoin_chain_graph::domain::IngestionOrchestrator;
use bitcoin_chain_graph::parser::BatchedBlockLoader;
use bitcoin_chain_graph::writer::Neo4jWriter;
use std::time::Instant;

#[tokio::test]
#[ignore]
async fn test_ingest_first_4_files() {
    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║  Batch Ingestion Test - First 4 Block Files                   ║");
    println!("║  Processing: blk00000.dat - blk00003.dat                      ║");
    println!("║  Mode: Batch loading + Batch DB writes                         ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    // Load configuration
    println!("📝 Loading configuration...");
    let config = ConfigLoader::from_file("config/default.toml")
        .expect("Failed to load config");
    println!("   Neo4j: {}", config.neo4j.uri);
    println!("   Blocks dir: {}", config.bitcoin.blocks_dir);

    // Connect to Neo4j
    println!("\n🔌 Connecting to Neo4j...");
    let writer = Neo4jWriter::new(config.neo4j.clone())
        .await
        .expect("Failed to connect to Neo4j");
    println!("   ✅ Connected");

    // Create orchestrator
    let cache_size = config.performance.cache_capacity();
    println!("\n🔧 Creating orchestrator (cache: {} entries)...", cache_size);
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, cache_size);

    // Initialize schema
    println!("\n🏗️  Initializing schema...");
    orchestrator.init_schema().await.expect("Failed to init schema");
    println!("   ✅ Schema ready");

    // Check resume point
    let resume_height = orchestrator.get_resume_height().await
        .expect("Failed to get resume height");
    println!("\n📍 Resume from block: {}", resume_height);

    if resume_height > 0 {
        println!("\n⚠️  Database is not empty!");
        println!("   Clear first with: cargo test --test clear_neo4j -- --ignored --nocapture");
        panic!("Database must be empty for this test");
    }

    // Load blocks from first 4 files using batch loader
    println!("\n📂 Loading and sorting blocks from first 4 files...");
    let load_start = Instant::now();

    let mut loader = BatchedBlockLoader::new(&config.bitcoin.blocks_dir, Network::Bitcoin);
    let blocks = match loader.load_files(&[0, 1, 2, 3]) {
        Ok(blocks) => {
            let load_duration = load_start.elapsed();
            println!("   ✅ Loaded {} blocks in {:.1}s", blocks.len(), load_duration.as_secs_f64());
            blocks
        }
        Err(e) => {
            println!("   ❌ Failed to load blocks: {:?}", e);
            panic!("Block loading failed");
        }
    };

    // Ingest blocks in batches
    let batch_size = 1000;
    let ingest_start = Instant::now();

    match orchestrator.ingest_blocks_batch(&blocks, batch_size).await {
        Ok(_) => {
            let ingest_duration = ingest_start.elapsed();
            let blocks_per_sec = blocks.len() as f64 / ingest_duration.as_secs_f64();

            println!("\n✅ Batch ingestion completed!");
            println!("   Total blocks: {}", blocks.len());
            println!("   Time: {:.1}s", ingest_duration.as_secs_f64());
            println!("   Speed: {:.1} blocks/sec", blocks_per_sec);
        }
        Err(e) => {
            println!("\n❌ Batch ingestion failed: {:?}", e);
            panic!("Ingestion failed");
        }
    }

    // Final statistics
    let final_stats = orchestrator.cache_stats();
    let final_height = orchestrator.get_resume_height().await
        .expect("Failed to get final height");
    let overall_duration = load_start.elapsed();

    println!("\n📊 Final Statistics:");
    println!("┌────────────────────────────────────────────────────────────────────┐");
    println!("│ Total blocks ingested: {:44} │", blocks.len());
    println!("│ Final height: {:54} │", final_height);
    println!("│ Overall time (load + ingest): {:33.1} seconds │", overall_duration.as_secs_f64());
    println!("│ Ingestion speed: {:43.1} blocks/sec │",
        blocks.len() as f64 / ingest_start.elapsed().as_secs_f64());
    println!("├────────────────────────────────────────────────────────────────────┤");
    println!("│ UTXO Cache Statistics:                                             │");
    println!("│   Total lookups: {:49} │", final_stats.hits + final_stats.misses);
    println!("│   Cache hits: {:46} ({:.1}%) │", final_stats.hits, final_stats.hit_rate_percent());
    println!("│   Cache misses: {:44} ({:.1}%) │", final_stats.misses, 100.0 - final_stats.hit_rate_percent());
    println!("│   Cache size: {:43} / {:6} │", orchestrator.cache_size(), cache_size);
    println!("└────────────────────────────────────────────────────────────────────┘");

    println!("\n✅ Test completed successfully!");
    println!("   All blocks from first 4 files processed with batch optimization");

    println!("\n💡 Query checkpoint:");
    println!("   MATCH (c:IngestionCheckpoint) RETURN c.lastProcessedHeight");
}
