//! Bitcoin Chain Graph - CLI Application
//!
//! High-performance Bitcoin blockchain ingestion into Neo4j with checkpoint management.
//!
//! Commands:
//! - init-schema: Initialize Neo4j schema and create initial checkpoint
//! - ingest: Start fresh ingestion from genesis block
//! - resume: Resume ingestion from last checkpoint
//! - status: Display checkpoint status and progress
//!
//! Example usage:
//! ```bash
//! # Initialize schema
//! cargo run -- init-schema --config config/default.toml
//!
//! # Start ingestion
//! cargo run -- ingest --config config/default.toml
//!
//! # Resume after interruption
//! cargo run -- resume --config config/default.toml
//!
//! # Check status
//! cargo run -- status --config config/default.toml
//! ```

use anyhow::{Context, Result};
use bitcoin::Network;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Instant;

use bitcoin_chain_graph::config::{Config, ConfigLoader};
use bitcoin_chain_graph::domain::IngestionOrchestrator;
use bitcoin_chain_graph::parser::BatchedBlockLoader;
use bitcoin_chain_graph::writer::Neo4jWriter;

/// Bitcoin Chain Graph - Blockchain ingestion into Neo4j
#[derive(Parser)]
#[command(name = "bitcoin-chain-graph")]
#[command(about = "High-performance Bitcoin blockchain ingestion into Neo4j", long_about = None)]
#[command(version)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, value_name = "FILE", default_value = "config/default.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize Neo4j schema and create initial checkpoint
    InitSchema,

    /// Start fresh ingestion from genesis block
    Ingest {
        /// Number of .blk files to process (default: all files)
        #[arg(short, long)]
        files: Option<u32>,

        /// Starting file number (default: 0 for blk00000.dat)
        #[arg(long, default_value = "0")]
        start_file: u32,
    },

    /// Resume ingestion from last checkpoint
    Resume {
        /// Number of .blk files to process from resume point (default: all remaining)
        #[arg(short, long)]
        files: Option<u32>,
    },

    /// Display checkpoint status and progress
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load configuration
    let config = ConfigLoader::from_file(&cli.config)
        .with_context(|| format!("Failed to load config from {:?}", cli.config))?;

    config.validate()
        .context("Configuration validation failed")?;

    match cli.command {
        Commands::InitSchema => init_schema(&config).await,
        Commands::Ingest { files, start_file } => ingest(&config, start_file, files).await,
        Commands::Resume { files } => resume(&config, files).await,
        Commands::Status => status(&config).await,
    }
}

/// Initialize Neo4j schema and create initial checkpoint
async fn init_schema(config: &Config) -> Result<()> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Initialize Neo4j Schema                                       ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    println!("🔌 Connecting to Neo4j at {}...", config.neo4j.uri);
    let writer = Neo4jWriter::new(config.neo4j.clone())
        .await
        .context("Failed to connect to Neo4j")?;
    println!("   ✅ Connected successfully");

    let cache_size = config.performance.utxo_cache_size;
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, cache_size);

    println!("\n🏗️  Initializing schema (constraints + indexes)...");
    orchestrator.init_schema()
        .await
        .context("Failed to initialize schema")?;
    println!("   ✅ Schema initialized");

    println!("\n✅ Initialization complete!");
    println!("\nNext steps:");
    println!("  cargo run -- ingest    # Start ingestion from genesis");
    println!("  cargo run -- status    # Check checkpoint status");

    Ok(())
}

/// Start fresh ingestion from genesis block
async fn ingest(config: &Config, start_file: u32, file_count: Option<u32>) -> Result<()> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Bitcoin Chain Graph Ingestion                                 ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    println!("📝 Configuration:");
    println!("   Neo4j URI: {}", config.neo4j.uri);
    println!("   Database: {}", config.neo4j.database);
    println!("   Blocks dir: {}", config.bitcoin.blocks_dir);
    println!("   Batch size: {}", config.ingestion.batch_size);
    println!("   UTXO cache: {} entries (~{:.1} MB)",
        config.performance.utxo_cache_size,
        (config.performance.utxo_cache_size as f64 * 138.0) / 1_000_000.0
    );

    // Connect to Neo4j
    println!("\n🔌 Connecting to Neo4j...");
    let writer = Neo4jWriter::new(config.neo4j.clone())
        .await
        .context("Failed to connect to Neo4j")?;
    println!("   ✅ Connected successfully");

    // Create orchestrator
    let orchestrator = IngestionOrchestrator::new(
        writer,
        Network::Bitcoin,
        config.performance.utxo_cache_size
    );

    // Check if schema is initialized
    let checkpoint = orchestrator.get_checkpoint().await
        .context("Failed to check checkpoint")?;

    if checkpoint.is_none() {
        println!("\n⚠️  Warning: No checkpoint found. Initializing schema...");
        orchestrator.init_schema().await
            .context("Failed to initialize schema")?;
        println!("   ✅ Schema initialized");
    }

    // Get resume height
    let resume_height = orchestrator.get_resume_height().await
        .context("Failed to get resume height")?;

    if resume_height > 0 {
        println!("\n⚠️  Warning: Existing ingestion found at block {}!", resume_height);
        println!("   Use 'resume' command to continue from last checkpoint");
        println!("   Or manually reset the checkpoint to start fresh");
        return Ok(());
    }

    println!("\n📂 Loading .blk files...");
    let end_file = if let Some(count) = file_count {
        start_file + count - 1
    } else {
        // Default: process up to blk00099.dat (or until files don't exist)
        start_file + 99
    };

    let file_numbers: Vec<u32> = (start_file..=end_file).collect();
    println!("   File range: blk{:05}.dat to blk{:05}.dat", start_file, end_file);

    let mut loader = BatchedBlockLoader::new(&config.bitcoin.blocks_dir, Network::Bitcoin);
    let blocks = loader.load_files(&file_numbers)
        .context("Failed to load block files")?;

    if blocks.is_empty() {
        println!("\n❌ No blocks found in specified file range");
        return Ok(());
    }

    println!("   ✅ Loaded {} blocks", blocks.len());

    // Start ingestion
    println!("\n🚀 Starting batch ingestion...");
    let start = Instant::now();

    orchestrator.ingest_blocks_batch(&blocks, config.ingestion.batch_size)
        .await
        .context("Batch ingestion failed")?;

    let duration = start.elapsed();
    let bps = blocks.len() as f64 / duration.as_secs_f64();

    println!("\n✅ Ingestion complete!");
    println!("   Blocks: {}", blocks.len());
    println!("   Duration: {:.2}s", duration.as_secs_f64());
    println!("   Speed: {:.2} blocks/sec", bps);

    // Show cache stats
    let stats = orchestrator.cache_stats();
    println!("\n💾 UTXO Cache:");
    println!("   Final size: {}", orchestrator.cache_size());
    println!("   Hit rate: {:.2}%", stats.hit_rate_percent());
    println!("   Hits: {}, Misses: {}", stats.hits, stats.misses);

    Ok(())
}

/// Resume ingestion from last checkpoint
async fn resume(config: &Config, file_count: Option<u32>) -> Result<()> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Resume Ingestion from Checkpoint                              ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    // Connect to Neo4j
    println!("🔌 Connecting to Neo4j at {}...", config.neo4j.uri);
    let writer = Neo4jWriter::new(config.neo4j.clone())
        .await
        .context("Failed to connect to Neo4j")?;
    println!("   ✅ Connected successfully");

    // Create orchestrator
    let orchestrator = IngestionOrchestrator::new(
        writer,
        Network::Bitcoin,
        config.performance.utxo_cache_size
    );

    // Get checkpoint
    let checkpoint = orchestrator.get_checkpoint().await
        .context("Failed to get checkpoint")?
        .context("No checkpoint found. Run 'init-schema' first.")?;

    println!("\n📍 Checkpoint Status:");
    println!("   Last processed height: {}", checkpoint.last_processed_height);
    println!("   Last processed hash: {}", checkpoint.last_processed_hash);
    println!("   Last processed file: {}", checkpoint.last_processed_file);
    println!("   Status: {}", checkpoint.status);

    if checkpoint.status == "completed" {
        println!("\n✅ Ingestion already complete!");
        return Ok(());
    }

    if checkpoint.status == "error" {
        println!("\n⚠️  Warning: Last ingestion ended with error status");
        println!("   Review logs and fix any issues before resuming");
        println!("   To resume anyway, the error status will be updated to 'in_progress'");
    }

    let resume_height = orchestrator.get_resume_height().await
        .context("Failed to calculate resume height")?;

    println!("\n🔄 Resume Plan:");
    println!("   Resume from block: {}", resume_height);
    println!("   Starting file: {}", checkpoint.last_processed_file);

    // Parse file number from checkpoint
    let last_file_num = parse_file_number(&checkpoint.last_processed_file)?;
    let start_file = last_file_num;

    let end_file = if let Some(count) = file_count {
        start_file + count - 1
    } else {
        start_file + 99 // Default: process remaining files
    };

    println!("   File range: blk{:05}.dat to blk{:05}.dat", start_file, end_file);

    println!("\n📂 Loading blocks from resume point...");
    let file_numbers: Vec<u32> = (start_file..=end_file).collect();

    let mut loader = BatchedBlockLoader::new(&config.bitcoin.blocks_dir, Network::Bitcoin);
    let all_blocks = loader.load_files(&file_numbers)
        .context("Failed to load block files")?;

    // Filter to blocks >= resume_height
    let blocks: Vec<_> = all_blocks.into_iter()
        .filter(|(height, _, _)| *height >= resume_height)
        .collect();

    if blocks.is_empty() {
        println!("\n✅ No new blocks to process. Ingestion is up to date!");
        return Ok(());
    }

    println!("   ✅ Loaded {} blocks to process", blocks.len());

    // Start ingestion
    println!("\n🚀 Resuming batch ingestion...");
    let start = Instant::now();

    orchestrator.ingest_blocks_batch(&blocks, config.ingestion.batch_size)
        .await
        .context("Batch ingestion failed")?;

    let duration = start.elapsed();
    let bps = blocks.len() as f64 / duration.as_secs_f64();

    println!("\n✅ Resume complete!");
    println!("   Blocks processed: {}", blocks.len());
    println!("   Duration: {:.2}s", duration.as_secs_f64());
    println!("   Speed: {:.2} blocks/sec", bps);

    // Show cache stats
    let stats = orchestrator.cache_stats();
    println!("\n💾 UTXO Cache:");
    println!("   Final size: {}", orchestrator.cache_size());
    println!("   Hit rate: {:.2}%", stats.hit_rate_percent());

    Ok(())
}

/// Display checkpoint status and progress
async fn status(config: &Config) -> Result<()> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Checkpoint Status                                             ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    // Connect to Neo4j
    println!("🔌 Connecting to Neo4j at {}...", config.neo4j.uri);
    let writer = Neo4jWriter::new(config.neo4j.clone())
        .await
        .context("Failed to connect to Neo4j")?;

    let cache_size = config.performance.utxo_cache_size;
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, cache_size);

    // Get checkpoint
    let checkpoint = orchestrator.get_checkpoint().await
        .context("Failed to query checkpoint")?;

    match checkpoint {
        Some(cp) => {
            println!("📍 Current Checkpoint:");
            println!("   Status: {}", cp.status);
            println!("   Last processed height: {}", cp.last_processed_height);
            println!("   Last processed hash: {}", cp.last_processed_hash);
            println!("   Last processed file: {}", cp.last_processed_file);

            if let Some(offset) = cp.last_processed_file_offset {
                println!("   File offset: {} bytes", offset);
            }

            let timestamp = chrono::DateTime::from_timestamp(cp.timestamp, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            println!("   Last updated: {}", timestamp);

            println!("\n🔄 Resume Information:");
            let resume_height = orchestrator.get_resume_height().await?;
            println!("   Next block to process: {}", resume_height);

            if cp.status == "in_progress" {
                println!("\n💡 Tip: Use 'resume' to continue ingestion");
            } else if cp.status == "completed" {
                println!("\n✅ Ingestion is complete!");
            } else if cp.status == "error" {
                println!("\n⚠️  Last ingestion ended with errors");
                println!("   Review logs and use 'resume' to retry");
            }
        }
        None => {
            println!("❌ No checkpoint found");
            println!("\n💡 Next steps:");
            println!("   1. Run: cargo run -- init-schema");
            println!("   2. Run: cargo run -- ingest");
        }
    }

    Ok(())
}

/// Parse file number from filename (e.g., "blk00000.dat" -> 0)
fn parse_file_number(filename: &str) -> Result<u32> {
    let num_str = filename
        .strip_prefix("blk")
        .and_then(|s| s.strip_suffix(".dat"))
        .context("Invalid file name format")?;

    num_str.parse::<u32>()
        .context("Failed to parse file number")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_file_number() {
        assert_eq!(parse_file_number("blk00000.dat").unwrap(), 0);
        assert_eq!(parse_file_number("blk00001.dat").unwrap(), 1);
        assert_eq!(parse_file_number("blk00042.dat").unwrap(), 42);
        assert_eq!(parse_file_number("blk00999.dat").unwrap(), 999);

        assert!(parse_file_number("invalid.dat").is_err());
        assert!(parse_file_number("blk.dat").is_err());
    }
}
