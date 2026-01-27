# Getting Started

Everything you need to install, configure, and run bitcoin-chain-graph.

---

## System Requirements

### Minimum (Testing)
- 2 CPU cores
- 4GB RAM
- 50GB SSD
- Use case: First 100k blocks

### Recommended (Full Chain)
- 8+ CPU cores
- 16GB RAM
- 1TB NVMe SSD
- Use case: Full blockchain

### High Performance
- 16+ CPU cores
- 32GB+ RAM
- 2TB+ NVMe SSD (RAID 0)
- Use case: Research, high-throughput analysis

**Live mode** additionally requires a running Bitcoin Core node with RPC and ZMQ enabled.

---

## Prerequisites

### Rust (1.70+)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Neo4j (5.x)

```bash
docker run -d \
  -p 7687:7687 -p 7474:7474 \
  -e NEO4J_AUTH=neo4j/password \
  -v $HOME/neo4j/data:/data \
  neo4j:5
```

### Bitcoin Core Block Files

Ensure Bitcoin Core is synced with block files available at:
- **Linux**: `~/.bitcoin/blocks/`
- **macOS**: `~/Library/Application Support/Bitcoin/blocks/`

### For Live Mode Only

Enable RPC and ZMQ in your `bitcoin.conf`:

```ini
rpcuser=btcgraph
rpcpassword=your-rpc-password
zmqpubhashblock=tcp://127.0.0.1:28332
```

---

## Installation

```bash
# Clone repository
git clone https://github.com/mkd-neo4j/bitcoin-chain-graph.git
cd bitcoin-chain-graph

# Copy and edit configuration
cp config.example/config.toml.example config/default.toml
# Edit config/default.toml: set blocks_dir and neo4j password at minimum

# Build release binary (optimized)
cargo build --release
```

For maximum performance on your hardware:

```bash
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

---

## Configuration

Copy and customise the example configuration:

```bash
cp config.example/config.toml.example config/default.toml
```

Edit the key sections:

```toml
[bitcoin]
blocks_dir = "/path/to/bitcoin/blocks"

[neo4j]
uri = "bolt://localhost:7687"
user = "neo4j"
password = "your-password"

[ingestion]
batch_size = 5000

[performance]
utxo_cache_memory_mb = 140

# Optional: for live mode, uncomment [bitcoin_rpc] section in the config
```

The example config is tuned for a mid-range server (4-8 cores, 16-32GB RAM). Every setting includes inline comments explaining how to scale up or down for your hardware. See [`config.example/README.md`](config.example/README.md) for a full scaling guide.

---

## First Run

### Step 1: Initialize the Schema

Create Neo4j constraints, indexes, and the initial checkpoint:

```bash
./target/release/bitcoin-chain-graph init-schema --config config/default.toml
```

### Step 2: Ingest Historical Blocks

Start ingesting from genesis block using `.blk` files:

```bash
./target/release/bitcoin-chain-graph ingest --config config/default.toml
```

You can limit to a specific height for testing:

```bash
./target/release/bitcoin-chain-graph ingest --config config/default.toml --max-height 1000
```

### Step 3: Check Progress

View the current checkpoint status:

```bash
./target/release/bitcoin-chain-graph status --config config/default.toml
```

### Step 4: Resume After Interruption

If ingestion is interrupted (Ctrl+C, crash, etc.), resume from the last checkpoint. The UTXO cache is pre-warmed automatically for faster throughput:

```bash
./target/release/bitcoin-chain-graph resume --config config/default.toml
```

### Step 5: Live Mode (Optional)

Once caught up, switch to live mode for real-time block streaming. This runs in two phases:

1. **RPC catchup**: Fetches any blocks between your checkpoint and the chain tip via Bitcoin Core JSON-RPC
2. **ZMQ streaming**: Subscribes to new block notifications and processes them as they arrive

```bash
./target/release/bitcoin-chain-graph live --config config/default.toml
```

Requires `[bitcoin_rpc]` section in your config and a running Bitcoin Core node.

---

## CLI Reference

| Command | Description |
|---------|-------------|
| `init-schema` | Create Neo4j constraints, indexes, and initial checkpoint |
| `ingest` | Fresh ingestion from genesis block (offline, `.blk` files) |
| `resume` | Continue from last checkpoint (with UTXO cache pre-warming) |
| `status` | Show checkpoint progress and resume information |
| `live` | Two-phase: RPC catchup then ZMQ real-time streaming |

**Global flags:**
- `--config <FILE>` — Path to config file (default: `config/default.toml`)

**Ingestion flags** (available on `ingest`, `resume`, `live`):
- `--max-height <N>` — Stop at a specific block height

---

## Performance

### Benchmark Results

**Test Environment:** AMD Ryzen 9 5950X (16 cores), 32GB DDR4, NVMe SSD, Neo4j 5.14 (16GB heap)

| Block Range | Throughput | Avg Time/Block | Total Time (1000 blocks) |
|-------------|-----------|----------------|---------------------------|
| 0-10,000 (early) | 82 blocks/sec | 12ms | ~2 minutes |
| 100,000-110,000 | 25 blocks/sec | 40ms | ~7 minutes |
| 750,000-751,000 | 4.5 blocks/sec | 222ms | ~4 hours |

**Memory Usage**: Depends on UTXO cache size (configurable from 15MB to 1.4GB+). Typical: 1.2-1.8 GB resident.

---

## Development

### Build Variants

```bash
# Debug build (fast compile, slow runtime)
cargo build

# Release build (slow compile, optimized runtime)
cargo build --release

# With native CPU optimizations
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test module
cargo test parser::tests

# Run integration tests (requires Neo4j)
cargo test --test integration_tests

# Run benchmarks
cargo bench
```

### Code Quality

```bash
# Format code
cargo fmt

# Run clippy lints
cargo clippy

# Generate documentation
cargo doc --open
```

---

## Next Steps

- Explore the [Data Model](docs/neo4j/DATA_MODEL.md) to understand the dual-layer graph schema
- Browse the [Cypher Query Library](docs/neo4j/CYPHER_EXAMPLES.md) for investigation and analysis patterns
- Read the [Architecture docs](docs/architecture/ARCHITECTURE.md) to understand the 3-layer system design
- See [Real-Time Architecture](docs/architecture/REAL_TIME_ARCHITECTURE.md) for live mode internals
- See [`config.example/README.md`](config.example/README.md) for advanced configuration tuning
