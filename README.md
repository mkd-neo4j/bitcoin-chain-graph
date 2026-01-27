# Bitcoin Blockchain → Neo4j Graph

High-performance Rust tool for ingesting Bitcoin blockchain data into a dual-layer Neo4j graph, purpose-built for financial crime investigation and blockchain forensics.

> **[Getting Started Guide](GETTING_STARTED.md)** — Install, configure, and run your first ingestion.

---

## Why Graph Databases for Bitcoin Analysis

Bitcoin is a graph. Addresses send value to other addresses through transactions, forming a massive directed network of money flow. Storing this data in a relational database forces you to JOIN through what is inherently a graph structure, recursive CTEs, self-joins, and multi-table traversals just to answer "where did this money go?"

Neo4j stores relationships as first-class citizens. Traversing from one address through a chain of transactions to another is a native operation, not an expensive query plan. What requires recursive SQL becomes a single variable-length path match in Cypher:

```cypher
MATCH path = shortestPath(
  (source:Address {address: $from})-[:PERFORMS|BENEFITS_TO*1..10]-(target:Address {address: $to})
)
RETURN path
```

This makes Neo4j the natural choice for:
- **Financial crime investigation** — Follow the money across any number of hops
- **Blockchain forensics** — Trace exact inputs and outputs at the UTXO level
- **AML/KYC compliance** — Map transaction networks around flagged addresses
- **Pattern analysis** — Detect mixing, tumbling, and structuring behavior

---

## The Dual-Layer Model

This project implements two complementary graph layers on top of the same data, connected through shared Transaction nodes.

### Simplified Layer: Follow the Money

```
Alice ──PERFORMS──► Transaction ──BENEFITS_TO──► Bob
                                ──BENEFITS_TO──► Alice (change)
```

The simplified layer models direct value flow: **Address → Transaction → Address**. PERFORMS and BENEFITS_TO relationships carry aggregated amounts (`amountSpent`, `amountReceived`) and counts (`inputCount`, `outputCount`).

This layer is designed for investigators. "Who sent money to whom?" is a single hop. "Trace funds across 10 intermediaries" is a variable-length path. The graph structure maps directly to the questions analysts ask.

### Detailed Layer: Full Forensic Granularity

```
Previous Output ──LOCKED_TO──► Alice
        │
     SPENDS
        │
        ▼
     Input 0 ──HAS_INPUT──► Transaction ──HAS_OUTPUT──► Output 0 ──LOCKED_TO──► Bob
                                         ──HAS_OUTPUT──► Output 1 ──LOCKED_TO──► Alice
```

The detailed layer preserves raw UTXO mechanics: which specific outputs were consumed, which new outputs were created, and the cryptographic proof data (scriptSig, witness) for each. This is essential when you need to verify exact transaction mechanics, calculate precise balances, or trace specific UTXOs through their spend chains.

### Why Both?

The simplified layer answers broad investigative questions quickly. The detailed layer provides the forensic evidence to back them up. Both share the same Transaction nodes, so you can start with a high-level money flow query and drill down into UTXO-level detail without leaving the graph.

---

## What You Can Do

### Trace Funds Between Addresses

*"Did money flow from this address to that one?"*

```cypher
MATCH path = shortestPath(
  (alice:Address {address: $aliceAddress})
  -[:PERFORMS|BENEFITS_TO*1..10]-
  (bob:Address {address: $bobAddress})
)
RETURN path
```

### Map an Address's Network

*"Who did this address transact with?"*

```cypher
MATCH (addr:Address {address: $address})-[:PERFORMS]->(t:Transaction)-[:BENEFITS_TO]->(recipient:Address)
WHERE recipient <> addr
RETURN DISTINCT recipient.address AS sentTo,
       count(t) AS transactionCount
ORDER BY transactionCount DESC
LIMIT 50
```

### Find Unspent Outputs (UTXOs)

*"What funds does this address currently hold?"*

```cypher
MATCH (addr:Address {address: $address})<-[:LOCKED_TO]-(o:Output)
WHERE o.isSpent = false
RETURN o.outputId, o.amount
ORDER BY o.amount DESC
```

### Detect Mixing Patterns

*"Which transactions show signs of tumbling or mixing?"*

```cypher
MATCH (t:Transaction)
MATCH (t)<-[:HAS_INPUT]-(i:Input)
MATCH (t)-[:HAS_OUTPUT]->(o:Output)
WITH t, count(DISTINCT i) AS inputCount, count(DISTINCT o) AS outputCount
WHERE inputCount >= 10 AND outputCount >= 10
RETURN t.txid, inputCount, outputCount, t.timestamp
ORDER BY inputCount DESC
LIMIT 100
```

### Analyze Large Transactions

*"What were the largest movements in a given time range?"*

```cypher
MATCH (t:Transaction)
WHERE t.timestamp >= datetime($startDate)
  AND t.timestamp <= datetime($endDate)
  AND t.totalOutput > 10000000000  // amounts in satoshis (1 BTC = 100,000,000)
RETURN t.txid, t.totalOutput, t.timestamp
ORDER BY t.totalOutput DESC
LIMIT 100
```

Full query library: [docs/neo4j/CYPHER_EXAMPLES.md](docs/neo4j/CYPHER_EXAMPLES.md)

---

## Key Features

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

## Architecture

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

See [docs/neo4j/DATA_MODEL.md](docs/neo4j/DATA_MODEL.md) for the complete schema.

---

## Documentation

- **[Getting Started](GETTING_STARTED.md)** — Prerequisites, installation, configuration, first run
- **[REAL_TIME_ARCHITECTURE.md](docs/architecture/REAL_TIME_ARCHITECTURE.md)** — Live mode: RPC catchup + ZMQ streaming

### Architecture
- [ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md) — 3-layer system design
- [INGESTION_ARCHITECTURE.md](docs/architecture/INGESTION_ARCHITECTURE.md) — 6-phase ingestion process

### Bitcoin Domain Knowledge
- [ADDRESS_DERIVATION.md](docs/bitcoin/ADDRESS_DERIVATION.md) — Bitcoin address extraction
- [SPECIAL_CASES.md](docs/bitcoin/SPECIAL_CASES.md) — Edge case handling

### Neo4j Database
- [DATA_MODEL.md](docs/neo4j/DATA_MODEL.md) — Graph schema specification
- [CYPHER_EXAMPLES.md](docs/neo4j/CYPHER_EXAMPLES.md) — Query library
- [VALIDATION.md](docs/neo4j/VALIDATION.md) — Data integrity checks

### Rust Implementation
- [rust/SETUP.md](docs/rust/SETUP.md) — Project setup and dependencies
- [rust/MEMORY_STRATEGY.md](docs/rust/MEMORY_STRATEGY.md) — Memory optimization
- [rust/BINARY_PARSING.md](docs/rust/BINARY_PARSING.md) — Bitcoin binary parsing
- [rust/NEO4J_INTEGRATION.md](docs/rust/NEO4J_INTEGRATION.md) — Neo4j driver usage
- [rust/PERFORMANCE.md](docs/rust/PERFORMANCE.md) — Performance tuning
- [rust/PARALLELISM.md](docs/rust/PARALLELISM.md) — Concurrent processing
- [rust/TESTING.md](docs/rust/TESTING.md) — Testing strategy

**Comprehensive index**: [docs/README.md](docs/README.md)

---

## Roadmap

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

## Contributing

Contributions are welcome! Please:

1. Read the documentation in [`docs/`](docs/)
2. Check existing issues/PRs
3. Follow Rust conventions (rustfmt, clippy)
4. Add tests for new features
5. Update documentation

---

## License

Apache-2.0

---

## Acknowledgments

- [rust-bitcoin](https://github.com/rust-bitcoin/rust-bitcoin) — Bitcoin protocol library
- [neo4rs](https://github.com/neo4j-labs/neo4rs) — Neo4j async driver
- [Neo4j](https://neo4j.com/) — Graph database platform
- Bitcoin Core developers

---

## Support

- **Getting Started**: [GETTING_STARTED.md](GETTING_STARTED.md)
- **Documentation**: [docs/README.md](docs/README.md)
- **Issues**: [GitHub Issues](https://github.com/mkd-neo4j/bitcoin-chain-graph/issues)
- **Discussions**: [GitHub Discussions](https://github.com/mkd-neo4j/bitcoin-chain-graph/discussions)

---

*Last updated: January 2026*
