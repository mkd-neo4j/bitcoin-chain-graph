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
│  • Transaction validation                                        │
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
- Detect script types (P2PKH, P2SH, P2WPKH, etc.)

**Key Principle**: Parser knows ONLY Bitcoin protocol, nothing about Neo4j or any database

**Dependencies**: `bitcoin` crate only

**Outputs**: Domain models (`Block`, `Transaction`, `Output`, `Input`)

**Example**:
```rust
// parser/block_file.rs
pub struct BlockFileReader {
    reader: BufReader<File>,
}

impl BlockFileReader {
    pub fn next_block(&mut self) -> Result<Option<Block>> {
        // Read and deserialize block
        // Returns domain model, no Neo4j types
    }
}

// parser/address.rs
pub fn derive_address(script: &Script, network: Network) -> Option<String> {
    // Pure function: Script → Address string
    // No side effects, no database calls
}
```

---

### Layer 2: Domain Logic (Zero Database Knowledge)

**Location**: `src/domain/`

**Responsibilities:**
- Orchestrate 6-phase ingestion process
- Manage UTXO cache (LRU cache with trait-based persistence)
- Validate transactions
- Checkpoint management for resumability
- Business logic and state management

**Key Principle**: Domain logic calls GraphWriter trait methods but never imports `neo4rs`

**Dependencies**: Parser layer, GraphWriter trait (abstraction only)

**Example**:
```rust
// domain/ingestion.rs
pub struct IngestionOrchestrator<W: GraphWriter> {
    writer: Arc<W>,  // Trait, not concrete Neo4j type
    utxo_cache: UtxoCache<W>,
}

impl<W: GraphWriter> IngestionOrchestrator<W> {
    /// Initialize ingestion (create checkpoint)
    pub async fn initialize(&self) -> Result<()> {
        self.writer.create_checkpoint().await?;
        Ok(())
    }

    /// Resume from last checkpoint
    pub async fn resume(&self) -> Result<Option<u32>> {
        if let Some(checkpoint) = self.writer.get_checkpoint().await? {
            Ok(Some(checkpoint.last_processed_height + 1))
        } else {
            Ok(None)  // No checkpoint, start from genesis
        }
    }

    /// Phase 1: Create block nodes
    pub async fn ingest_blocks(&mut self, blocks: Vec<Block>) -> Result<()> {
        let block_data: Vec<BlockData> = blocks
            .iter()
            .map(|b| BlockData::from_block(b))
            .collect();

        // Call trait method - no Neo4j-specific code here
        self.writer.write_blocks(&block_data).await?;

        Ok(())
    }

    /// After successfully ingesting a block, update checkpoint
    pub async fn finalize_block(&self, block: &Block, file_name: &str, file_offset: Option<u64>) -> Result<()> {
        let checkpoint = CheckpointData {
            last_processed_height: block.height,
            last_processed_hash: block.hash.clone(),
            last_processed_file: file_name.to_string(),
            last_processed_file_offset: file_offset,
            status: "in_progress".to_string(),
        };
        self.writer.update_checkpoint(checkpoint).await?;
        Ok(())
    }

    /// Phase 3: Create outputs (adds to UTXO cache)
    pub async fn ingest_outputs(&mut self, txs: &[Transaction]) -> Result<()> {
        let output_data: Vec<OutputData> = extract_outputs(txs);

        // Write to database via trait
        self.writer.write_outputs(&output_data).await?;

        // Update in-memory cache
        for output in output_data {
            self.utxo_cache.insert(output.output_id.clone(), output);
        }

        Ok(())
    }
}
```

**UTXO Cache Abstraction**:
```rust
// domain/utxo/cache.rs
pub struct UtxoCache<W: GraphWriter> {
    cache: LruCache<String, CachedOutput>,
    writer: Arc<W>,  // For cache misses
}

impl<W: GraphWriter> UtxoCache<W> {
    /// Get output (cache-first, then query via trait)
    pub async fn get(&mut self, output_id: &str) -> Result<CachedOutput> {
        // Try cache
        if let Some(output) = self.cache.get(output_id) {
            return Ok(output.clone());
        }

        // Cache miss - use trait method (NOT Neo4j-specific)
        let output = self.writer.lookup_output(output_id).await?;
        self.cache.put(output_id.to_string(), output.clone());
        Ok(output)
    }
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
- Manage connection pool
- Handle batch accumulation

**Key Principle**: This is the ONLY layer that imports `neo4rs`. All database operations happen here.

#### GraphWriter Trait (The Contract)

```rust
// writer/traits.rs
use async_trait::async_trait;

#[async_trait]
pub trait GraphWriter: Send + Sync {
    /// Initialize schema (constraints, indexes)
    async fn init_schema(&self) -> Result<()>;

    /// Write block nodes in bulk
    async fn write_blocks(&self, blocks: &[BlockData]) -> Result<()>;

    /// Write transaction nodes in bulk
    async fn write_transactions(&self, txs: &[TransactionData]) -> Result<()>;

    /// Write output nodes and LOCKED_TO relationships in bulk
    async fn write_outputs(&self, outputs: &[OutputData]) -> Result<()>;

    /// Write input nodes and SPENDS relationships in bulk
    async fn write_inputs(&self, inputs: &[InputData]) -> Result<()>;

    /// Update transaction amounts (Phase 5)
    async fn calculate_amounts(&self, txs: &[TransactionData]) -> Result<()>;

    /// Create simplified BENEFITS_TO relationships (Phase 6)
    async fn write_simplified_layer(&self, txs: &[TransactionData]) -> Result<()>;

    /// Lookup output by ID (for UTXO cache misses)
    async fn lookup_output(&self, output_id: &str) -> Result<OutputData>;

    /// Mark output as spent (update isSpent, spentInTxid, spentAtHeight)
    async fn mark_output_spent(&self, output_id: &str, spent_in_txid: &str, spent_at_height: u32) -> Result<()>;

    // Checkpoint management for resume-on-failure

    /// Create initial checkpoint (before starting ingestion)
    async fn create_checkpoint(&self) -> Result<()>;

    /// Update checkpoint after successfully processing a block
    async fn update_checkpoint(&self, checkpoint: CheckpointData) -> Result<()>;

    /// Get current checkpoint state (for resume logic)
    async fn get_checkpoint(&self) -> Result<Option<CheckpointData>>;

    /// Mark ingestion as complete
    async fn mark_checkpoint_complete(&self) -> Result<()>;
}
```

#### Neo4j Implementation

**Directory Structure**:
```
src/writer/
├── mod.rs              # Re-exports trait and Neo4j impl
├── traits.rs           # GraphWriter trait definition
├── neo4j/
│   ├── mod.rs          # Neo4jWriter struct
│   ├── client.rs       # Connection pool (neo4rs::Graph)
│   ├── schema.rs       # DDL operations (constraints, indexes)
│   ├── queries.rs      # ALL Cypher queries centralized
│   └── batch.rs        # Batch accumulator
└── mock.rs             # Mock implementation for testing
```

**Neo4j Writer Implementation**:
```rust
// writer/neo4j/mod.rs
use neo4rs::Graph;
use crate::writer::traits::GraphWriter;
use async_trait::async_trait;

pub struct Neo4jWriter {
    graph: Graph,
    batch_builder: BatchBuilder,
}

impl Neo4jWriter {
    pub async fn new(uri: &str, user: &str, password: &str) -> Result<Self> {
        let graph = connect_neo4j(uri, user, password).await?;
        Ok(Self {
            graph,
            batch_builder: BatchBuilder::new(500, 4096),
        })
    }
}

#[async_trait]
impl GraphWriter for Neo4jWriter {
    async fn write_blocks(&self, blocks: &[BlockData]) -> Result<()> {
        // Import query from centralized location
        use crate::writer::neo4j::queries::CREATE_BLOCKS_QUERY;

        let query = neo4rs::query(CREATE_BLOCKS_QUERY)
            .param("blocks", blocks_to_neo4j_list(blocks)?);

        self.graph.run(query).await?;
        Ok(())
    }

    async fn write_outputs(&self, outputs: &[OutputData]) -> Result<()> {
        use crate::writer::neo4j::queries::{CREATE_OUTPUTS_QUERY, CREATE_LOCKED_TO_QUERY};

        // Create output nodes
        let query = neo4rs::query(CREATE_OUTPUTS_QUERY)
            .param("outputs", outputs_to_neo4j_list(outputs)?);
        self.graph.run(query).await?;

        // Create LOCKED_TO relationships (for outputs with addresses)
        let outputs_with_address: Vec<_> = outputs
            .iter()
            .filter(|o| o.address.is_some())
            .collect();

        if !outputs_with_address.is_empty() {
            let query = neo4rs::query(CREATE_LOCKED_TO_QUERY)
                .param("outputs", outputs_to_neo4j_list(&outputs_with_address)?);
            self.graph.run(query).await?;
        }

        Ok(())
    }

    async fn lookup_output(&self, output_id: &str) -> Result<OutputData> {
        use crate::writer::neo4j::queries::LOOKUP_OUTPUT_QUERY;

        let mut result = self.graph.execute(
            neo4rs::query(LOOKUP_OUTPUT_QUERY)
                .param("outputId", output_id)
        ).await?;

        let row = result.next().await?
            .ok_or_else(|| Error::OutputNotFound(output_id.to_string()))?;

        Ok(OutputData {
            output_id: row.get("outputId")?,
            amount: row.get("amount")?,
            script_type: row.get("scriptType")?,
            address: row.get("address").ok(),
        })
    }

    // Checkpoint management implementation

    async fn create_checkpoint(&self) -> Result<()> {
        use crate::writer::neo4j::queries::CREATE_CHECKPOINT_QUERY;

        self.graph.run(neo4rs::query(CREATE_CHECKPOINT_QUERY)).await?;
        Ok(())
    }

    async fn update_checkpoint(&self, checkpoint: CheckpointData) -> Result<()> {
        use crate::writer::neo4j::queries::UPDATE_CHECKPOINT_QUERY;

        let query = neo4rs::query(UPDATE_CHECKPOINT_QUERY)
            .param("blockHeight", checkpoint.last_processed_height)
            .param("blockHash", checkpoint.last_processed_hash)
            .param("blkFileName", checkpoint.last_processed_file)
            .param("fileOffset", checkpoint.last_processed_file_offset)
            .param("status", checkpoint.status);

        self.graph.run(query).await?;
        Ok(())
    }

    async fn get_checkpoint(&self) -> Result<Option<CheckpointData>> {
        use crate::writer::neo4j::queries::GET_CHECKPOINT_QUERY;

        let mut result = self.graph.execute(
            neo4rs::query(GET_CHECKPOINT_QUERY)
        ).await?;

        if let Some(row) = result.next().await? {
            Ok(Some(CheckpointData {
                last_processed_height: row.get("lastHeight")?,
                last_processed_hash: row.get("lastHash")?,
                last_processed_file: row.get("lastFile")?,
                last_processed_file_offset: row.get("fileOffset").ok(),
                status: row.get("status")?,
            }))
        } else {
            Ok(None)
        }
    }

    async fn mark_checkpoint_complete(&self) -> Result<()> {
        use crate::writer::neo4j::queries::MARK_CHECKPOINT_COMPLETE_QUERY;

        self.graph.run(neo4rs::query(MARK_CHECKPOINT_COMPLETE_QUERY)).await?;
        Ok(())
    }
}
```

#### Centralized Queries

```rust
// writer/neo4j/queries.rs
// ALL Cypher queries live here - easy to find and update

/// Phase 1: Create block nodes with NEXT_BLOCK relationships
pub const CREATE_BLOCKS_QUERY: &str = r#"
    UNWIND $blocks AS block
    CREATE (b:Block {
        height: block.height,
        hash: block.hash,
        previousHash: block.previousHash,
        merkleRoot: block.merkleRoot,
        timestamp: datetime({epochSeconds: block.timestamp}),
        bits: block.bits,
        nonce: block.nonce,
        version: block.version,
        size: block.size,
        transactionCount: block.transactionCount
    })
    WITH b, block
    MATCH (prev:Block {height: block.height - 1})
    CREATE (prev)-[:NEXT_BLOCK]->(b)
"#;

/// Phase 2: Create transaction nodes with INCLUDED_IN relationships
pub const CREATE_TRANSACTIONS_QUERY: &str = r#"
    UNWIND $transactions AS tx
    CREATE (t:Transaction {
        txid: tx.txid,
        version: tx.version,
        locktime: tx.locktime,
        size: tx.size,
        vsize: tx.vsize,
        weight: tx.weight,
        isCoinbase: tx.isCoinbase,
        blockHeight: tx.blockHeight
    })
    WITH t, tx
    MATCH (b:Block {height: tx.blockHeight})
    CREATE (t)-[:INCLUDED_IN]->(b)
"#;

/// Phase 3: Create output nodes
pub const CREATE_OUTPUTS_QUERY: &str = r#"
    UNWIND $outputs AS out
    CREATE (o:Output {
        outputId: out.outputId,
        outputIndex: out.outputIndex,
        amount: out.amount,
        scriptPubKey: out.scriptPubKey,
        scriptType: out.scriptType,
        isSpent: false,
        spentInTxid: null,
        spentAtHeight: null
    })
    WITH o, out
    MATCH (t:Transaction {txid: out.txid})
    CREATE (t)-[:HAS_OUTPUT]->(o)
"#;

/// Phase 3: Create LOCKED_TO relationships (for outputs with addresses)
pub const CREATE_LOCKED_TO_QUERY: &str = r#"
    UNWIND $outputs AS out
    MATCH (o:Output {outputId: out.outputId})
    MERGE (a:Address {address: out.address})
    CREATE (o)-[:LOCKED_TO]->(a)
"#;

/// Phase 4: Create input nodes with SPENDS relationships
pub const CREATE_INPUTS_QUERY: &str = r#"
    UNWIND $inputs AS inp
    CREATE (i:Input {
        inputId: inp.inputId,
        inputIndex: inp.inputIndex,
        scriptSig: inp.scriptSig,
        sequence: inp.sequence,
        witness: inp.witness
    })
    WITH i, inp
    MATCH (t:Transaction {txid: inp.txid})
    CREATE (i)-[:HAS_INPUT]->(t)
    WITH i, inp
    MATCH (o:Output {outputId: inp.previousOutputId})
    CREATE (i)-[:SPENDS]->(o)
    SET o.isSpent = true,
        o.spentInTxid = inp.txid,
        o.spentAtHeight = inp.blockHeight
"#;

/// Lookup single output by ID (for UTXO cache misses)
pub const LOOKUP_OUTPUT_QUERY: &str = r#"
    MATCH (o:Output {outputId: $outputId})
    OPTIONAL MATCH (o)-[:LOCKED_TO]->(a:Address)
    RETURN o.outputId as outputId,
           o.amount as amount,
           o.scriptType as scriptType,
           a.address as address
"#;

/// Checkpoint management: Create initial checkpoint
pub const CREATE_CHECKPOINT_QUERY: &str = r#"
    CREATE (c:IngestionCheckpoint {
        lastProcessedHeight: -1,
        lastProcessedHash: null,
        lastProcessedFile: null,
        lastProcessedFileOffset: null,
        timestamp: datetime(),
        status: "in_progress"
    })
"#;

/// Checkpoint management: Update checkpoint after successful block
pub const UPDATE_CHECKPOINT_QUERY: &str = r#"
    MATCH (c:IngestionCheckpoint)
    SET c.lastProcessedHeight = $blockHeight,
        c.lastProcessedHash = $blockHash,
        c.lastProcessedFile = $blkFileName,
        c.lastProcessedFileOffset = $fileOffset,
        c.timestamp = datetime(),
        c.status = $status
"#;

/// Checkpoint management: Get current checkpoint
pub const GET_CHECKPOINT_QUERY: &str = r#"
    MATCH (c:IngestionCheckpoint)
    RETURN c.lastProcessedHeight AS lastHeight,
           c.lastProcessedHash AS lastHash,
           c.lastProcessedFile AS lastFile,
           c.lastProcessedFileOffset AS fileOffset,
           c.status AS status
"#;

/// Checkpoint management: Mark ingestion complete
pub const MARK_CHECKPOINT_COMPLETE_QUERY: &str = r#"
    MATCH (c:IngestionCheckpoint)
    SET c.status = "completed",
        c.timestamp = datetime()
"#;

// ... more queries for Phase 5, Phase 6, validation, etc.
```

**Benefits of Centralization**:
- ✅ Need to change a query? Edit one file
- ✅ Easy to review all queries for optimization
- ✅ Easy to version control query changes
- ✅ Easy to generate documentation from queries
- ✅ Easy to test queries in Neo4j Browser (copy/paste)

#### Mock Implementation (Testing)

```rust
// writer/mock.rs
use crate::writer::traits::GraphWriter;
use async_trait::async_trait;
use std::sync::Mutex;

/// Mock writer for testing - stores data in memory
pub struct MockWriter {
    blocks: Mutex<Vec<BlockData>>,
    transactions: Mutex<Vec<TransactionData>>,
    outputs: Mutex<Vec<OutputData>>,
    inputs: Mutex<Vec<InputData>>,
}

impl MockWriter {
    pub fn new() -> Self {
        Self {
            blocks: Mutex::new(Vec::new()),
            transactions: Mutex::new(Vec::new()),
            outputs: Mutex::new(Vec::new()),
            inputs: Mutex::new(Vec::new()),
        }
    }

    /// Get all blocks written (for test assertions)
    pub fn get_blocks(&self) -> Vec<BlockData> {
        self.blocks.lock().unwrap().clone()
    }

    /// Get all outputs written (for test assertions)
    pub fn get_outputs(&self) -> Vec<OutputData> {
        self.outputs.lock().unwrap().clone()
    }
}

#[async_trait]
impl GraphWriter for MockWriter {
    async fn init_schema(&self) -> Result<()> {
        // No-op for mock
        Ok(())
    }

    async fn write_blocks(&self, blocks: &[BlockData]) -> Result<()> {
        self.blocks.lock().unwrap().extend_from_slice(blocks);
        Ok(())
    }

    async fn write_outputs(&self, outputs: &[OutputData]) -> Result<()> {
        self.outputs.lock().unwrap().extend_from_slice(outputs);
        Ok(())
    }

    async fn lookup_output(&self, output_id: &str) -> Result<OutputData> {
        self.outputs.lock().unwrap()
            .iter()
            .find(|o| o.output_id == output_id)
            .cloned()
            .ok_or_else(|| Error::OutputNotFound(output_id.to_string()))
    }

    // ... implement other trait methods
}
```

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
```

**Key Rules**:
1. ✅ Parser NEVER imports from domain or writer
2. ✅ Domain NEVER imports neo4rs (only uses GraphWriter trait)
3. ✅ Writer is the ONLY module that imports neo4rs
4. ✅ Main.rs wires everything together (dependency injection)

**Example Wiring**:
```rust
// main.rs
use bitcoin_chain_graph::{
    parser::BlockFileReader,
    domain::IngestionOrchestrator,
    writer::neo4j::Neo4jWriter,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Create writer implementation
    let writer = Neo4jWriter::new("bolt://localhost:7687", "neo4j", "password").await?;

    // Inject writer into domain orchestrator
    let mut orchestrator = IngestionOrchestrator::new(Arc::new(writer));

    // Parser is independent
    let mut parser = BlockFileReader::new("/path/to/blk00000.dat")?;

    // Orchestrate ingestion
    while let Some(block) = parser.next_block()? {
        orchestrator.ingest_block(block).await?;
    }

    Ok(())
}
```

---

## Key Design Principles

### 1. Zero Leakage

**Neo4j types never escape the writer module**:
```rust
// ❌ BAD: Neo4j type in domain layer
use neo4rs::Node;

pub fn process_block(node: Node) -> Result<()> {
    // Domain code shouldn't see neo4rs types
}

// ✅ GOOD: Domain types only
pub fn process_block(block: BlockData) -> Result<()> {
    // Domain model, no Neo4j knowledge
}
```

### 2. Interface Stability

**Domain models are stable, writer implementation can change**:
```rust
// Domain model (stable)
pub struct OutputData {
    pub output_id: String,
    pub amount: f64,
    pub script_type: String,
    pub address: Option<String>,
}

// Checkpoint domain model (stable)
pub struct CheckpointData {
    pub last_processed_height: u32,
    pub last_processed_hash: String,
    pub last_processed_file: String,
    pub last_processed_file_offset: Option<u64>,
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

    #[tokio::test]
    async fn test_ingestion_without_database() {
        // Use mock writer - no Neo4j needed
        let writer = Arc::new(MockWriter::new());
        let mut orchestrator = IngestionOrchestrator::new(writer.clone());

        // Test business logic
        let block = create_test_block();
        orchestrator.ingest_block(block).await.unwrap();

        // Assert using mock methods
        let blocks = writer.get_blocks();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].height, 100);
    }
}
```

### 4. Easy Query Updates

**Need to optimize a query? Edit one file**:
```rust
// Before: Slow query
pub const CREATE_OUTPUTS_QUERY: &str = r#"
    UNWIND $outputs AS out
    CREATE (o:Output {...})
    WITH o, out
    MATCH (t:Transaction {txid: out.txid})  // Slow: index lookup per output
    CREATE (t)-[:HAS_OUTPUT]->(o)
"#;

// After: Optimized with WHERE
pub const CREATE_OUTPUTS_QUERY: &str = r#"
    UNWIND $outputs AS out
    CREATE (o:Output {...})
    WITH collect(o) as outputs, collect(out) as outputData
    UNWIND range(0, size(outputs)-1) AS idx
    WITH outputs[idx] as o, outputData[idx] as out
    MATCH (t:Transaction)
    WHERE t.txid = out.txid
    CREATE (t)-[:HAS_OUTPUT]->(o)
"#;

// Only writer/neo4j/queries.rs changed
// Domain layer, parser layer, tests - all unchanged
```

---

## Module Summary

| Module | Responsibility | Dependencies | Outputs |
|--------|---------------|--------------|---------|
| **parser/** | Read .blk files | `bitcoin` | Domain models |
| **domain/** | Business logic | parser, GraphWriter trait | Orchestration |
| **writer/traits.rs** | Database abstraction | None | Trait definition |
| **writer/neo4j/** | Neo4j implementation | `neo4rs` | Trait implementation |
| **writer/mock.rs** | Testing mock | None | Trait implementation |

---

## Benefits

### For Development

✅ **Clear boundaries**: Each layer has single responsibility
✅ **Easy to reason about**: Follow data flow from parser → domain → writer
✅ **Parallel work**: Team can work on parser, domain, writer independently
✅ **Type safety**: Rust compiler enforces boundaries

### For Testing

✅ **Fast tests**: Test parser and domain without database
✅ **Integration tests**: Use mock writer, no Neo4j setup
✅ **Isolated testing**: Test queries separately from business logic

### For Maintenance

✅ **Query updates**: Edit writer/neo4j/queries.rs only
✅ **Performance tuning**: Optimize batch sizes in writer only
✅ **Database migration**: Implement new writer, domain unchanged
✅ **Refactoring**: Change writer internals, interface stable

### For Future

✅ **Multiple databases**: Implement additional writers
✅ **Hybrid storage**: Use both Neo4j and time-series DB
✅ **Caching layer**: Wrap writer with decorator
✅ **Read replicas**: Add read-only writer implementation

---

## Anti-Patterns to Avoid

### ❌ Domain Layer Importing neo4rs

```rust
// ❌ BAD: Domain layer should NOT import neo4rs
use neo4rs::{Graph, query};

pub struct IngestionOrchestrator {
    graph: Graph,  // Neo4j type leaked into domain
}
```

### ❌ Scattering Queries Across Modules

```rust
// ❌ BAD: Query defined in domain layer
pub async fn ingest_blocks(&self, blocks: &[Block]) -> Result<()> {
    let query = "UNWIND $blocks AS block CREATE (b:Block {...})";
    self.writer.execute(query).await?;
}

// ✅ GOOD: Query defined in writer/neo4j/queries.rs
pub async fn ingest_blocks(&self, blocks: &[BlockData]) -> Result<()> {
    self.writer.write_blocks(blocks).await?;
}
```

### ❌ Parser Doing Business Logic

```rust
// ❌ BAD: Parser shouldn't know about UTXO cache
pub fn next_block(&mut self, utxo_cache: &mut UtxoCache) -> Result<Option<Block>> {
    let block = self.read_block()?;
    utxo_cache.update(&block);  // Business logic in parser
    Ok(Some(block))
}

// ✅ GOOD: Parser returns data, domain handles logic
pub fn next_block(&mut self) -> Result<Option<Block>> {
    self.read_block()
}
```

---

## Next Steps

1. Review [NEO4J_INTEGRATION.md](../rust/NEO4J_INTEGRATION.md) for Neo4j-specific implementation details
2. Review [TESTING.md](../rust/TESTING.md) for testing strategy with mock writer
3. Review [SETUP.md](../rust/SETUP.md) for detailed project structure
4. Review [INGESTION_ARCHITECTURE.md](INGESTION_ARCHITECTURE.md) for 6-phase ingestion process
