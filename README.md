# Bitcoin Blockchain → Neo4j Graph

High-performance Rust implementation for ingesting Bitcoin blockchain data into Neo4j with a dual-layer graph model optimized for financial crime investigation and blockchain forensics.

---

## 🎯 Project Goals

Load Bitcoin Core raw block files (`.blk`) into Neo4j to enable:
- **Financial crime investigation**: Follow the money through transaction chains
- **Blockchain forensics**: Detailed UTXO-level analysis
- **Network analysis**: Identify transaction patterns and clusters
- **Compliance**: AML/KYC investigation support

---

## ✨ Key Features

### Dual-Layer Graph Model
- **Simplified layer**: Direct Address → Transaction → Address paths for "follow the money" queries
- **Detailed layer**: Complete UTXO mechanics with Inputs, Outputs, and SPENDS relationships
- Single Transaction node shared between layers for flexibility

### High Performance
- **50-100 blocks/sec** on early chain (blocks 0-100k)
- **1-5 blocks/sec** on modern chain (blocks 700k+)
- Memory-efficient: <2GB resident memory
- Multi-core parallelism with Rust's tokio + rayon

### Complete Bitcoin Support
- All address types: P2PKH, P2SH, P2WPKH, P2WSH, P2TR, P2PK
- SegWit witness data
- Taproot support
- Special cases: coinbase, OP_RETURN, genesis block

### Rust Implementation
- Zero-cost abstractions
- No garbage collection overhead
- Memory safety without runtime cost
- Excellent concurrency primitives

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
```

### Build and Run

```bash
# Clone repository
git clone https://github.com/yourusername/bitcoin-chain-graph.git
cd bitcoin-chain-graph

# Build release binary (optimized)
cargo build --release

# Initialize Neo4j schema (constraints and indexes)
./target/release/bitcoin-chain-graph init-schema

# Ingest first 1000 blocks
./target/release/bitcoin-chain-graph ingest \
  --start-height 0 \
  --end-height 1000 \
  --batch-size 50

# Validate ingested data
./target/release/bitcoin-chain-graph validate

# View ingestion statistics
./target/release/bitcoin-chain-graph stats
```

### Configuration

Create `.env` file:

```bash
# Neo4j connection
NEO4J_URI=bolt://localhost:7687
NEO4J_USER=neo4j
NEO4J_PASSWORD=password
NEO4J_DATABASE=neo4j

# Bitcoin block files
BITCOIN_BLOCKS_DIR=/home/user/.bitcoin/blocks

# Performance tuning
BATCH_SIZE=50                 # Blocks per Neo4j transaction
UTXO_CACHE_SIZE=200000       # Recent outputs to cache in memory
WORKER_THREADS=4              # Parallel workers
LOG_LEVEL=info
```

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

**Memory Usage**: 1.2-1.8 GB resident (with 200k UTXO cache)

---

## 📚 Documentation

Comprehensive documentation in [`docs/`](docs/):

### General (Language-Agnostic)
- **[DATA_MODEL.md](docs/DATA_MODEL.md)** - Neo4j graph schema
- **[INGESTION_ARCHITECTURE.md](docs/INGESTION_ARCHITECTURE.md)** - 6-phase ingestion process
- **[ADDRESS_DERIVATION.md](docs/ADDRESS_DERIVATION.md)** - Bitcoin address extraction
- **[SPECIAL_CASES.md](docs/SPECIAL_CASES.md)** - Edge case handling
- **[CYPHER_EXAMPLES.md](docs/CYPHER_EXAMPLES.md)** - Query library
- **[VALIDATION.md](docs/VALIDATION.md)** - Data integrity checks

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
  AND t.totalOutput > 100.0
RETURN t.txid, t.totalOutput, t.timestamp
ORDER BY t.totalOutput DESC
LIMIT 100
```

More examples in [docs/CYPHER_EXAMPLES.md](docs/CYPHER_EXAMPLES.md)

---

## 🏗️ Architecture

### Data Flow

```
Bitcoin Core         Rust Ingestion Tool           Neo4j Database
    .blk files  →   Parser → Processor   →   Graph (dual-layer)
                         ↓
                    UTXO Cache (LRU)
                         ↓
                    Batch Builder
                         ↓
                    Bulk Insert (UNWIND)
```

### Graph Model

```
Simplified Layer:
  Address --PERFORMS--> Transaction --BENEFITS_TO--> Address

Detailed Layer:
  Input --SPENDS--> Output --LOCKED_TO--> Address
                    ↓
  Transaction --HAS_INPUT--> Input
  Transaction --HAS_OUTPUT--> Output
```

See [docs/DATA_MODEL.md](docs/DATA_MODEL.md) for complete schema.

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
- [ ] Parallel block processing
- [ ] Resume from checkpoint
- [ ] Real-time ingestion (monitor new blocks)
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

[Add your license here - e.g., MIT, Apache 2.0]

---

## 🙏 Acknowledgments

- [rust-bitcoin](https://github.com/rust-bitcoin/rust-bitcoin) - Bitcoin protocol library
- [neo4rs](https://github.com/neo4j-labs/neo4rs) - Neo4j async driver
- [Neo4j](https://neo4j.com/) - Graph database platform
- Bitcoin Core developers

---

## 📞 Support

- **Documentation**: [docs/README.md](docs/README.md)
- **Issues**: [GitHub Issues](https://github.com/yourusername/bitcoin-chain-graph/issues)
- **Discussions**: [GitHub Discussions](https://github.com/yourusername/bitcoin-chain-graph/discussions)

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

---

**Built with ❤️ and ⚡ Rust**

---

*Last updated: January 2025*
