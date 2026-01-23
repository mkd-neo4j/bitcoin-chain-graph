# Bitcoin Blockchain → Neo4j Documentation

Complete documentation for ingesting Bitcoin blockchain data into Neo4j with a dual-layer graph model.

---

## Overview

This project provides a high-performance system for loading Bitcoin blockchain data from raw block files into a Neo4j graph database, optimized for both financial crime investigation and blockchain forensics.

**Key Features:**
- **Dual-layer graph model**: Simplified "follow the money" layer + detailed UTXO mechanics
- **Memory-efficient**: <2GB resident memory with streaming parser and LRU caching
- **High throughput**: 50-100 blocks/sec (early chain), 1-5 blocks/sec (modern chain)
- **Rust implementation**: Zero-cost abstractions, no GC overhead
- **Complete address support**: P2PKH, P2SH, P2WPKH, P2WSH, P2TR, P2PK

---

## Documentation Structure

### General Documentation (Language-Agnostic)

Core concepts and data model applicable to any implementation:

1. **[DATA_MODEL.md](DATA_MODEL.md)** - Neo4j graph schema specification
   - Dual-layer model (simplified + detailed)
   - Node definitions (Block, Transaction, Output, Input, Address)
   - Relationship definitions (PERFORMS, BENEFITS_TO, SPENDS, LOCKED_TO, etc.)
   - Blockchain data mapping

2. **[INGESTION_ARCHITECTURE.md](INGESTION_ARCHITECTURE.md)** - 6-phase ingestion process
   - Block-by-block sequential processing
   - Phase dependencies and ordering
   - Bitcoin Core `.blk` file structure
   - Checkpoint and resume strategy

3. **[ADDRESS_DERIVATION.md](ADDRESS_DERIVATION.md)** - Bitcoin address extraction
   - Script type detection (P2PKH, P2SH, P2WPKH, P2WSH, P2TR, P2PK, OP_RETURN)
   - Base58Check encoding (legacy addresses)
   - Bech32/Bech32m encoding (SegWit addresses)
   - Edge cases and testing

4. **[SPECIAL_CASES.md](SPECIAL_CASES.md)** - Edge case handling
   - Coinbase transactions
   - OP_RETURN outputs (NULL_DATA)
   - Genesis block
   - P2PK (obsolete format)
   - SegWit witness data
   - Bare multisig and non-standard scripts

5. **[CYPHER_EXAMPLES.md](CYPHER_EXAMPLES.md)** - Complete Cypher query library
   - Schema setup (constraints, indexes)
   - Ingestion queries for all 6 phases
   - "Follow the money" queries
   - UTXO layer queries
   - Analysis and validation queries

6. **[VALIDATION.md](VALIDATION.md)** - Data integrity validation
   - Transaction balance checks
   - Relationship integrity
   - UTXO consistency
   - Block chain validation
   - Summary validation report

---

### Rust Implementation Documentation

Implementation-specific guides for high-performance Rust ingestion:

📁 **[rust/](rust/)** - Rust-specific implementation docs

1. **[rust/SETUP.md](rust/SETUP.md)** - Project setup and configuration
   - Cargo.toml dependencies (bitcoin, neo4rs, tokio, rayon)
   - Project structure
   - Build configuration (release optimizations)
   - CLI interface
   - Environment configuration

2. **[rust/MEMORY_STRATEGY.md](rust/MEMORY_STRATEGY.md)** - Memory-efficient implementation
   - Streaming block file parser (no full file load)
   - LRU UTXO cache strategy
   - Bounded batch accumulator
   - Zero-copy parsing patterns
   - Memory profiling and monitoring
   - Target: <2GB resident memory

3. **[rust/BINARY_PARSING.md](rust/BINARY_PARSING.md)** - Bitcoin binary format parsing
   - Using `bitcoin` crate for deserialization
   - Block file format (.blk magic bytes, size headers)
   - Streaming parser implementation
   - Address derivation with `bitcoin::Address`
   - Endianness and varint handling
   - Test vectors

4. **[rust/NEO4J_INTEGRATION.md](rust/NEO4J_INTEGRATION.md)** - Neo4j driver usage
   - `neo4rs` async driver
   - Connection pooling
   - Bulk insert with UNWIND patterns
   - Transaction management
   - Error handling and retry logic
   - Query execution patterns

5. **[rust/PERFORMANCE.md](rust/PERFORMANCE.md)** - Optimization strategies
   - Bulk insert patterns (500x speedup)
   - UTXO cache optimization
   - Connection pooling
   - Index and constraint optimization
   - Profiling tools (flamegraph, heaptrack)
   - Benchmarking with Criterion
   - Real-world performance results

6. **[rust/PARALLELISM.md](rust/PARALLELISM.md)** - Concurrent processing
   - Async I/O with Tokio
   - Data parallelism with Rayon
   - Multi-stage pipelines
   - Worker pool patterns
   - Backpressure handling
   - Synchronization primitives

7. **[rust/TESTING.md](rust/TESTING.md)** - Testing strategy
   - Unit tests (parsers, address derivation, UTXO cache)
   - Integration tests (Neo4j ingestion)
   - Property-based tests (balance invariants)
   - Validation tests
   - Performance benchmarks
   - CI/CD setup

---

## Quick Start

### Prerequisites

1. **Rust** (1.70+)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Neo4j** (5.x)
   ```bash
   # Docker
   docker run -p 7687:7687 -p 7474:7474 \
     -e NEO4J_AUTH=neo4j/password \
     neo4j:5
   ```

3. **Bitcoin Core Block Files**
   - Location: `~/.bitcoin/blocks/`
   - Requires synced Bitcoin Core node

### Build and Run

```bash
# Clone repository
git clone https://github.com/yourusername/bitcoin-chain-graph.git
cd bitcoin-chain-graph

# Build release binary
cargo build --release

# Initialize Neo4j schema
./target/release/bitcoin-chain-graph init-schema

# Ingest blocks
./target/release/bitcoin-chain-graph ingest --start-height 0 --end-height 1000

# Validate ingested data
./target/release/bitcoin-chain-graph validate
```

---

## Documentation Reading Order

### For First-Time Users

1. Start with [DATA_MODEL.md](DATA_MODEL.md) to understand the graph structure
2. Read [INGESTION_ARCHITECTURE.md](INGESTION_ARCHITECTURE.md) for the ingestion process
3. Review [SPECIAL_CASES.md](SPECIAL_CASES.md) for edge cases
4. Skim [CYPHER_EXAMPLES.md](CYPHER_EXAMPLES.md) for query patterns

### For Rust Developers

1. Read [rust/SETUP.md](rust/SETUP.md) for project configuration
2. Study [rust/BINARY_PARSING.md](rust/BINARY_PARSING.md) for Bitcoin data parsing
3. Review [rust/MEMORY_STRATEGY.md](rust/MEMORY_STRATEGY.md) for memory optimization
4. Read [rust/NEO4J_INTEGRATION.md](rust/NEO4J_INTEGRATION.md) for database integration
5. Implement and test following [rust/TESTING.md](rust/TESTING.md)

### For Performance Tuning

1. [rust/PERFORMANCE.md](rust/PERFORMANCE.md) - Optimization strategies
2. [rust/PARALLELISM.md](rust/PARALLELISM.md) - Concurrent processing
3. [rust/MEMORY_STRATEGY.md](rust/MEMORY_STRATEGY.md) - Memory profiling

---

## System Requirements

### Minimum (Testing)
- **CPU**: 2 cores
- **Memory**: 4GB RAM
- **Storage**: 50GB SSD
- **Network**: Local Neo4j instance
- **Use case**: First 100k blocks

### Recommended (Full Chain)
- **CPU**: 8+ cores
- **Memory**: 16GB RAM (8GB for Neo4j, 2GB for ingestion, 6GB OS)
- **Storage**: 1TB NVMe SSD (500GB for Bitcoin blocks, 500GB for Neo4j)
- **Network**: Local Neo4j instance or low-latency connection
- **Use case**: Full blockchain (850k+ blocks)

### High Performance
- **CPU**: 16+ cores (Ryzen 9 / Threadripper / Xeon)
- **Memory**: 32GB+ RAM
- **Storage**: 2TB+ NVMe SSD (RAID 0 for Neo4j)
- **Network**: 10Gbps local connection
- **Use case**: Research, high-throughput analysis

---

## Performance Expectations

| Scenario | Throughput | Time to Ingest | Notes |
|----------|-----------|----------------|-------|
| **Early chain (0-100k)** | 50-100 blocks/sec | ~20 minutes | Small blocks, simple transactions |
| **Middle chain (100k-500k)** | 10-30 blocks/sec | ~4 hours | Growing block sizes |
| **Modern chain (500k-850k)** | 1-5 blocks/sec | ~24-96 hours | Large blocks (1-4MB), complex transactions |
| **Full chain (0-850k)** | Variable | ~2-5 days | End-to-end ingestion |

**Factors affecting performance:**
- Neo4j heap size (larger = faster)
- SSD vs HDD (3-5x difference)
- Batch size (larger = faster, more memory)
- CPU core count (for parallel processing)

---

## Troubleshooting

### Common Issues

**Slow ingestion (<1 block/sec)**
- Check if Neo4j indexes exist (`SHOW INDEXES`)
- Increase batch size
- Use SSD for Neo4j storage
- Increase Neo4j heap size

**Out of memory**
- Reduce batch size
- Reduce UTXO cache size
- Close other applications

**Connection errors**
- Verify Neo4j is running (`systemctl status neo4j`)
- Check Neo4j URI in config
- Verify network connectivity

**Data integrity failures**
- Re-run validation queries from [VALIDATION.md](VALIDATION.md)
- Check for blocks processed out-of-order
- Verify all 6 ingestion phases completed

---

## Contributing

Contributions are welcome! Areas for improvement:
- Additional query examples
- Performance optimizations
- Support for other Bitcoin networks (testnet, regtest)
- Additional validation queries
- Documentation improvements

---

## References

### Bitcoin Protocol
- [Bitcoin Developer Reference](https://developer.bitcoin.org/reference/)
- [Bitcoin Wiki](https://en.bitcoin.it/)
- [Bitcoin Improvement Proposals (BIPs)](https://github.com/bitcoin/bips)

### Neo4j
- [Neo4j Documentation](https://neo4j.com/docs/)
- [Cypher Manual](https://neo4j.com/docs/cypher-manual/current/)
- [Neo4j Graph Academy](https://graphacademy.neo4j.com/)

### Rust
- [Rust Book](https://doc.rust-lang.org/book/)
- [rust-bitcoin](https://docs.rs/bitcoin/latest/bitcoin/)
- [neo4rs](https://docs.rs/neo4rs/latest/neo4rs/)
- [Tokio](https://tokio.rs/)

---

## License

[Add your license here]

---

## Support

- **Issues**: [GitHub Issues](https://github.com/yourusername/bitcoin-chain-graph/issues)
- **Discussions**: [GitHub Discussions](https://github.com/yourusername/bitcoin-chain-graph/discussions)
- **Documentation**: This repository

---

Last updated: January 2025
