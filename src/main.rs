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
use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use bitcoin_chain_graph::config::{Config, ConfigLoader};
use bitcoin_chain_graph::domain::IngestionOrchestrator;
use bitcoin_chain_graph::parser::SingleBlockLoader;
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

    /// Start fresh ingestion from genesis block (lazy streaming mode)
    Ingest {
        /// Maximum block height to process (default: all blocks)
        #[arg(long)]
        max_height: Option<u32>,
    },

    /// Resume ingestion from last checkpoint (lazy streaming with cache pre-warming)
    Resume {
        /// Maximum block height to process (default: all blocks)
        #[arg(long)]
        max_height: Option<u32>,
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

    // Initialize logging
    init_logging(&config);

    match cli.command {
        Commands::InitSchema => init_schema(&config).await,
        Commands::Ingest { max_height } => ingest(&config, max_height).await,
        Commands::Resume { max_height } => resume(&config, max_height).await,
        Commands::Status => status(&config).await,
    }
}

/// Initialize tracing subscriber for structured logging
fn init_logging(config: &Config) {
    // Check for RUST_LOG environment variable, otherwise use config
    let log_level = std::env::var("RUST_LOG")
        .unwrap_or_else(|_| config.logging.level.clone());

    // Create env filter
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&log_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // Setup formatting layer
    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(false)
        .with_line_number(false)
        .with_ansi(true);

    // Initialize subscriber
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();

    tracing::info!(
        log_level = %log_level,
        config_file = ?config,
        "Logging initialized"
    );
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

    let cache_size = config.performance.cache_capacity();
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

/// Start fresh ingestion from genesis block (streaming mode)
async fn ingest(config: &Config, cli_max_height: Option<u32>) -> Result<()> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Start Fresh Ingestion (Streaming Mode)                       ║");
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
        config.performance.cache_capacity()
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

    // Determine max_height: CLI arg > config.bitcoin.end_height > chain tip
    let max_height = cli_max_height
        .or(config.bitcoin.end_height)
        .unwrap_or(u32::MAX);  // Will stop when blocks run out

    // Create lazy-loading block loader (instant startup)
    println!("\n📚 Initializing lazy block loader (instant startup)...");
    let loader = SingleBlockLoader::new(&config.bitcoin.blocks_dir, Network::Bitcoin)
        .context("Failed to initialize block loader")?;

    println!("   Target height range: 0 to {}",
        if max_height == u32::MAX { "chain tip".to_string() } else { max_height.to_string() });

    // Start streaming ingestion (no pre-warming for fresh start)
    run_streaming_ingestion(config, orchestrator, loader, 0, max_height, false).await
}

/// Resume ingestion from last checkpoint (streaming with pre-warming)
async fn resume(config: &Config, cli_max_height: Option<u32>) -> Result<()> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Resume Ingestion (Streaming with Pre-warming)                ║");
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
        config.performance.cache_capacity()
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

    // Determine max_height: CLI arg > config.bitcoin.end_height > chain tip
    let max_height = cli_max_height
        .or(config.bitcoin.end_height)
        .unwrap_or(u32::MAX);  // Will stop when blocks run out

    // Create lazy-loading block loader (instant startup)
    tracing::info!("Initializing lazy block loader");
    let loader = SingleBlockLoader::new(&config.bitcoin.blocks_dir, Network::Bitcoin)
        .context("Failed to initialize block loader")?;

    tracing::info!(
        start = resume_height,
        end = if max_height == u32::MAX { "chain tip".to_string() } else { max_height.to_string() },
        "Target height range"
    );

    if resume_height >= max_height && max_height != u32::MAX {
        tracing::info!("No new blocks to process. Ingestion is up to date");
        return Ok(());
    }

    // Start streaming ingestion with pre-warming
    run_streaming_ingestion(config, orchestrator, loader, resume_height, max_height, true).await
}

/// Core streaming ingestion function with optional cache pre-warming
async fn run_streaming_ingestion(
    config: &Config,
    orchestrator: IngestionOrchestrator<Neo4jWriter>,
    mut loader: SingleBlockLoader,
    start_height: u32,
    max_height: u32,
    enable_prewarm: bool,
) -> Result<()> {
    let start_time = Instant::now();
    let mut blocks_processed = 0;

    // Pre-warm cache if resuming and configured
    if enable_prewarm && config.performance.utxo_prewarm_depth > 0 && start_height > 0 {
        let cache = orchestrator.get_cache();
        cache.enable_prewarm_mode();

        let prewarm_blocks = loader.prewarm_cache(
            cache,
            start_height,
            config.performance.utxo_prewarm_depth
        ).await
        .context("Cache pre-warming failed")?;

        cache.disable_prewarm_mode();

        println!("   Pre-warmed {} blocks, cache at {:.1}%",
                 prewarm_blocks, cache.fill_percentage() * 100.0);
    }

    // Pre-load full index range (single scan optimization)
    if max_height != u32::MAX {
        tracing::info!("Pre-loading index for range {}-{}", start_height, max_height);
        loader.preload_full_range(start_height, max_height)
            .context("Failed to preload index range")?;
    }

    // Start streaming ingestion with batching
    tracing::info!("Starting streaming ingestion with batching");
    let batch_size = config.ingestion.batch_size;
    tracing::info!(batch_size = batch_size, "Batch configuration");

    let mut batch: Vec<(u32, bitcoin::Block, String)> = Vec::with_capacity(batch_size);
    let mut batch_start_height = start_height;

    for height in start_height..=max_height {
        // Load single block (show progress during loading)
        if batch.is_empty() {
            batch_start_height = height;
            let end_range = (height + batch_size as u32 - 1).min(max_height);
            tracing::info!(
                start = height,
                end = end_range,
                "Loading batch"
            );
        }

        // Show progress every 50 blocks during loading
        if height > batch_start_height && (height - batch_start_height) % 50 == 0 {
            let progress = height - batch_start_height;
            let total = batch_size.min((max_height - batch_start_height + 1) as usize);
            let pct = (progress as f64 / total as f64) * 100.0;
            tracing::debug!(
                progress = progress,
                total = total,
                percent = format!("{:.0}%", pct),
                "Batch loading progress"
            );
        }

        let (h, block, file_name) = match loader.load_block(height)? {
            Some(data) => data,
            None => {
                tracing::warn!(height = height, "Block not found, stopping ingestion");
                break;
            }
        };

        // Add to batch
        batch.push((h, block, file_name));

        // Ingest batch when full or at end
        if batch.len() >= batch_size || height == max_height {
            let batch_start = Instant::now();
            let batch_start_height = batch.first().unwrap().0;
            let batch_end_height = batch.last().unwrap().0;

            tracing::info!(
                start = batch_start_height,
                end = batch_end_height,
                "Ingesting batch"
            );

            orchestrator.ingest_blocks_batch(&batch, batch_size).await
                .with_context(|| format!("Failed to ingest batch starting at block {}", batch_start_height))?;

            blocks_processed += batch.len();

            let elapsed = batch_start.elapsed().as_secs_f64();
            let bps = batch.len() as f64 / elapsed;

            let stats = orchestrator.cache_stats();
            let cache = orchestrator.get_cache();
            tracing::info!(
                blocks_per_sec = format!("{:.1}", bps),
                cache_size = orchestrator.cache_size(),
                cache_capacity = config.performance.cache_capacity(),
                cache_memory_mb = config.performance.utxo_cache_memory_mb,
                cache_fill_pct = format!("{:.1}", cache.fill_percentage() * 100.0),
                hit_rate_pct = format!("{:.1}", stats.hit_rate_percent()),
                "Batch complete"
            );

            // Clear batch for next iteration
            batch.clear();
        }
    }

    let total_duration = start_time.elapsed();
    let overall_bps = blocks_processed as f64 / total_duration.as_secs_f64();

    // Show final cache stats
    let stats = orchestrator.cache_stats();

    tracing::info!(
        blocks_processed = blocks_processed,
        duration_secs = format!("{:.2}", total_duration.as_secs_f64()),
        avg_blocks_per_sec = format!("{:.2}", overall_bps),
        cache_size = orchestrator.cache_size(),
        cache_capacity = config.performance.cache_capacity(),
        cache_memory_mb = config.performance.utxo_cache_memory_mb,
        cache_fill_pct = format!("{:.1}", orchestrator.cache_size() as f64 / config.performance.cache_capacity() as f64 * 100.0),
        hit_rate_pct = format!("{:.2}", stats.hit_rate_percent()),
        hits = stats.hits,
        misses = stats.misses,
        "Streaming ingestion complete"
    );

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

    let cache_size = config.performance.cache_capacity();
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
