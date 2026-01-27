# Bitcoin Blockchain → Neo4j Graph

High-performance Rust implementation for ingesting Bitcoin blockchain data into Neo4j with a dual-layer graph model optimized for financial crime investigation and blockchain forensics.

---

## 🎯 Project Goals

Load Bitcoin blockchain data into Neo4j to enable:
- **Financial crime investigation**: Follow the money through transaction chains
- **Blockchain forensics**: Detailed UTXO-level analysis
- **Network analysis**: Identify transaction patterns and clusters
- **Compliance**: AML/KYC investigation support

---

## ✨ Key Features

### Dual-Layer Graph Model
- **Simplified layer**: Address → Transaction → Address with aggregated amounts for "follow the money" queries
- **Detailed layer**: Complete UTXO mechanics with Inputs, Outputs, and SPENDS relationships
- Single Transaction node shared between layers for flexibility

### Three Ingestion Modes
- **Offline** (`ingest`/`resume`): Stream from Bitcoin Core `.blk` files for bulk historical sync
- **RPC catchup** (`live`): Fetch blocks from a running Bitcoin Core node via JSON-RPC
- **ZMQ real-time** (`live`): Subscribe to new block notifications for live chain following

### Checkpoint & Resume
- Idempotent MERGE-based checkpointing for crash recovery
- Automatic resume from last processed block
- Cache pre-warming on restart for faster throughput

### High Performance
- **50-100 blocks/sec** on early chain (blocks 0-100k)
- **1-5 blocks/sec** on modern chain (blocks 700k+)
- Concurrent I/O and CPU overlapping with batch UTXO operations
- Configurable UTXO cache (15MB-1.4GB+) with Neo4j fallback

### Complete Bitcoin Support
- All address types: P2PKH, P2SH, P2WPKH, P2WSH, P2TR, P2PK
- SegWit witness data and Taproot support
- Special cases: coinbase, OP_RETURN, genesis block

---

## 🚀 Quick Start

### Prerequisites

```bash
# Install Rust (1.70+)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Start Neo4j (Docker)
docker run -d \
  -p 7687:7687 -p 7474:7474 \
  -e NEO4J_AUTH=neo4j/password \
  -v $HOME/neo4j/data:/data \
  neo4j:5

# Ensure Bitcoin Core is synced with block files at ~/.bitcoin/blocks/

# For live mode only: enable RPC and ZMQ in bitcoin.conf
# rpcuser=btcgraph
# rpcpassword=your-rpc-password
# zmqpubhashblock=tcp://127.0.0.1:28332
```

### Build and Run

```bash
# Clone repository
git clone https://github.com/mkd-neo4j/bitcoin-chain-graph.git
cd bitcoin-chain-graph

# Copy and edit configuration
cp config.example/default.toml.example config/default.toml
# Edit config/default.toml: set blocks_dir, neo4j password, etc.

# Build release binary (optimized)
cargo build --release

# Initialize Neo4j schema (constraints and indexes)
./target/release/bitcoin-chain-graph init-schema --config config/default.toml

# Ingest from .blk files (offline mode)
./target/release/bitcoin-chain-graph ingest --config config/default.toml

# Check progress
./target/release/bitcoin-chain-graph status --config config/default.toml

# Resume after interruption
./target/release/bitcoin-chain-graph resume --config config/default.toml

# Live mode: RPC catchup + ZMQ real-time streaming
./target/release/bitcoin-chain-graph live --config config/default.toml
```

### Commands

| Command | Description |
|---------|-------------|
| `init-schema` | Create Neo4j constraints, indexes, and initial checkpoint |
| `ingest` | Fresh ingestion from genesis block (offline, `.blk` files) |
| `resume` | Continue from last checkpoint (with UTXO cache pre-warming) |
| `status` | Show checkpoint progress and resume information |
| `live` | Two-phase: RPC catchup then ZMQ real-time streaming |

All commands accept `--config <FILE>` (default: `config/default.toml`).
`ingest`, `resume`, and `live` accept `--max-height <N>` to stop at a specific block.

### Configuration

Create a TOML config file (see [`config.example/`](config.example/) for profiles):

```toml
[bitcoin]
blocks_dir = "/path/to/bitcoin/blocks"

[neo4j]
uri = "bolt://localhost:7687"
user = "neo4j"
password = "your-password"

[ingestion]
batch_size = 50

[performance]
utxo_cache_memory_mb = 140

# Optional: for live mode only
[bitcoin_rpc]
url = "http://localhost:8332"
user = "btcgraph"
password = "your-rpc-password"
zmq_endpoint = "tcp://127.0.0.1:28332"
```

Configuration profiles: `low-resource`, `default`, `high-performance`, `ultra-performance`.
See [`config.example/README.md`](config.example/README.md) for details.

---

## 📊 Performance

### Benchmark Results

**Test Environment:**
- CPU: AMD Ryzen 9 5950X (16 cores)
- Memory: 32GB DDR4
- Storage: NVMe SSD (3500 MB/s read)
- Neo4j: 5.14, 16GB heap

| Block Range | Throughput | Avg Time/Block | Total Time (1000 blocks) |
|-------------|-----------|----------------|---------------------------|
| 0-10,000 (early) | 82 blocks/sec | 12ms | ~2 minutes |
| 100,000-110,000 | 25 blocks/sec | 40ms | ~7 minutes |
| 750,000-751,000 | 4.5 blocks/sec | 222ms | ~4 hours |

**Memory Usage**: Depends on UTXO cache size (configurable from 15MB to 1.4GB+). Typical: 1.2-1.8 GB resident.

---

## 📚 Documentation

Comprehensive documentation in [`docs/`](docs/):

### Architecture
- **[ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md)** - System architecture
- **[INGESTION_ARCHITECTURE.md](docs/architecture/INGESTION_ARCHITECTURE.md)** - 6-phase ingestion process
- **[REAL_TIME_ARCHITECTURE.md](docs/REAL_TIME_ARCHITECTURE.md)** - Live mode: RPC catchup + ZMQ streaming

### Bitcoin Domain Knowledge
- **[ADDRESS_DERIVATION.md](docs/bitcoin/ADDRESS_DERIVATION.md)** - Bitcoin address extraction
- **[SPECIAL_CASES.md](docs/bitcoin/SPECIAL_CASES.md)** - Edge case handling

### Neo4j Database
- **[DATA_MODEL.md](docs/neo4j/DATA_MODEL.md)** - Neo4j graph schema
- **[CYPHER_EXAMPLES.md](docs/neo4j/CYPHER_EXAMPLES.md)** - Query library
- **[VALIDATION.md](docs/neo4j/VALIDATION.md)** - Data integrity checks

### Rust Implementation
- **[rust/SETUP.md](docs/rust/SETUP.md)** - Project setup and dependencies
- **[rust/MEMORY_STRATEGY.md](docs/rust/MEMORY_STRATEGY.md)** - Memory optimization
- **[rust/BINARY_PARSING.md](docs/rust/BINARY_PARSING.md)** - Bitcoin binary parsing
- **[rust/NEO4J_INTEGRATION.md](docs/rust/NEO4J_INTEGRATION.md)** - Neo4j driver usage
- **[rust/PERFORMANCE.md](docs/rust/PERFORMANCE.md)** - Performance tuning
- **[rust/PARALLELISM.md](docs/rust/PARALLELISM.md)** - Concurrent processing
- **[rust/TESTING.md](docs/rust/TESTING.md)** - Testing strategy

**Start here**: [docs/README.md](docs/README.md)

---

## 🔍 Example Queries

### Follow the Money

```cypher
// Trace funds from Address A to Address B
MATCH path = shortestPath(
  (alice:Address {address: $aliceAddress})
  -[:PERFORMS|BENEFITS_TO*1..10]-
  (bob:Address {address: $bobAddress})
)
RETURN path
```

### Find Unspent Outputs (UTXOs)

```cypher
// Get all UTXOs for an address
MATCH (addr:Address {address: $address})<-[:LOCKED_TO]-(o:Output)
WHERE o.isSpent = false
RETURN o.outputId, o.amount
ORDER BY o.amount DESC
```

### Analyze Transaction Patterns

```cypher
// Find large transactions in a time range
MATCH (t:Transaction)
WHERE t.timestamp >= datetime($startDate)
  AND t.timestamp <= datetime($endDate)
  AND t.totalOutput > 10000000000  // amounts in satoshis (1 BTC = 100,000,000)
RETURN t.txid, t.totalOutput, t.timestamp
ORDER BY t.totalOutput DESC
LIMIT 100
```

More examples in [docs/CYPHER_EXAMPLES.md](docs/neo4j/CYPHER_EXAMPLES.md)

---

## 🏗️ Architecture

### Data Flow

```
Bitcoin Core              Rust Ingestion Tool              Neo4j
                     ┌──────────────────────────┐
  .blk files ───────>│                          │
  RPC (catchup) ────>│  Parser → Orchestrator   │──> Graph
  ZMQ (real-time) ──>│      ↕ UTXO Cache        │   (dual-layer)
                     └──────────────────────────┘
```

### Graph Model

```
Simplified Layer:
  Address -[PERFORMS {inputCount, amountSpent}]-> Transaction
  Transaction -[BENEFITS_TO {outputCount, amountReceived}]-> Address

Detailed Layer:
  Transaction -[:HAS_INPUT]-> Input -[:SPENDS]-> Output -[:LOCKED_TO]-> Address
  Transaction -[:HAS_OUTPUT]-> Output

Block Chain:
  Block -[:NEXT_BLOCK]-> Block
  Transaction -[:INCLUDED_IN]-> Block
```

See [docs/DATA_MODEL.md](docs/neo4j/DATA_MODEL.md) for complete schema.

---

## 🛠️ Development

### Build

```bash
# Debug build (fast compile, slow runtime)
cargo build

# Release build (slow compile, optimized runtime)
cargo build --release

# With native CPU optimizations
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

### Test

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

## 📈 Roadmap

- [x] Core ingestion pipeline (6 phases)
- [x] Complete address support (P2PKH → P2TR)
- [x] Memory-efficient streaming parser
- [x] UTXO cache with LRU eviction
- [x] Bulk insert optimization
- [x] Comprehensive documentation
- [x] Parallel block processing
- [x] Resume from checkpoint
- [x] Real-time ingestion (RPC catchup + ZMQ streaming)
- [ ] GraphQL API for queries
- [ ] Neo4j Bloom visualizations
- [ ] Testnet and regtest support
- [ ] Address clustering algorithms

---

## 🤝 Contributing

Contributions are welcome! Please:

1. Read the documentation in [`docs/`](docs/)
2. Check existing issues/PRs
3. Follow Rust conventions (rustfmt, clippy)
4. Add tests for new features
5. Update documentation

---

## 📜 License

Apache-2.0

---

## 🙏 Acknowledgments

- [rust-bitcoin](https://github.com/rust-bitcoin/rust-bitcoin) - Bitcoin protocol library
- [neo4rs](https://github.com/neo4j-labs/neo4rs) - Neo4j async driver
- [Neo4j](https://neo4j.com/) - Graph database platform
- Bitcoin Core developers

---

## 📞 Support

- **Documentation**: [docs/README.md](docs/README.md)
- **Issues**: [GitHub Issues](https://github.com/mkd-neo4j/bitcoin-chain-graph/issues)
- **Discussions**: [GitHub Discussions](https://github.com/mkd-neo4j/bitcoin-chain-graph/discussions)

---

## 📊 System Requirements

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

**Built with ❤️ and ⚡ Rust**

---

*Last updated: January 2026*
