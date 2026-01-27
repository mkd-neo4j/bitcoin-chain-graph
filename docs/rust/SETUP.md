# Rust Implementation Setup

Configuration and setup for building a high-performance Bitcoin blockchain ingestion system in Rust.

---

## Overview

This implementation prioritizes:
- **Memory efficiency**: <2GB resident memory for ingestion process
- **CPU optimization**: Multi-core utilization for parallel processing
- **Zero-copy parsing**: Minimal allocations during block deserialization
- **Fast ingestion**: 10-100 blocks/sec (early chain), 1-5 blocks/sec (modern chain)

---

## Prerequisites

### Required Software

1. **Rust** (latest stable, 1.70+)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustup update stable
   ```

2. **Neo4j** (5.x recommended)
   - Running instance with Bolt protocol enabled
   - Minimum 8GB heap recommended
   - See [Neo4j Installation](https://neo4j.com/docs/operations-manual/current/installation/)

3. **Bitcoin Core Block Files** (`.blk` files)
   - Location: `~/.bitcoin/blocks/` (Linux/Mac) or `%APPDATA%\Bitcoin\blocks\` (Windows)
   - Requires Bitcoin Core to be synced (or partial sync for testing)
   - Files: `blk00000.dat`, `blk00001.dat`, etc.

---

## Project Structure

### 3-Layer Architecture

This project uses a **strict 3-layer architecture** with isolated Neo4j write operations:

```
bitcoin-chain-graph/
├── Cargo.toml              # Project manifest
├── Cargo.lock              # Dependency lock file
├── .cargo/
│   └── config.toml         # Build configuration
├── config/
│   ├── standard.toml       # 2-4GB RAM configuration
│   ├── high-performance.toml
│   └── ultra-performance.toml  # 40GB+ RAM configuration
├── src/
│   ├── main.rs             # CLI entry point (wires layers together)
│   ├── lib.rs              # Public API
│   │
│   ├── parser/             # 🔵 LAYER 1: Bitcoin Data Reading
│   │   ├── mod.rs          # (Zero knowledge of Neo4j)
│   │   ├── block_file.rs   # Stream .blk files (no DB deps)
│   │   ├── address.rs      # Derive addresses (pure function)
│   │   └── script.rs       # Detect script types (pure function)
│   │
│   ├── domain/             # 🟢 LAYER 2: Business Logic
│   │   ├── mod.rs          # (Zero knowledge of Neo4j)
│   │   ├── models.rs       # Domain types (Block, Transaction, etc.)
│   │   ├── ingestion.rs    # Orchestrates 6-phase ingestion
│   │   └── utxo/
│   │       ├── mod.rs
│   │       └── cache.rs    # LRU UTXO cache (uses GraphWriter trait)
│   │
│   ├── writer/             # 🔴 LAYER 3: Database Abstraction
│   │   ├── mod.rs          # Public interface (re-exports)
│   │   ├── traits.rs       # ⭐ GraphWriter trait (the contract)
│   │   ├── neo4j/          # Neo4j-specific implementation
│   │   │   ├── mod.rs      # Neo4jWriter struct
│   │   │   ├── client.rs   # Connection pool (neo4rs::Graph)
│   │   │   ├── schema.rs   # DDL operations (constraints, indexes)
│   │   │   ├── queries.rs  # ⭐ ALL Cypher queries centralized
│   │   │   └── batch.rs    # Batch accumulator
│   │   └── mock.rs         # Mock implementation for testing
│   │
│   ├── checkpoint.rs       # Resumability (checkpointing)
│   ├── config.rs           # Configuration management
│   └── error.rs            # Error types
├── tests/
│   ├── integration_tests.rs    # Uses MockWriter (no Neo4j)
│   ├── parser_tests.rs         # Pure parser tests
│   └── test_data/              # Test block data
├── benches/
│   └── ingestion_bench.rs      # Performance benchmarks
└── README.md
```

### Layer Responsibilities

| Layer | Location | Responsibility | Key Principle |
|-------|----------|---------------|---------------|
| **1. Parser** | `src/parser/` | Read Bitcoin .blk files | ❌ Never imports domain or writer |
| **2. Domain** | `src/domain/` | Business logic, UTXO cache | ❌ Never imports `neo4rs` (uses trait) |
| **3. Writer** | `src/writer/` | ALL database operations | ✅ Only layer that imports `neo4rs` |

### Key Files

**`src/writer/traits.rs`** - The Contract
```rust
/// GraphWriter trait defines ALL database operations
/// Domain code depends on this trait, not Neo4j implementation
#[async_trait]
pub trait GraphWriter: Send + Sync {
    async fn write_blocks(&self, blocks: &[BlockData]) -> Result<()>;
    async fn write_outputs(&self, outputs: &[OutputData]) -> Result<()>;
    async fn lookup_output(&self, output_id: &str) -> Result<OutputData>;
    // ... all other operations
}
```

**`src/writer/neo4j/queries.rs`** - Centralized Queries
```rust
/// ALL Cypher queries in ONE file - easy to find and update
pub const CREATE_BLOCKS_QUERY: &str = r#"..."#;
pub const CREATE_OUTPUTS_QUERY: &str = r#"..."#;
pub const LOOKUP_OUTPUT_QUERY: &str = r#"..."#;
// ... all queries
```

**`src/domain/ingestion.rs`** - Business Logic
```rust
/// Orchestrates ingestion using GraphWriter trait
pub struct IngestionOrchestrator<W: GraphWriter> {
    writer: Arc<W>,  // Generic trait, not Neo4j type
    utxo_cache: UtxoCache<W>,
}
```

**`src/main.rs`** - Dependency Injection
```rust
/// Wires layers together
let writer = Neo4jWriter::new(...).await?;
let orchestrator = IngestionOrchestrator::new(Arc::new(writer));
```

### Why This Structure?

**✅ Easy Neo4j Updates**
- Change a query? Edit `writer/neo4j/queries.rs` only
- Optimize batch size? Edit `writer/neo4j/batch.rs` only
- Domain and parser layers unchanged

**✅ Fast Testing**
- Parser tests: No database needed
- Domain tests: Use `MockWriter` (in-memory)
- Only integration tests need real Neo4j

**✅ Future Flexibility**
- Swap Neo4j for PostgreSQL? Implement new writer
- Use multiple databases? Create composite writer
- Add caching layer? Wrap writer with decorator

**✅ Clean Boundaries**
- Compiler enforces layer boundaries
- No accidental coupling
- Type-safe dependency injection

See [ARCHITECTURE.md](../architecture/ARCHITECTURE.md) for detailed explanation of this design.

---

## Cargo.toml Configuration

```toml
[package]
name = "bitcoin-chain-graph"
version = "0.1.0"
edition = "2021"
rust-version = "1.70"

[dependencies]
# Bitcoin protocol
bitcoin = { version = "0.31", features = ["serde"] }

# Neo4j driver
neo4rs = "0.7"

# Async runtime
tokio = { version = "1.35", features = ["full"] }

# Data parallelism
rayon = "1.8"

# Caching
lru = "0.12"

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Error handling
anyhow = "1.0"
thiserror = "1.0"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# CLI
clap = { version = "4.4", features = ["derive"] }

# Configuration
config = "0.13"

# Utilities
hex = "0.4"
chrono = "0.4"
once_cell = "1.19"

[dev-dependencies]
# Testing
criterion = "0.5"
tempfile = "3.8"
testcontainers = "0.15"

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
strip = true
panic = "abort"

[profile.dev]
opt-level = 0

[profile.bench]
inherits = "release"
```

### Key Dependencies Explained

**Bitcoin Protocol:**
- `bitcoin` (0.31+): Rust Bitcoin library
  - Provides types for Block, Transaction, Script, Address
  - Handles serialization/deserialization
  - Address encoding (Base58Check, Bech32, Bech32m)

**Neo4j:**
- `neo4rs` (0.7+): Async Neo4j driver for Rust
  - Connection pooling
  - Parameterized queries
  - Transaction management

**Async Runtime:**
- `tokio` (1.35+): Async I/O for Neo4j operations
  - Enables concurrent block processing
  - Async file I/O for streaming block files

**Parallelism:**
- `rayon` (1.8+): Data parallelism for CPU-bound work
  - Parallel iteration over independent blocks
  - Work-stealing scheduler

**Caching:**
- `lru` (0.12+): LRU cache for UTXO set
  - Fixed-size cache with automatic eviction
  - Used for recent outputs lookup

---

## Build Configuration

### `.cargo/config.toml`

```toml
[build]
# Use native CPU features for maximum performance
rustflags = ["-C", "target-cpu=native"]

[profile.release]
# Additional optimizations
debug = false
```

### Environment Variables

Create `.env` file in project root:

```bash
# Neo4j connection
NEO4J_URI=bolt://localhost:7687
NEO4J_USER=neo4j
NEO4J_PASSWORD=your_password
NEO4J_DATABASE=neo4j

# Bitcoin block files
BITCOIN_BLOCKS_DIR=/home/user/.bitcoin/blocks

# Ingestion configuration
BATCH_SIZE=50                    # Blocks per Neo4j transaction
UTXO_CACHE_SIZE=100000          # Number of recent outputs to cache
WORKER_THREADS=4                 # Number of parallel workers
LOG_LEVEL=info                   # trace, debug, info, warn, error

# Checkpointing
CHECKPOINT_INTERVAL=1000         # Save checkpoint every N blocks
```

---

## Building the Project

### Development Build

```bash
# Clone repository
git clone https://github.com/yourusername/bitcoin-chain-graph.git
cd bitcoin-chain-graph

# Build in debug mode (fast compilation, slower execution)
cargo build

# Run
cargo run -- --help
```

### Release Build (Production)

```bash
# Build with optimizations (slow compilation, fast execution)
cargo build --release

# Binary location
./target/release/bitcoin-chain-graph
```

### Build Time

- **Debug build**: ~2-5 minutes (initial), <1 minute (incremental)
- **Release build**: ~5-10 minutes (initial), ~2 minutes (incremental)

---

## Running Tests

```bash
# Run all tests
cargo test

# Run specific test module
cargo test parser::tests

# Run integration tests only
cargo test --test integration_tests

# Run with logging
RUST_LOG=debug cargo test

# Run benchmarks
cargo bench
```

---

## Running the Ingestion Tool

### Initialize Neo4j Schema

```bash
# Create constraints and indexes (must run BEFORE ingestion)
cargo run --release -- init-schema
```

This creates:
- Unique constraints on Block.height, Transaction.txid, Output.outputId, etc.
- Indexes for query performance

### Start Ingestion

```bash
# Ingest blocks 0-1000 (for testing)
cargo run --release -- ingest --start-height 0 --end-height 1000

# Ingest all available blocks
cargo run --release -- ingest

# Resume from last checkpoint
cargo run --release -- ingest --resume

# Ingest with custom batch size
cargo run --release -- ingest --batch-size 100

# Ingest specific block range
cargo run --release -- ingest --start-height 100000 --end-height 200000
```

### Validate Ingested Data

```bash
# Run validation queries from VALIDATION.md
cargo run --release -- validate

# Run specific validation
cargo run --release -- validate --check balance
```

---

## Configuration Options

### Choosing Configuration Profile

Select configuration based on available system resources:

| Profile | RAM Available | CPU Cores | Use Case | Config File |
|---------|--------------|-----------|----------|-------------|
| **Constrained** | 1-2GB | 2-4 | Raspberry Pi, low-end VPS | `config/constrained.toml` |
| **Standard** | 2-4GB | 4-8 | Desktop, standard VPS | `config/standard.toml` |
| **High Performance** | 4-8GB | 8-16 | Workstation, dedicated server | `config/high-performance.toml` |
| **Ultra Performance** | 40GB+ | 8+ | High-memory server (like yours!) | `config/ultra-performance.toml` |

**Your server specs (i7-7700, 40GB RAM, 8 cores)**: Use **Ultra Performance** profile

### Command-Line Interface

```rust
// src/main.rs (CLI structure)
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bitcoin-chain-graph")]
#[command(about = "Bitcoin blockchain to Neo4j ingestion tool")]
struct Cli {
    /// Configuration file path
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize Neo4j schema (constraints and indexes)
    InitSchema,

    /// Ingest Bitcoin blocks into Neo4j
    Ingest {
        /// Start block height
        #[arg(long, default_value = "0")]
        start_height: u32,

        /// End block height (optional, ingest all if not specified)
        #[arg(long)]
        end_height: Option<u32>,

        /// Resume from last checkpoint
        #[arg(long)]
        resume: bool,

        /// Override batch size from config
        #[arg(long)]
        batch_size: Option<usize>,

        /// Override number of workers from config
        #[arg(long)]
        workers: Option<usize>,
    },

    /// Validate ingested data
    Validate {
        /// Specific validation check to run
        #[arg(long)]
        check: Option<String>,
    },

    /// Show ingestion statistics
    Stats,
}
```

### Configuration File Examples

**Standard configuration (`config/standard.toml`):**

```toml
[neo4j]
uri = "bolt://localhost:7687"
user = "neo4j"
password = "password"
database = "neo4j"
max_connections = 10

[bitcoin]
blocks_dir = "/home/user/.bitcoin/blocks"
network = "mainnet"

[memory]
utxo_cache_size = 200000
batch_max_blocks = 50
batch_max_memory_mb = 256
parser_buffer_mb = 8

[parallelism]
num_worker_threads = 4
max_concurrent_blocks = 8

[logging]
level = "info"
```

**Ultra Performance configuration (`config/ultra-performance.toml`):**

See [config/ultra-performance.toml](../../config/ultra-performance.toml) for complete configuration optimized for 40GB RAM servers.

Key settings:
- UTXO cache: 10M entries (~1.5GB)
- Batch size: 500 blocks (~4GB buffer)
- Worker threads: 8 (matching CPU cores)
- Neo4j connections: 100 (aggressive pooling)

---

## Performance Tuning

### Rust Compiler Flags

For maximum performance:

```bash
# Use native CPU instructions (AVX2, SSE4, etc.)
RUSTFLAGS="-C target-cpu=native" cargo build --release

# Link-Time Optimization (longer compile, faster binary)
RUSTFLAGS="-C lto=fat" cargo build --release

# Combine multiple optimizations
RUSTFLAGS="-C target-cpu=native -C lto=fat" cargo build --release
```

### Memory Limits

```bash
# Limit memory usage (useful for constrained environments)
ulimit -v 2097152  # 2GB virtual memory limit
cargo run --release -- ingest
```

### CPU Affinity (Linux)

```bash
# Pin to specific CPU cores for better cache locality
taskset -c 0-3 cargo run --release -- ingest
```

---

## Troubleshooting

### Build Errors

**Issue**: `bitcoin` crate compilation fails
```bash
# Solution: Update Rust to latest stable
rustup update stable
```

**Issue**: Linker errors on Linux
```bash
# Solution: Install build essentials
sudo apt-get install build-essential
```

### Runtime Errors

**Issue**: Neo4j connection refused
```bash
# Solution: Check Neo4j is running
systemctl status neo4j  # Linux
# Or check Docker container if using Docker
```

**Issue**: Out of memory during ingestion
```bash
# Solution: Reduce batch size and UTXO cache size
cargo run --release -- ingest --batch-size 10
# Edit .env: UTXO_CACHE_SIZE=50000
```

**Issue**: Slow ingestion performance
- Check Neo4j indexes are created (`cargo run -- init-schema`)
- Increase batch size for better throughput
- Increase worker threads (up to number of CPU cores)
- Use SSD for Neo4j database storage

---

## Development Tools

### Recommended VSCode Extensions

```json
{
  "recommendations": [
    "rust-lang.rust-analyzer",      // Rust language server
    "vadimcn.vscode-lldb",          // Debugger
    "serayuzgur.crates",            // Cargo.toml management
    "tamasfe.even-better-toml"      // TOML syntax
  ]
}
```

### Code Formatting

```bash
# Format code
cargo fmt

# Check formatting without changing files
cargo fmt -- --check

# Run clippy lints
cargo clippy

# Fix clippy warnings automatically
cargo clippy --fix
```

### Documentation

```bash
# Generate and open documentation
cargo doc --open

# Include private items
cargo doc --document-private-items --open
```

---

## Next Steps

1. Read [MEMORY_STRATEGY.md](MEMORY_STRATEGY.md) for memory management approach
2. Read [BINARY_PARSING.md](BINARY_PARSING.md) for block file parsing details
3. Read [NEO4J_INTEGRATION.md](NEO4J_INTEGRATION.md) for Neo4j integration patterns
4. Read [PERFORMANCE.md](PERFORMANCE.md) for optimization strategies

---

## References

- [Rust Book](https://doc.rust-lang.org/book/)
- [rust-bitcoin Documentation](https://docs.rs/bitcoin/latest/bitcoin/)
- [neo4rs Documentation](https://docs.rs/neo4rs/latest/neo4rs/)
- [Tokio Documentation](https://tokio.rs/)
- [Cargo Book](https://doc.rust-lang.org/cargo/)
