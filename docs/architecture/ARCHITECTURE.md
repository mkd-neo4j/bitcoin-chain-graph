# System Architecture

High-level architecture for Bitcoin blockchain ingestion into Neo4j with emphasis on **clean separation of concerns** and **isolated Neo4j write operations**.

---

## Design Goals

1. **Isolation**: Neo4j write logic completely isolated for easy updates
2. **Testability**: Test business logic without requiring a database
3. **Maintainability**: Changes to queries don't ripple through codebase
4. **Flexibility**: Easy to swap database implementations
5. **Performance**: Zero-copy parsing, efficient memory usage

---

## System Overview

### 3-Layer Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         CLI / Main                               │
│                      (Orchestration)                             │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Layer 1: Parser                              │
│                   (Bitcoin Data Reading)                         │
│                                                                   │
│  • Streams .blk files                                            │
│  • Deserializes blocks/transactions                              │
│  • Derives addresses from scripts                                │
│  • Zero knowledge of Neo4j                                       │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼ Domain Models (Block, Transaction, etc.)
┌─────────────────────────────────────────────────────────────────┐
│                    Layer 2: Domain Logic                         │
│                    (Business Logic)                              │
│                                                                   │
│  • UTXO cache management                                         │
│  • 6-phase ingestion orchestration                               │
│  • Transaction amount calculation                                │
│  • Checkpointing                                                 │
│  • Zero knowledge of Neo4j                                       │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼ GraphWriter trait calls
┌─────────────────────────────────────────────────────────────────┐
│                  Layer 3: Writer (Database)                      │
│                 (Neo4j Write Operations)                         │
│                                                                   │
│  • GraphWriter trait (abstraction)                               │
│  • Neo4j implementation (neo4rs)                                 │
│  • Mock implementation (testing)                                 │
│  • ALL Cypher queries centralized                                │
│  • Only layer that imports neo4rs                                │
└─────────────────────────────────────────────────────────────────┘
```

### Data Flow

```
.blk files → Parser → Domain Models → GraphWriter Trait → Neo4j
                                                        └→ Mock (tests)
```

---

## Module Boundaries

### Layer 1: Parser (Zero Database Knowledge)

**Location**: `src/parser/`

**Responsibilities:**
- Read Bitcoin Core `.blk` files as streams
- Deserialize binary data into Bitcoin protocol types
- Derive addresses from scriptPubKey (pure function)
- Detect script types (P2PKH, P2SH, P2WPKH, P2WSH, P2TR, P2PK)
- Provide block loading via LevelDB index (offline) or RPC (live)
- Listen for real-time block notifications via ZMQ

**Key Principle**: Parser knows ONLY Bitcoin protocol, nothing about Neo4j or any database

**Dependencies**: `bitcoin`, `reqwest`, `zeromq`, `serde_json`, `leveldb`

**Module files:**
```
src/parser/
├── mod.rs                  # Re-exports
├── address.rs              # extract_address(), AddressInfo, ScriptType
├── block_file.rs           # BlockFileReader (streaming .blk file reader)
├── block_index.rs          # BlockIndexEntry, BlockIndexReader (LevelDB)
├── single_block_loader.rs  # SingleBlockLoader (lazy O(1) loader with pre-warming)
├── rpc_provider.rs         # RpcBlockProvider (Bitcoin Core RPC, live mode)
└── zmq_listener.rs         # ZmqBlockListener (real-time notifications, live mode)
```

**Example**:
```rust
// parser/address.rs
pub fn extract_address(script: &Script, network: Network) -> Option<AddressInfo> {
    // Pure function: Script → AddressInfo (address string + script type)
    // No side effects, no database calls
}

// parser/single_block_loader.rs
pub struct SingleBlockLoader { /* ... */ }

impl SingleBlockLoader {
    pub fn load_block(&self, height: u32) -> Result<(Block, String, Option<u64>)> {
        // O(1) lookup via height → file offset HashMap
        // Returns (Block, file_name, file_offset)
    }
}
```

---

### Layer 2: Domain Logic (Zero Database Knowledge)

**Location**: `src/domain/`

**Responsibilities:**
- Orchestrate 6-phase ingestion process (see [INGESTION_ARCHITECTURE.md](INGESTION_ARCHITECTURE.md))
- Manage UTXO cache (sharded LRU cache with trait-based Neo4j fallback)
- Calculate transaction amounts in Rust (totalInput, totalOutput, fee)
- Pre-aggregate simplified layer data (PERFORMS, BENEFITS_TO)
- Checkpoint management for resumability
- Convert parser types to domain models

**Key Principle**: Domain logic calls GraphWriter trait methods but never imports `neo4rs`

**Dependencies**: Parser layer, GraphWriter trait (abstraction only)

**Module files:**
```
src/domain/
├── mod.rs            # Re-exports
├── models.rs         # BlockData, TransactionData, OutputData, InputData,
│                     #   CheckpointData, PerformsData, BenefitsToData
├── ingestion.rs      # IngestionOrchestrator<W> (6-phase pipeline)
├── conversions.rs    # Parser → Domain model conversion functions
└── utxo/
    ├── mod.rs        # Re-exports UtxoCache, UtxoKey, CachedOutput
    └── cache.rs      # 16-shard LRU cache with Neo4j fallback
```

**Orchestrator:**
```rust
// domain/ingestion.rs
pub struct IngestionOrchestrator<W: GraphWriter> {
    writer: Arc<W>,           // Trait, not concrete Neo4j type
    network: Network,         // For address derivation
    utxo_cache: UtxoCache<W>, // Sharded LRU with Neo4j fallback
}

impl<W: GraphWriter + 'static> IngestionOrchestrator<W> {
    pub fn new(writer: W, network: Network, cache_size: usize) -> Self {
        let writer_arc = Arc::new(writer);
        let utxo_cache = UtxoCache::new(cache_size, Arc::clone(&writer_arc));
        Self { writer: writer_arc, network, utxo_cache }
    }

    pub async fn init_schema(&self) -> Result<()> { /* ... */ }
    pub async fn get_resume_height(&self) -> Result<u32> { /* ... */ }

    /// Ingest a single block through all phases
    pub async fn ingest_block(
        &self, block: &Block, height: u32, file_name: &str, file_offset: Option<u64>,
    ) -> Result<()> { /* Phase 1-7 */ }

    /// Ingest multiple blocks in batches for improved throughput
    pub async fn ingest_blocks_batch(
        &self, blocks: &[(u32, Block, String)], batch_size: usize,
    ) -> Result<()> { /* Bulk phase processing */ }

    pub fn cache_stats(&self) -> UtxoCacheStats { /* ... */ }
    pub fn get_cache(&self) -> &UtxoCache<W> { /* ... */ }
}
```

**UTXO Cache:**
```rust
// domain/utxo/cache.rs

/// Compact 36-byte UTXO identifier (stack-allocated, zero-alloc from OutPoint)
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct UtxoKey {
    txid: [u8; 32],  // Raw bytes, not hex string
    vout: u32,
}

/// Compact cached output (~36 bytes)
pub struct CachedOutput {
    pub output_index: u32,
    pub amount: u64,
    pub script_type: ScriptTypeTag,      // 1-byte enum
    pub address: Option<Arc<str>>,       // Shared across entries
}

/// 16-shard LRU cache with Neo4j fallback
pub struct UtxoCache<W: GraphWriter> {
    shards: Vec<Mutex<LruCache<UtxoKey, CachedOutput>>>,  // 16 shards
    writer: Arc<W>,                                        // Fallback on miss
    stats: AtomicUtxoCacheStats,                           // Lock-free counters
}

impl<W: GraphWriter + 'static> UtxoCache<W> {
    pub fn insert(&self, key: UtxoKey, value: CachedOutput) { /* ... */ }
    pub fn get_many(&self, keys: &[UtxoKey]) -> HashMap<UtxoKey, CachedOutput> { /* ... */ }
    pub async fn get_many_with_fallback(&self, keys: &[UtxoKey]) -> Result<HashMap<UtxoKey, CachedOutput>> { /* ... */ }
    pub fn remove_many(&self, keys: &[UtxoKey]) { /* ... */ }
}
```

---

### Layer 3: Writer (Database Operations - ISOLATED)

**Location**: `src/writer/`

**Responsibilities:**
- Define GraphWriter trait (database abstraction)
- Implement Neo4j-specific writer
- Implement mock writer for testing
- Centralize ALL Cypher queries
- Manage connection pool and retry logic
- Handle batch accumulation via UNWIND queries

**Key Principle**: This is the ONLY layer that imports `neo4rs`. All database operations happen here.

#### GraphWriter Trait (The Contract)

```rust
// writer/traits.rs
use async_trait::async_trait;

#[async_trait]
pub trait GraphWriter: Send + Sync {
    /// Initialize schema (constraints, indexes)
    async fn init_schema(&self) -> Result<()>;

    // Phase 1: Blocks
    async fn write_blocks(&self, blocks: &[BlockData]) -> Result<()>;

    // Phase 3: Transactions (with pre-calculated amounts)
    async fn write_transactions(&self, txs: &[TransactionData]) -> Result<()>;

    // Phase 2: Outputs
    async fn write_outputs(&self, outputs: &[OutputData]) -> Result<()>;

    // Phase 4: Inputs
    async fn write_inputs(&self, inputs: &[InputData]) -> Result<()>;

    // Phase 5: REMOVED — amounts calculated in Rust using UTXO cache

    // Phase 6: Simplified Layer (pre-aggregated in Rust)
    async fn write_performs(&self, performs: &[PerformsData]) -> Result<()>;
    async fn write_benefits_to(&self, benefits_to: &[BenefitsToData]) -> Result<()>;

    // UTXO Operations (for cache misses)
    async fn lookup_output(&self, output_id: &str) -> Result<OutputData>;
    async fn lookup_outputs_batch(&self, output_ids: &[String]) -> Result<Vec<OutputData>>;
    async fn mark_output_spent(&self, output_id: &str, spent_in_txid: &str, spent_at_height: u32) -> Result<()>;

    // Checkpoint Management
    async fn create_checkpoint(&self) -> Result<()>;
    async fn update_checkpoint(&self, checkpoint: &CheckpointData) -> Result<()>;
    async fn get_checkpoint(&self) -> Result<Option<CheckpointData>>;
    async fn mark_checkpoint_complete(&self) -> Result<()>;
    async fn set_checkpoint_status(&self, status: &str) -> Result<()>;
}
```

#### Neo4j Implementation

**Directory Structure**:
```
src/writer/
├── mod.rs              # Re-exports trait and Neo4j impl
├── traits.rs           # GraphWriter trait definition
├── error.rs            # WriterError enum
├── neo4j/
│   ├── mod.rs          # Neo4jWriter struct + GraphWriter impl
│   ├── schema.rs       # DDL operations (constraints, indexes)
│   ├── queries.rs      # ALL Cypher queries centralized
│   └── conversions.rs  # Domain model → BoltType conversions
└── mock.rs             # Mock implementation for testing
```

**Neo4j Writer Implementation**:
```rust
// writer/neo4j/mod.rs
use neo4rs::Graph;
use crate::config::Neo4jConfig;
use crate::writer::traits::GraphWriter;
use async_trait::async_trait;

pub struct Neo4jWriter {
    graph: Arc<Graph>,
    batch_size: usize,
    max_retries: usize,
}

impl Neo4jWriter {
    pub async fn new(config: Neo4jConfig) -> Result<Self> {
        let graph = Self::connect(&config).await?;
        let writer = Self {
            graph: Arc::new(graph),
            batch_size: config.write_batch_size,
            max_retries: config.max_retries,
        };
        writer.health_check().await?;
        Ok(writer)
    }
}

#[async_trait]
impl GraphWriter for Neo4jWriter {
    async fn write_blocks(&self, blocks: &[BlockData]) -> Result<()> {
        // Uses execute_batched() with UNWIND queries
        // Splits large datasets into chunks of batch_size (default: 5000)
        // Each chunk retried via run_with_retry() with exponential backoff
        use crate::writer::neo4j::queries::CREATE_BLOCKS_QUERY;
        self.execute_batched(blocks, CREATE_BLOCKS_QUERY, "blocks", blocks_to_bolt_list).await
    }

    async fn lookup_output(&self, output_id: &str) -> Result<OutputData> {
        use crate::writer::neo4j::queries::LOOKUP_OUTPUT_QUERY;
        // Single query with retry logic
        // Returns OutputData or WriterError::OutputNotFound
    }

    // ... all other trait methods follow same pattern
}
```

#### Centralized Queries

```rust
// writer/neo4j/queries.rs
// ALL Cypher queries live here — easy to find and update

/// Phase 1: Create block nodes with NEXT_BLOCK relationships
pub const CREATE_BLOCKS_QUERY: &str = r#"
    UNWIND $blocks AS block
    MERGE (b:Block {hash: block.hash})
    SET b.height = block.height,
        b.previousHash = block.previousHash,
        b.merkleRoot = block.merkleRoot,
        b.timestamp = datetime({epochSeconds: block.timestamp}),
        b.bits = block.bits,
        b.difficulty = block.difficulty,
        b.nonce = block.nonce,
        b.version = block.version,
        b.txCount = block.txCount,
        b.size = block.size,
        b.weight = block.weight
    WITH b, block
    WHERE block.height > 0
    OPTIONAL MATCH (prev:Block {height: block.height - 1})
    FOREACH (ignoreMe IN CASE WHEN prev IS NOT NULL THEN [1] ELSE [] END |
        MERGE (prev)-[:NEXT_BLOCK]->(b)
    )
"#;

/// Phase 3: Create transaction nodes with INCLUDED_IN (M7: includes amounts)
pub const CREATE_TRANSACTIONS_QUERY: &str = r#"
    UNWIND $transactions AS tx
    MERGE (t:Transaction {txid: tx.txid})
    SET t.blockHeight = tx.blockHeight,
        t.blockHash = tx.blockHash,
        t.timestamp = datetime({epochSeconds: tx.timestamp}),
        t.version = tx.version,
        t.locktime = tx.locktime,
        t.size = tx.size,
        t.vsize = tx.vsize,
        t.weight = tx.weight,
        t.isCoinbase = tx.isCoinbase,
        t.totalInput = tx.totalInput,
        t.totalOutput = tx.totalOutput,
        t.fee = tx.fee
    WITH t, tx
    MATCH (b:Block {height: tx.blockHeight})
    MERGE (t)-[:INCLUDED_IN]->(b)
"#;

/// Phase 2: Create output nodes with HAS_OUTPUT (MERGE with ON CREATE/ON MATCH)
pub const CREATE_OUTPUTS_QUERY: &str = r#"
    UNWIND $outputs AS out
    MERGE (o:Output {outputId: out.outputId})
    ON CREATE SET
        o.outputIndex = out.outputIndex,
        o.amount = out.amount,
        o.scriptPubKey = out.scriptPubKey,
        o.scriptType = out.scriptType,
        o.isSpent = false,
        o.spentInTxid = null,
        o.spentAtHeight = null
    ON MATCH SET
        o.outputIndex = out.outputIndex,
        o.amount = out.amount,
        o.scriptPubKey = out.scriptPubKey,
        o.scriptType = out.scriptType
    WITH o, out
    MATCH (t:Transaction {txid: out.txid})
    MERGE (t)-[:HAS_OUTPUT]->(o)
"#;

/// Phase 4: Create input nodes with SPENDS (coinbase excluded via WHERE)
pub const CREATE_INPUTS_QUERY: &str = r#"
    UNWIND $inputs AS inp
    MERGE (i:Input {inputId: inp.inputId})
    SET i.inputIndex = inp.inputIndex,
        i.scriptSig = inp.scriptSig,
        i.sequence = inp.sequence,
        i.witness = inp.witness
    WITH i, inp
    MATCH (t:Transaction {txid: inp.txid})
    MERGE (t)-[:HAS_INPUT]->(i)
    WITH i, inp
    WHERE inp.previousOutputIndex <> 4294967295
    MATCH (o:Output {outputId: inp.previousTxid + ':' + toString(inp.previousOutputIndex)})
    MERGE (i)-[:SPENDS]->(o)
    SET o.isSpent = true,
        o.spentInTxid = inp.txid,
        o.spentAtHeight = inp.blockHeight
"#;

// Phase 5: REMOVED — amounts calculated in Rust using UTXO cache

/// Phase 6: Bulk PERFORMS relationships (pre-aggregated in Rust)
pub const CREATE_PERFORMS_BULK_QUERY: &str = r#"
    UNWIND $performs AS p
    MERGE (addr:Address {address: p.fromAddress})
    WITH addr, p
    MATCH (t:Transaction {txid: p.toTxid})
    MERGE (addr)-[r:PERFORMS]->(t)
    SET r.inputCount = p.inputCount,
        r.amountSpent = p.amountSpent
"#;

/// Phase 6: Bulk BENEFITS_TO relationships (pre-aggregated in Rust)
pub const CREATE_BENEFITS_TO_BULK_QUERY: &str = r#"
    UNWIND $benefitsTo AS b
    MATCH (t:Transaction {txid: b.fromTxid})
    WITH t, b
    MERGE (addr:Address {address: b.toAddress})
    MERGE (t)-[r:BENEFITS_TO]->(addr)
    SET r.outputCount = b.outputCount,
        r.amountReceived = b.amountReceived
"#;

/// Batch output lookup (for UTXO cache misses)
pub const LOOKUP_OUTPUTS_BATCH_QUERY: &str = r#"
    UNWIND $outputIds AS oid
    MATCH (o:Output {outputId: oid})
    OPTIONAL MATCH (o)-[:LOCKED_TO]->(a:Address)
    RETURN o.outputId AS outputId,
           o.outputIndex AS outputIndex,
           o.amount AS amount,
           o.scriptPubKey AS scriptPubKey,
           o.scriptType AS scriptType,
           a.address AS address
"#;

/// Checkpoint: sentinel height -999 (neo4rs bug avoids -1)
pub const CREATE_CHECKPOINT_QUERY: &str = r#"
    CREATE (c:IngestionCheckpoint {
        lastProcessedHeight: -999,
        lastProcessedHash: '0000...0000',
        lastProcessedFile: 'blk00000.dat',
        lastProcessedFileOffset: 0,
        timestamp: datetime(),
        status: 'in_progress'
    })
"#;

/// Checkpoint: MERGE-based update (guarantees node exists)
pub const UPDATE_CHECKPOINT_QUERY: &str = r#"
    MERGE (c:IngestionCheckpoint)
    SET c.lastProcessedHeight = $height,
        c.lastProcessedHash = $hash,
        c.lastProcessedFile = $file,
        c.lastProcessedFileOffset = $offset,
        c.timestamp = datetime(),
        c.status = $status
"#;
```

**Benefits of Centralization**:
- Need to change a query? Edit one file
- Easy to review all queries for optimization
- Easy to version control query changes
- Easy to test queries in Neo4j Browser (copy/paste)

#### Mock Implementation (Testing)

```rust
// writer/mock.rs
use crate::writer::traits::GraphWriter;
use async_trait::async_trait;
use std::sync::Mutex;

/// Mock writer for testing — stores data in memory
pub struct MockWriter {
    blocks: Mutex<Vec<BlockData>>,
    transactions: Mutex<Vec<TransactionData>>,
    outputs: Mutex<Vec<OutputData>>,
    inputs: Mutex<Vec<InputData>>,
    performs: Mutex<Vec<PerformsData>>,
    benefits_to: Mutex<Vec<BenefitsToData>>,
    checkpoint: Mutex<Option<CheckpointData>>,
}

// Implements all GraphWriter trait methods:
// - init_schema, write_blocks, write_transactions, write_outputs, write_inputs
// - write_performs, write_benefits_to
// - lookup_output, lookup_outputs_batch, mark_output_spent
// - create_checkpoint, update_checkpoint, get_checkpoint,
//   mark_checkpoint_complete, set_checkpoint_status
```

---

## Configuration

**Location**: `src/config/`

```
src/config/
├── mod.rs     # Config structs, defaults, validation
└── loader.rs  # TOML file loading
```

**Config sections:**

| Section | Struct | Description |
|---------|--------|-------------|
| `[bitcoin]` | `BitcoinConfig` | `blocks_dir`, `start_height`, `end_height` |
| `[neo4j]` | `Neo4jConfig` | `uri`, `user`, `password`, `database`, pool settings, `write_batch_size`, `max_retries` |
| `[ingestion]` | `IngestionConfig` | `batch_size`, `checkpoint_interval`, `enable_validation`, `auto_resume` |
| `[performance]` | `PerformanceConfig` | `utxo_cache_memory_mb`, `utxo_prewarm_depth`, `parallel_batches` |
| `[logging]` | `LoggingConfig` | `level`, `json_format` |
| `[bitcoin_rpc]` | `BitcoinRpcConfig` | RPC url/credentials, `batch_size`, ZMQ endpoint, retry settings (optional, live mode only) |

---

## Dependency Direction

### Strict One-Way Dependencies

```
main.rs
  ↓
domain/ (orchestration)
  ↓
writer/traits.rs (abstraction)
  ↓
writer/neo4j/ (implementation)

parser/ (independent, no dependencies on other layers)
config/ (independent, no dependencies on other layers)
```

**Key Rules**:
1. Parser NEVER imports from domain or writer
2. Domain NEVER imports neo4rs (only uses GraphWriter trait)
3. Writer is the ONLY module that imports neo4rs
4. Main.rs wires everything together (dependency injection)

**Example Wiring**:
```rust
// main.rs
use bitcoin_chain_graph::{
    config::Config,
    parser::SingleBlockLoader,
    domain::IngestionOrchestrator,
    writer::neo4j::Neo4jWriter,
};
use bitcoin::Network;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_file("config.toml")?;

    // Create writer implementation
    let writer = Neo4jWriter::new(config.neo4j.clone()).await?;

    // Inject writer into domain orchestrator
    let orchestrator = IngestionOrchestrator::new(
        writer,
        Network::Bitcoin,
        config.performance.cache_capacity(),
    );

    // Initialize schema
    orchestrator.init_schema().await?;

    // Parser is independent
    let loader = SingleBlockLoader::new(&config.bitcoin.blocks_dir, Network::Bitcoin)?;

    // Load and ingest blocks
    let (block, file_name, offset) = loader.load_block(0)?;
    orchestrator.ingest_block(&block, 0, &file_name, offset).await?;

    Ok(())
}
```

---

## Key Design Principles

### 1. Zero Leakage

**Neo4j types never escape the writer module**:
```rust
// BAD: Neo4j type in domain layer
use neo4rs::Node;
pub fn process_block(node: Node) -> Result<()> { /* ... */ }

// GOOD: Domain types only
pub fn process_block(block: BlockData) -> Result<()> { /* ... */ }
```

### 2. Interface Stability

**Domain models are stable, writer implementation can change**:
```rust
// Domain model (stable)
pub struct OutputData {
    pub output_id: String,
    pub output_index: u32,
    pub txid: String,
    pub amount: u64,           // Satoshis as integer
    pub script_pubkey: String,
    pub script_type: String,
    pub address: Option<String>,
}

// Checkpoint domain model (stable)
pub struct CheckpointData {
    pub last_processed_height: i32,
    pub last_processed_hash: String,
    pub last_processed_file: String,
    pub last_processed_file_offset: Option<u64>,
    pub timestamp: i64,
    pub status: String,
}

// Writer can change how it stores this:
// - Neo4j today
// - PostgreSQL tomorrow
// - Multiple DBs simultaneously
// Domain code doesn't change
```

### 3. Testability

**Test business logic without Neo4j**:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::mock::MockWriter;
    use bitcoin::Network;

    #[tokio::test]
    async fn test_ingestion_without_database() {
        let writer = MockWriter::new();
        let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, 10_000);

        // Initialize schema
        orchestrator.init_schema().await.unwrap();

        // Ingest a block
        let block = create_test_block();
        orchestrator.ingest_block(&block, 0, "test.dat", None).await.unwrap();

        // Assert using cache stats
        let stats = orchestrator.cache_stats();
        assert!(stats.inserts > 0);
    }
}
```

### 4. Easy Query Updates

**Need to optimize a query? Edit one file**:
```rust
// All queries in writer/neo4j/queries.rs
// Only writer/neo4j/queries.rs changed
// Domain layer, parser layer, tests — all unchanged
```

---

## Module Summary

| Module | Responsibility | Dependencies | Outputs |
|--------|---------------|--------------|---------|
| **config/** | Configuration loading & validation | `serde`, `toml` | `Config` struct |
| **parser/** | Read .blk files, RPC, ZMQ | `bitcoin`, `reqwest`, `zeromq`, `leveldb` | Domain models, raw blocks |
| **domain/** | Business logic, UTXO cache | parser, GraphWriter trait | Orchestration |
| **domain/utxo/** | Sharded LRU cache | GraphWriter trait | Cache lookups |
| **writer/traits.rs** | Database abstraction | None | Trait definition |
| **writer/neo4j/** | Neo4j implementation | `neo4rs` | Trait implementation |
| **writer/mock.rs** | Testing mock | None | Trait implementation |

---

## Benefits

### For Development

- **Clear boundaries**: Each layer has single responsibility
- **Easy to reason about**: Follow data flow from parser -> domain -> writer
- **Parallel work**: Team can work on parser, domain, writer independently
- **Type safety**: Rust compiler enforces boundaries

### For Testing

- **Fast tests**: Test parser and domain without database
- **Integration tests**: Use mock writer, no Neo4j setup
- **Isolated testing**: Test queries separately from business logic

### For Maintenance

- **Query updates**: Edit writer/neo4j/queries.rs only
- **Performance tuning**: Optimize batch sizes in writer only
- **Database migration**: Implement new writer, domain unchanged
- **Refactoring**: Change writer internals, interface stable

### For Future

- **Multiple databases**: Implement additional writers
- **Hybrid storage**: Use both Neo4j and time-series DB
- **Caching layer**: Wrap writer with decorator
- **Read replicas**: Add read-only writer implementation

---

## Anti-Patterns to Avoid

### Domain Layer Importing neo4rs

```rust
// BAD: Domain layer should NOT import neo4rs
use neo4rs::{Graph, query};

pub struct IngestionOrchestrator {
    graph: Graph,  // Neo4j type leaked into domain
}
```

### Scattering Queries Across Modules

```rust
// BAD: Query defined in domain layer
pub async fn ingest_blocks(&self, blocks: &[Block]) -> Result<()> {
    let query = "UNWIND $blocks AS block CREATE (b:Block {...})";
    self.writer.execute(query).await?;
}

// GOOD: Query defined in writer/neo4j/queries.rs
pub async fn ingest_blocks(&self, blocks: &[BlockData]) -> Result<()> {
    self.writer.write_blocks(blocks).await?;
}
```

### Parser Doing Business Logic

```rust
// BAD: Parser shouldn't know about UTXO cache
pub fn next_block(&mut self, utxo_cache: &mut UtxoCache) -> Result<Option<Block>> {
    let block = self.read_block()?;
    utxo_cache.update(&block);  // Business logic in parser
    Ok(Some(block))
}

// GOOD: Parser returns data, domain handles logic
pub fn load_block(&self, height: u32) -> Result<(Block, String, Option<u64>)> {
    // Pure data loading
}
```

---

## Next Steps

1. Review [INGESTION_ARCHITECTURE.md](INGESTION_ARCHITECTURE.md) for 6-phase ingestion process with UTXO cache
2. Review [REAL_TIME_ARCHITECTURE.md](REAL_TIME_ARCHITECTURE.md) for live mode (RPC + ZMQ)
