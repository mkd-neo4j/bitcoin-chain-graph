//! Configuration module for Bitcoin Chain Graph
//!
//! Provides configuration structures and loading mechanisms for the application.

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

mod loader;
pub use loader::ConfigLoader;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Bitcoin blockchain data configuration
    pub bitcoin: BitcoinConfig,

    /// Neo4j database connection configuration
    pub neo4j: Neo4jConfig,

    /// Ingestion process configuration
    pub ingestion: IngestionConfig,

    /// Performance tuning configuration
    pub performance: PerformanceConfig,

    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,

    /// Bitcoin Core RPC configuration (optional, for live mode)
    #[serde(default)]
    pub bitcoin_rpc: Option<BitcoinRpcConfig>,
}

/// Bitcoin blockchain data configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitcoinConfig {
    /// Path to Bitcoin blocks directory containing .blk files
    ///
    /// Examples:
    /// - Development: "./test_data/blocks"
    /// - Production: "/data/bitcoin/blocks"
    /// - Bitcoin Core default (Linux): "~/.bitcoin/blocks"
    /// - Bitcoin Core default (macOS): "~/Library/Application Support/Bitcoin/blocks"
    pub blocks_dir: String,

    /// Starting block height for ingestion (default: 0 for genesis)
    pub start_height: u32,

    /// Optional ending block height for bounded ingestion
    /// If None, will sync to chain tip
    pub end_height: Option<u32>,
}

/// Neo4j database connection and pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Neo4jConfig {
    /// Neo4j bolt URI (e.g., "bolt://localhost:7687")
    pub uri: String,

    /// Database username
    pub user: String,

    /// Database password
    pub password: String,

    /// Database name (default: "neo4j")
    pub database: String,

    /// Maximum number of connections in pool
    pub max_connections: usize,

    /// Minimum number of keep-alive connections
    pub min_connections: usize,

    /// Connection timeout in seconds
    pub connection_timeout_secs: u64,

    /// Number of rows to fetch per batch
    pub fetch_size: usize,

    /// Maximum number of query retries on failure
    pub max_retries: usize,

    /// Batch size for writing records to Neo4j (chunk size for UNWIND queries)
    pub write_batch_size: usize,

    /// Query timeout in seconds (prevents hanging on dead connections)
    #[serde(default = "default_query_timeout_secs")]
    pub query_timeout_secs: u64,
}

fn default_query_timeout_secs() -> u64 {
    120 // 2 minutes default
}

fn default_utxo_lookup_batch_size() -> usize {
    1000 // Batch up to 1000 transactions' input lookups together
}

/// Ingestion process configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionConfig {
    /// Number of blocks to process per batch.
    /// This is the primary control for memory usage — larger batches use more RAM.
    pub batch_size: usize,

    /// Enable validation after ingestion
    pub enable_validation: bool,

    /// Validate every N blocks
    pub validate_every_n_blocks: u32,

    /// Update checkpoint every N blocks (for performance)
    pub checkpoint_interval: u32,

    /// Automatically resume from last checkpoint on startup
    pub auto_resume: bool,

    /// Validate checkpoint integrity before resume
    pub validate_on_resume: bool,
}

/// Performance tuning configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// UTXO cache memory budget in megabytes
    ///
    /// Configures the maximum memory used by the UTXO cache for storing
    /// recent transaction outputs. Each cached output uses ~72 bytes
    /// (36-byte UtxoKey + 36-byte CachedOutput value).
    ///
    /// Memory to entry conversion:
    /// - 2 MB ≈ 28,000 entries (low resource)
    /// - 15 MB ≈ 208,000 entries
    /// - 140 MB ≈ 1,944,000 entries (default)
    /// - 500 MB ≈ 6,940,000 entries (high performance)
    /// - 1400 MB ≈ 19,440,000 entries (ultra)
    ///
    /// Higher memory = better cache hit rate = fewer Neo4j queries = faster ingestion
    pub utxo_cache_memory_mb: usize,

    /// Number of blocks to load backwards for cache pre-warming
    ///
    /// Before starting forward ingestion, load this many blocks backwards
    /// to populate the UTXO cache from files (fast) instead of Neo4j (slow).
    /// This dramatically improves cache hit rates during ingestion.
    ///
    /// Recommended values:
    /// - Low resource: 1,000 blocks
    /// - Default: 10,000 blocks
    /// - High performance: 10,000 blocks
    /// - Ultra performance: 10,000+ blocks
    ///
    /// Set to 0 to disable pre-warming.
    /// Note: This setting is only used in file-based ingestion (resume command),
    /// not in live mode which starts with an empty cache.
    #[serde(default)]
    pub utxo_prewarm_depth: u32,

    /// Maximum number of transactions to batch for UTXO lookups.
    ///
    /// During ingestion, input UTXOs must be looked up to calculate transaction
    /// amounts. Instead of querying per-transaction (N round-trips), we batch
    /// lookups across multiple transactions (1 round-trip for all cache misses).
    ///
    /// Higher values = fewer Neo4j round-trips during cold cache, but more memory.
    /// Default: 1000 transactions per batch.
    #[serde(default = "default_utxo_lookup_batch_size")]
    pub utxo_lookup_batch_size: usize,

    /// Number of concurrent batch writes
    pub parallel_batches: usize,

    /// Report progress every N blocks
    pub progress_report_interval: u32,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level: trace, debug, info, warn, error
    /// Can be overridden with RUST_LOG environment variable
    pub level: String,

    /// Enable structured JSON logging (false = human-readable)
    pub json_format: bool,
}

/// Bitcoin Core RPC configuration for live mode
///
/// Required for the `live` command to connect to a running Bitcoin Core node.
/// When using SSH tunnel, set url to "http://localhost:8332".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitcoinRpcConfig {
    /// RPC endpoint URL (e.g., "http://localhost:8332")
    pub url: String,

    /// RPC username (must match bitcoin.conf rpcuser)
    pub user: String,

    /// RPC password (must match bitcoin.conf rpcpassword)
    pub password: String,

    /// Number of blocks to fetch in parallel during catchup
    /// Higher = faster catchup, but more load on bitcoind
    #[serde(default = "default_rpc_batch_size")]
    pub batch_size: usize,

    /// Timeout for individual RPC requests in seconds
    #[serde(default = "default_rpc_timeout")]
    pub timeout_secs: u64,

    /// ZMQ endpoint for real-time block notifications
    /// Must match bitcoin.conf zmqpubhashblock setting
    pub zmq_endpoint: String,

    /// Maximum concurrent RPC requests during batch fetching.
    /// Limits in-flight requests to prevent overwhelming bitcoind.
    /// With batch_size=200 and max_concurrent=32, at most 32 block fetches
    /// run at once; the rest queue behind a stream buffer.
    #[serde(default = "default_rpc_max_concurrent")]
    pub rpc_max_concurrent: usize,

    /// Maximum ZMQ reconnection attempts before giving up
    #[serde(default = "default_zmq_max_reconnect_attempts")]
    pub zmq_max_reconnect_attempts: u32,

    /// Base delay for ZMQ reconnection backoff in milliseconds
    /// Actual delay: base_delay_ms * 2^(attempt-1)
    #[serde(default = "default_zmq_reconnect_base_delay_ms")]
    pub zmq_reconnect_base_delay_ms: u64,

    /// ZMQ recv timeout in minutes (0 = no timeout)
    /// If no block is received within this time, the connection is
    /// considered stale and will be reconnected.
    #[serde(default = "default_zmq_recv_timeout_mins")]
    pub zmq_recv_timeout_mins: u64,

    /// Maximum consecutive ZMQ failures before the process exits
    #[serde(default = "default_zmq_max_consecutive_failures")]
    pub zmq_max_consecutive_failures: u32,
}

fn default_rpc_batch_size() -> usize {
    50
}
fn default_rpc_timeout() -> u64 {
    30
}
fn default_rpc_max_concurrent() -> usize {
    32
}
fn default_zmq_max_reconnect_attempts() -> u32 {
    5
}
fn default_zmq_reconnect_base_delay_ms() -> u64 {
    1000
}
fn default_zmq_recv_timeout_mins() -> u64 {
    60
}
fn default_zmq_max_consecutive_failures() -> u32 {
    10
}

impl Default for BitcoinRpcConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:8332".to_string(),
            user: "btcgraph".to_string(),
            password: "password".to_string(),
            batch_size: 50,
            timeout_secs: 30,
            rpc_max_concurrent: 32,
            zmq_endpoint: "tcp://127.0.0.1:28332".to_string(),
            zmq_max_reconnect_attempts: 5,
            zmq_reconnect_base_delay_ms: 1000,
            zmq_recv_timeout_mins: 60,
            zmq_max_consecutive_failures: 10,
        }
    }
}

/// Configuration errors
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to load config file: {0}")]
    LoadError(String),

    #[error("Failed to parse config: {0}")]
    ParseError(String),

    #[error("Invalid configuration: {0}")]
    ValidationError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

impl Config {
    /// Load configuration from TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        ConfigLoader::from_file(path)
    }

    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self, ConfigError> {
        ConfigLoader::from_env()
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Validate Neo4j config
        if self.neo4j.uri.is_empty() {
            return Err(ConfigError::ValidationError(
                "Neo4j URI cannot be empty".into(),
            ));
        }

        if self.neo4j.max_connections == 0 {
            return Err(ConfigError::ValidationError(
                "max_connections must be > 0".into(),
            ));
        }

        if self.neo4j.min_connections > self.neo4j.max_connections {
            return Err(ConfigError::ValidationError(
                "min_connections cannot exceed max_connections".into(),
            ));
        }

        // Validate ingestion config
        if self.ingestion.batch_size == 0 {
            return Err(ConfigError::ValidationError(
                "batch_size must be > 0".into(),
            ));
        }

        // Validate performance config
        if self.performance.utxo_cache_memory_mb == 0 {
            return Err(ConfigError::ValidationError(
                "utxo_cache_memory_mb must be > 0".into(),
            ));
        }

        if self.performance.parallel_batches == 0 {
            return Err(ConfigError::ValidationError(
                "parallel_batches must be > 0".into(),
            ));
        }

        Ok(())
    }

    /// Validate Bitcoin RPC configuration for live mode.
    ///
    /// Call this before using the `live` command to get clear validation errors
    /// instead of cryptic runtime failures.
    pub fn validate_rpc(&self) -> Result<(), ConfigError> {
        let rpc = self.bitcoin_rpc.as_ref().ok_or_else(|| {
            ConfigError::ValidationError("[bitcoin_rpc] section is required for live mode".into())
        })?;

        if rpc.url.is_empty() {
            return Err(ConfigError::ValidationError(
                "bitcoin_rpc.url cannot be empty".into(),
            ));
        }

        if rpc.zmq_endpoint.is_empty() {
            return Err(ConfigError::ValidationError(
                "bitcoin_rpc.zmq_endpoint cannot be empty".into(),
            ));
        }

        if rpc.timeout_secs == 0 {
            return Err(ConfigError::ValidationError(
                "bitcoin_rpc.timeout_secs must be > 0".into(),
            ));
        }

        if rpc.batch_size == 0 {
            return Err(ConfigError::ValidationError(
                "bitcoin_rpc.batch_size must be > 0".into(),
            ));
        }

        Ok(())
    }
}

#[allow(clippy::derivable_impls)]
impl Default for Config {
    fn default() -> Self {
        Self {
            bitcoin: BitcoinConfig::default(),
            neo4j: Neo4jConfig::default(),
            ingestion: IngestionConfig::default(),
            performance: PerformanceConfig::default(),
            logging: LoggingConfig::default(),
            bitcoin_rpc: None,
        }
    }
}

impl Default for BitcoinConfig {
    fn default() -> Self {
        Self {
            blocks_dir: "/data/bitcoin/blocks".to_string(),
            start_height: 0,
            end_height: None,
        }
    }
}

impl Default for Neo4jConfig {
    fn default() -> Self {
        Self {
            uri: "bolt://localhost:7687".to_string(),
            user: "neo4j".to_string(),
            password: "password".to_string(),
            database: "neo4j".to_string(),
            max_connections: 20,
            min_connections: 2,
            connection_timeout_secs: 30,
            fetch_size: 500,
            max_retries: 3,
            write_batch_size: 5000,
            query_timeout_secs: default_query_timeout_secs(),
        }
    }
}

impl Default for IngestionConfig {
    fn default() -> Self {
        Self {
            batch_size: 5000,
            enable_validation: true,
            validate_every_n_blocks: 10000,
            checkpoint_interval: 10,
            auto_resume: true,
            validate_on_resume: true,
        }
    }
}

impl PerformanceConfig {
    /// Convert memory budget to cache entry count
    ///
    /// Uses the approximate size of each cached output (72 bytes: 36-byte
    /// UtxoKey + 36-byte CachedOutput value) to calculate how many entries
    /// can fit in the configured memory budget.
    ///
    /// # Returns
    ///
    /// The maximum number of UTXO cache entries for the configured memory budget
    ///
    /// # Example
    ///
    /// ```
    /// use bitcoin_chain_graph::config::PerformanceConfig;
    ///
    /// let config = PerformanceConfig {
    ///     utxo_cache_memory_mb: 50,
    ///     utxo_prewarm_depth: 50,
    ///     utxo_lookup_batch_size: 1000,
    ///     parallel_batches: 4,
    ///     progress_report_interval: 100,
    /// };
    ///
    /// let capacity = config.cache_capacity();
    /// assert_eq!(capacity, 694_444); // ~694k entries for 50 MB
    /// ```
    pub fn cache_capacity(&self) -> usize {
        const BYTES_PER_ENTRY: usize = 72;
        const BYTES_PER_MB: usize = 1_000_000;

        (self.utxo_cache_memory_mb * BYTES_PER_MB) / BYTES_PER_ENTRY
    }
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            utxo_cache_memory_mb: 140, // ~1.9M entries
            utxo_prewarm_depth: 1_000_000,
            utxo_lookup_batch_size: default_utxo_lookup_batch_size(),
            parallel_batches: 4,
            progress_report_interval: 500,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            json_format: false,
        }
    }
}
