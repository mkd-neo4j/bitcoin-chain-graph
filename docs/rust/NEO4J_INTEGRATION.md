# Neo4j Integration with neo4rs

Rust implementation guide for connecting to Neo4j and performing bulk ingestion using the `neo4rs` async driver.

---

## Overview

This document covers:
- **Isolation strategy**: Trait-based abstraction for easy updates
- Connection setup with `neo4rs`
- Connection pooling
- Bulk insert patterns with UNWIND
- Transaction management
- Error handling
- Performance optimization

---

## Isolation and Modularity Strategy

### Why Isolate Neo4j Write Logic?

**Problem**: Tightly coupled database code makes updates difficult
- Changing a query requires touching business logic
- Testing requires running Neo4j
- Swapping databases requires rewriting entire codebase
- Cypher queries scattered across multiple files

**Solution**: Trait-based abstraction with centralized queries
- ✅ ALL Neo4j operations isolated in `writer/neo4j/` module
- ✅ Domain logic uses `GraphWriter` trait (abstraction)
- ✅ ALL Cypher queries centralized in `writer/neo4j/queries.rs`
- ✅ Easy to swap implementations (Neo4j, mock, other DB)
- ✅ Test business logic without database

### GraphWriter Trait (The Contract)

**Location**: `src/writer/traits.rs`

This trait defines ALL database operations needed by the domain layer:

```rust
use async_trait::async_trait;
use anyhow::Result;

/// Database write abstraction
///
/// This trait isolates ALL database operations for:
/// - Easy query updates (change implementation, not interface)
/// - Testing without database (use MockWriter)
/// - Future flexibility (swap Neo4j for another DB)
#[async_trait]
pub trait GraphWriter: Send + Sync {
    /// Initialize schema (constraints, indexes)
    async fn init_schema(&self) -> Result<()>;

    /// Write block nodes in bulk (Phase 1)
    async fn write_blocks(&self, blocks: &[BlockData]) -> Result<()>;

    /// Write transaction nodes in bulk (Phase 2)
    async fn write_transactions(&self, txs: &[TransactionData]) -> Result<()>;

    /// Write output nodes and LOCKED_TO relationships in bulk (Phase 3)
    async fn write_outputs(&self, outputs: &[OutputData]) -> Result<()>;

    /// Write input nodes and SPENDS relationships in bulk (Phase 4)
    async fn write_inputs(&self, inputs: &[InputData]) -> Result<()>;

    /// Update transaction amounts (Phase 5)
    async fn calculate_amounts(&self, txs: &[TransactionData]) -> Result<()>;

    /// Create simplified BENEFITS_TO relationships (Phase 6)
    async fn write_simplified_layer(&self, txs: &[TransactionData]) -> Result<()>;

    /// Lookup output by ID (for UTXO cache misses)
    async fn lookup_output(&self, output_id: &str) -> Result<OutputData>;

    /// Mark output as spent
    async fn mark_output_spent(
        &self,
        output_id: &str,
        spent_in_txid: &str,
        spent_at_height: u32
    ) -> Result<()>;
}
```

### Domain Layer Usage (No Neo4j Knowledge)

**Location**: `src/domain/ingestion.rs`

Domain code uses the trait, never imports `neo4rs`:

```rust
use crate::writer::GraphWriter;  // Trait only
use std::sync::Arc;

pub struct IngestionOrchestrator<W: GraphWriter> {
    writer: Arc<W>,  // Generic over trait, not concrete type
    utxo_cache: UtxoCache<W>,
}

impl<W: GraphWriter> IngestionOrchestrator<W> {
    pub fn new(writer: Arc<W>) -> Self {
        Self {
            writer: writer.clone(),
            utxo_cache: UtxoCache::new(writer),
        }
    }

    /// Phase 1: Create block nodes
    pub async fn ingest_blocks(&self, blocks: Vec<Block>) -> Result<()> {
        let block_data: Vec<BlockData> = blocks
            .iter()
            .map(|b| BlockData::from_block(b))
            .collect();

        // Call trait method - no Neo4j-specific code
        self.writer.write_blocks(&block_data).await?;

        Ok(())
    }

    /// Phase 3: Create outputs
    pub async fn ingest_outputs(&mut self, txs: &[Transaction]) -> Result<()> {
        let output_data: Vec<OutputData> = extract_outputs(txs);

        // Write via trait
        self.writer.write_outputs(&output_data).await?;

        // Update cache
        for output in output_data {
            self.utxo_cache.insert(output.output_id.clone(), output);
        }

        Ok(())
    }
}
```

**Key Point**: Domain code is generic over `W: GraphWriter`. It works with ANY implementation (Neo4j, mock, future databases).

### Neo4j Implementation Structure

**Directory**: `src/writer/neo4j/`

```
writer/
├── mod.rs              # Re-exports
├── traits.rs           # GraphWriter trait definition
├── neo4j/
│   ├── mod.rs          # Neo4jWriter struct
│   ├── client.rs       # Connection pool setup
│   ├── schema.rs       # DDL operations (init_schema)
│   ├── queries.rs      # ⭐ ALL Cypher queries centralized
│   └── batch.rs        # Batch accumulator
└── mock.rs             # Mock implementation for testing
```

### Centralized Queries (Easy Updates)

**Location**: `src/writer/neo4j/queries.rs`

**ALL Cypher queries live in ONE file** - easy to find, review, and optimize:

```rust
// writer/neo4j/queries.rs

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

/// Phase 3: Create LOCKED_TO relationships
pub const CREATE_LOCKED_TO_QUERY: &str = r#"
    UNWIND $outputs AS out
    MATCH (o:Output {outputId: out.outputId})
    MERGE (a:Address {address: out.address})
    CREATE (o)-[:LOCKED_TO]->(a)
"#;

/// Lookup output by ID (for UTXO cache misses)
pub const LOOKUP_OUTPUT_QUERY: &str = r#"
    MATCH (o:Output {outputId: $outputId})
    OPTIONAL MATCH (o)-[:LOCKED_TO]->(a:Address)
    RETURN o.outputId as outputId,
           o.amount as amount,
           o.scriptType as scriptType,
           a.address as address
"#;

// ... all other queries
```

**Benefits**:
- ✅ **One place to update**: Change a query → edit one file
- ✅ **Easy review**: See all queries at once for optimization
- ✅ **Version control**: Track query changes in git
- ✅ **Testing**: Copy/paste into Neo4j Browser to test
- ✅ **Documentation**: Generate query docs from this file

### Neo4jWriter Implementation

**Location**: `src/writer/neo4j/mod.rs`

```rust
use neo4rs::{Graph, query};
use crate::writer::GraphWriter;
use async_trait::async_trait;

pub struct Neo4jWriter {
    graph: Graph,
}

impl Neo4jWriter {
    pub async fn new(uri: &str, user: &str, password: &str) -> Result<Self> {
        let graph = connect_neo4j(uri, user, password).await?;
        Ok(Self { graph })
    }
}

#[async_trait]
impl GraphWriter for Neo4jWriter {
    async fn write_blocks(&self, blocks: &[BlockData]) -> Result<()> {
        // Import query from centralized location
        use crate::writer::neo4j::queries::CREATE_BLOCKS_QUERY;

        let query = query(CREATE_BLOCKS_QUERY)
            .param("blocks", blocks_to_neo4j_list(blocks)?);

        self.graph.run(query).await?;
        Ok(())
    }

    async fn write_outputs(&self, outputs: &[OutputData]) -> Result<()> {
        use crate::writer::neo4j::queries::{
            CREATE_OUTPUTS_QUERY,
            CREATE_LOCKED_TO_QUERY
        };

        // Create output nodes
        let query = query(CREATE_OUTPUTS_QUERY)
            .param("outputs", outputs_to_neo4j_list(outputs)?);
        self.graph.run(query).await?;

        // Create LOCKED_TO relationships (for outputs with addresses)
        let outputs_with_address: Vec<_> = outputs
            .iter()
            .filter(|o| o.address.is_some())
            .collect();

        if !outputs_with_address.is_empty() {
            let query = query(CREATE_LOCKED_TO_QUERY)
                .param("outputs", outputs_to_neo4j_list(&outputs_with_address)?);
            self.graph.run(query).await?;
        }

        Ok(())
    }

    async fn lookup_output(&self, output_id: &str) -> Result<OutputData> {
        use crate::writer::neo4j::queries::LOOKUP_OUTPUT_QUERY;

        let mut result = self.graph.execute(
            query(LOOKUP_OUTPUT_QUERY).param("outputId", output_id)
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

    // ... implement other trait methods
}
```

### Mock Implementation (Testing Without Neo4j)

**Location**: `src/writer/mock.rs`

```rust
use crate::writer::GraphWriter;
use async_trait::async_trait;
use std::sync::Mutex;

/// Mock writer for testing - stores data in memory
pub struct MockWriter {
    blocks: Mutex<Vec<BlockData>>,
    outputs: Mutex<Vec<OutputData>>,
    // ... other collections
}

impl MockWriter {
    pub fn new() -> Self {
        Self {
            blocks: Mutex::new(Vec::new()),
            outputs: Mutex::new(Vec::new()),
        }
    }

    /// Get all outputs written (for test assertions)
    pub fn get_outputs(&self) -> Vec<OutputData> {
        self.outputs.lock().unwrap().clone()
    }
}

#[async_trait]
impl GraphWriter for MockWriter {
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

### Testing Without Neo4j

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::mock::MockWriter;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_ingestion_logic() {
        // Use mock writer - no Neo4j required!
        let writer = Arc::new(MockWriter::new());
        let orchestrator = IngestionOrchestrator::new(writer.clone());

        // Test business logic
        let blocks = vec![create_test_block()];
        orchestrator.ingest_blocks(blocks).await.unwrap();

        // Assert using mock methods
        let written_blocks = writer.get_blocks();
        assert_eq!(written_blocks.len(), 1);
        assert_eq!(written_blocks[0].height, 100);
    }
}
```

### Swapping Implementations (Dependency Injection)

**Location**: `src/main.rs`

```rust
use bitcoin_chain_graph::{
    domain::IngestionOrchestrator,
    writer::{neo4j::Neo4jWriter, mock::MockWriter},
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // Choose implementation at runtime
    let writer: Arc<dyn GraphWriter> = if cfg!(test) {
        Arc::new(MockWriter::new())
    } else {
        Arc::new(Neo4jWriter::new(
            "bolt://localhost:7687",
            "neo4j",
            "password"
        ).await?)
    };

    // Domain code doesn't care which implementation
    let orchestrator = IngestionOrchestrator::new(writer);

    // Run ingestion
    orchestrator.run().await?;

    Ok(())
}
```

### Benefits Summary

| Benefit | How Achieved |
|---------|-------------|
| **Easy query updates** | Edit `writer/neo4j/queries.rs` only |
| **Fast tests** | Use `MockWriter`, no Neo4j needed |
| **Database flexibility** | Implement trait for any DB |
| **Clean boundaries** | Domain never imports `neo4rs` |
| **Type safety** | Compiler enforces trait contract |
| **Parallel development** | Team can work on domain and writer separately |

---

## neo4rs Driver Basics

### Installation

```toml
[dependencies]
neo4rs = "0.7"
tokio = { version = "1.35", features = ["full"] }
```

### Basic Connection

```rust
use neo4rs::{Graph, ConfigBuilder};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let config = ConfigBuilder::default()
        .uri("bolt://localhost:7687")
        .user("neo4j")
        .password("password")
        .db("neo4j")
        .fetch_size(500)
        .max_connections(10)
        .build()?;

    let graph = Graph::connect(config).await?;

    // Test connection
    let mut result = graph.execute(query("RETURN 1 AS num")).await?;
    let row = result.next().await?.unwrap();
    let value: i64 = row.get("num")?;
    println!("Connection successful: {}", value);

    Ok(())
}
```

---

## Connection Pool Management

### Configuration

```rust
use neo4rs::{Graph, ConfigBuilder};

pub struct Neo4jClient {
    graph: Graph,
}

impl Neo4jClient {
    pub async fn new(uri: &str, user: &str, password: &str) -> Result<Self> {
        let config = ConfigBuilder::default()
            .uri(uri)
            .user(user)
            .password(password)
            .db("neo4j")
            // Connection pool settings
            .max_connections(10)          // Pool size
            .min_connections(2)            // Keep-alive connections
            .connection_timeout(std::time::Duration::from_secs(30))
            // Query settings
            .fetch_size(1000)              // Rows per fetch batch
            .max_retries(3)                // Retry failed queries
            .build()?;

        let graph = Graph::connect(config).await?;

        Ok(Self { graph })
    }

    /// Get reference to graph
    pub fn graph(&self) -> &Graph {
        &self.graph
    }
}
```

### Pool Size Guidelines

| Scenario | Pool Size | Notes |
|----------|-----------|-------|
| Single-threaded | 1-2 | Minimal overhead |
| Multi-threaded (2-4 workers) | 4-8 | One connection per worker + overhead |
| High concurrency (8+ workers) | 10-20 | Balance connections vs Neo4j load |

---

## Schema Initialization

### Create Constraints and Indexes

```rust
use neo4rs::query;

impl Neo4jClient {
    pub async fn init_schema(&self) -> Result<()> {
        tracing::info!("Initializing Neo4j schema...");

        // Unique constraints (also create indexes)
        let constraints = vec![
            "CREATE CONSTRAINT block_height_unique IF NOT EXISTS
             FOR (b:Block) REQUIRE b.height IS UNIQUE",

            "CREATE CONSTRAINT block_hash_unique IF NOT EXISTS
             FOR (b:Block) REQUIRE b.hash IS UNIQUE",

            "CREATE CONSTRAINT transaction_unique IF NOT EXISTS
             FOR (t:Transaction) REQUIRE t.txid IS UNIQUE",

            "CREATE CONSTRAINT output_unique IF NOT EXISTS
             FOR (o:Output) REQUIRE o.outputId IS UNIQUE",

            "CREATE CONSTRAINT input_unique IF NOT EXISTS
             FOR (i:Input) REQUIRE i.inputId IS UNIQUE",

            "CREATE CONSTRAINT address_unique IF NOT EXISTS
             FOR (a:Address) REQUIRE a.address IS UNIQUE",
        ];

        for constraint in constraints {
            self.graph.run(query(constraint)).await?;
        }

        // Additional indexes
        let indexes = vec![
            "CREATE INDEX transaction_timestamp IF NOT EXISTS
             FOR (t:Transaction) ON (t.timestamp)",

            "CREATE INDEX transaction_block IF NOT EXISTS
             FOR (t:Transaction) ON (t.blockHeight)",

            "CREATE INDEX transaction_coinbase IF NOT EXISTS
             FOR (t:Transaction) ON (t.isCoinbase)",

            "CREATE INDEX output_spent IF NOT EXISTS
             FOR (o:Output) ON (o.isSpent)",

            "CREATE INDEX output_amount IF NOT EXISTS
             FOR (o:Output) ON (o.amount)",

            "CREATE INDEX input_previous_tx IF NOT EXISTS
             FOR (i:Input) ON (i.previousTxid)",

            "CREATE INDEX address_type IF NOT EXISTS
             FOR (a:Address) ON (a.type)",

            "CREATE INDEX block_timestamp IF NOT EXISTS
             FOR (b:Block) ON (b.timestamp)",
        ];

        for index in indexes {
            self.graph.run(query(index)).await?;
        }

        tracing::info!("Schema initialization complete");
        Ok(())
    }
}
```

---

## Bulk Insert Patterns with UNWIND

### Pattern 1: Bulk Create Nodes

```rust
use neo4rs::{query, BoltMap};
use serde_json::json;

impl Neo4jClient {
    /// Bulk create block nodes
    pub async fn create_blocks(&self, blocks: Vec<BlockData>) -> Result<()> {
        let cypher = "
            UNWIND $blocks AS block
            CREATE (b:Block {
                height: block.height,
                hash: block.hash,
                previousHash: block.previousHash,
                merkleRoot: block.merkleRoot,
                timestamp: datetime(block.timestamp),
                txCount: block.txCount,
                size: block.size,
                weight: block.weight,
                bits: block.bits,
                difficulty: block.difficulty,
                nonce: block.nonce,
                version: block.version
            })
        ";

        // Convert blocks to neo4j format
        let blocks_data: Vec<BoltMap> = blocks
            .into_iter()
            .map(|b| {
                let mut map = BoltMap::new();
                map.put("height".into(), b.height.into());
                map.put("hash".into(), b.hash.into());
                map.put("previousHash".into(), b.previous_hash.into());
                map.put("merkleRoot".into(), b.merkle_root.into());
                map.put("timestamp".into(), b.timestamp.into());
                map.put("txCount".into(), b.tx_count.into());
                map.put("size".into(), b.size.into());
                map.put("weight".into(), b.weight.into());
                map.put("bits".into(), b.bits.into());
                map.put("difficulty".into(), b.difficulty.into());
                map.put("nonce".into(), b.nonce.into());
                map.put("version".into(), b.version.into());
                map
            })
            .collect();

        self.graph
            .run(query(cypher).param("blocks", blocks_data))
            .await?;

        Ok(())
    }

    /// Bulk create transaction nodes
    pub async fn create_transactions(&self, transactions: Vec<TransactionData>) -> Result<()> {
        let cypher = "
            UNWIND $transactions AS tx
            CREATE (t:Transaction {
                txid: tx.txid,
                blockHeight: tx.blockHeight,
                blockHash: tx.blockHash,
                timestamp: datetime(tx.timestamp),
                version: tx.version,
                locktime: tx.locktime,
                size: tx.size,
                vsize: tx.vsize,
                weight: tx.weight,
                isCoinbase: tx.isCoinbase,
                totalInput: tx.totalInput,
                totalOutput: tx.totalOutput,
                fee: tx.fee
            })
        ";

        let tx_data: Vec<BoltMap> = transactions
            .into_iter()
            .map(|tx| {
                // Convert TransactionData to BoltMap
                // (implementation omitted for brevity)
                todo!()
            })
            .collect();

        self.graph
            .run(query(cypher).param("transactions", tx_data))
            .await?;

        Ok(())
    }
}
```

### Pattern 2: Bulk Create Relationships

```rust
impl Neo4jClient {
    /// Bulk create NEXT_BLOCK relationships
    pub async fn link_blocks(&self, block_heights: Vec<(u32, u32)>) -> Result<()> {
        let cypher = "
            UNWIND $pairs AS pair
            MATCH (prev:Block {height: pair.prevHeight})
            MATCH (next:Block {height: pair.nextHeight})
            CREATE (prev)-[:NEXT_BLOCK]->(next)
        ";

        let pairs: Vec<BoltMap> = block_heights
            .into_iter()
            .map(|(prev, next)| {
                let mut map = BoltMap::new();
                map.put("prevHeight".into(), prev.into());
                map.put("nextHeight".into(), next.into());
                map
            })
            .collect();

        self.graph
            .run(query(cypher).param("pairs", pairs))
            .await?;

        Ok(())
    }

    /// Bulk create HAS_OUTPUT relationships
    pub async fn link_outputs_to_transactions(&self, outputs: Vec<OutputData>) -> Result<()> {
        let cypher = "
            UNWIND $outputs AS out
            MATCH (t:Transaction {txid: out.txid})
            MATCH (o:Output {outputId: out.outputId})
            CREATE (t)-[:HAS_OUTPUT]->(o)
        ";

        let output_data: Vec<BoltMap> = outputs
            .into_iter()
            .map(|out| {
                let mut map = BoltMap::new();
                map.put("txid".into(), out.txid.into());
                map.put("outputId".into(), out.output_id.into());
                map
            })
            .collect();

        self.graph
            .run(query(cypher).param("outputs", output_data))
            .await?;

        Ok(())
    }
}
```

### Pattern 3: Bulk Create Nodes + Relationships

```rust
impl Neo4jClient {
    /// Create outputs and LOCKED_TO relationships in single query
    pub async fn create_outputs_with_addresses(&self, outputs: Vec<OutputData>) -> Result<()> {
        let cypher = "
            UNWIND $outputs AS out

            // Create output node
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

            // Link to transaction
            WITH o, out
            MATCH (t:Transaction {txid: out.txid})
            CREATE (t)-[:HAS_OUTPUT]->(o)

            // Create or link to address (if address exists)
            WITH o, out
            WHERE out.address IS NOT NULL
            MERGE (a:Address {address: out.address})
            ON CREATE SET a.type = out.addressType
            CREATE (o)-[:LOCKED_TO]->(a)
        ";

        let output_data: Vec<BoltMap> = outputs
            .into_iter()
            .map(|out| {
                let mut map = BoltMap::new();
                map.put("outputId".into(), out.output_id.into());
                map.put("outputIndex".into(), out.output_index.into());
                map.put("amount".into(), out.amount.into());
                map.put("scriptPubKey".into(), hex::encode(out.script_pubkey).into());
                map.put("scriptType".into(), out.script_type.into());
                map.put("txid".into(), out.txid.into());
                if let Some(addr) = out.address {
                    map.put("address".into(), addr.address.into());
                    map.put("addressType".into(), addr.address_type.into());
                }
                map
            })
            .collect();

        self.graph
            .run(query(cypher).param("outputs", output_data))
            .await?;

        Ok(())
    }
}
```

---

## Transaction Management

### Neo4j Transaction Pattern

```rust
impl Neo4jClient {
    /// Execute multiple operations in single Neo4j transaction
    pub async fn ingest_block_batch(&self, batch: BatchData) -> Result<()> {
        // Start Neo4j transaction
        let mut txn = self.graph.start_txn().await?;

        // Phase 1: Create blocks
        txn.run(query("...").param("blocks", batch.blocks)).await?;

        // Phase 2: Create transactions
        txn.run(query("...").param("transactions", batch.transactions)).await?;

        // Phase 3: Create outputs
        txn.run(query("...").param("outputs", batch.outputs)).await?;

        // Phase 4: Create inputs
        txn.run(query("...").param("inputs", batch.inputs)).await?;

        // Commit transaction (all or nothing)
        txn.commit().await?;

        Ok(())
    }
}
```

### Error Handling and Rollback

```rust
impl Neo4jClient {
    pub async fn ingest_with_retry(&self, batch: BatchData) -> Result<()> {
        let max_retries = 3;
        let mut attempt = 0;

        loop {
            match self.ingest_block_batch(batch.clone()).await {
                Ok(_) => return Ok(()),
                Err(e) if attempt < max_retries => {
                    attempt += 1;
                    tracing::warn!(
                        "Ingestion failed (attempt {}/{}): {}",
                        attempt, max_retries, e
                    );
                    // Exponential backoff
                    tokio::time::sleep(tokio::time::Duration::from_secs(2_u64.pow(attempt))).await;
                }
                Err(e) => return Err(e),
            }
        }
    }
}
```

---

## Query Execution Patterns

### Pattern 1: Simple Query (No Results)

```rust
impl Neo4jClient {
    pub async fn mark_outputs_spent(&self, output_ids: Vec<String>, txid: &str, height: u32) -> Result<()> {
        let cypher = "
            UNWIND $outputIds AS outputId
            MATCH (o:Output {outputId: outputId})
            SET o.isSpent = true,
                o.spentInTxid = $txid,
                o.spentAtHeight = $height
        ";

        self.graph
            .run(query(cypher)
                .param("outputIds", output_ids)
                .param("txid", txid)
                .param("height", height))
            .await?;

        Ok(())
    }
}
```

### Pattern 2: Query with Results

```rust
use neo4rs::RowStream;

impl Neo4jClient {
    pub async fn get_output(&self, output_id: &str) -> Result<Option<CachedOutput>> {
        let cypher = "
            MATCH (o:Output {outputId: $outputId})
            OPTIONAL MATCH (o)-[:LOCKED_TO]->(a:Address)
            RETURN o.outputId as outputId,
                   o.amount as amount,
                   o.scriptPubKey as scriptPubKey,
                   a.address as address
        ";

        let mut result = self.graph
            .execute(query(cypher).param("outputId", output_id))
            .await?;

        if let Some(row) = result.next().await? {
            Ok(Some(CachedOutput {
                output_id: row.get("outputId")?,
                amount: row.get("amount")?,
                script_pubkey: hex::decode(row.get::<String>("scriptPubKey")?)?,
                address: row.get("address").ok(),
            }))
        } else {
            Ok(None)
        }
    }
}
```

### Pattern 3: Streaming Results

```rust
impl Neo4jClient {
    pub async fn stream_all_utxos(&self) -> Result<Vec<OutputData>> {
        let cypher = "
            MATCH (o:Output {isSpent: false})
            RETURN o.outputId as outputId,
                   o.amount as amount
            ORDER BY o.amount DESC
        ";

        let mut result = self.graph.execute(query(cypher)).await?;
        let mut utxos = Vec::new();

        // Stream results
        while let Some(row) = result.next().await? {
            utxos.push(OutputData {
                output_id: row.get("outputId")?,
                amount: row.get("amount")?,
                // ... other fields
            });

            // Prevent memory overflow for large result sets
            if utxos.len() >= 100_000 {
                break;
            }
        }

        Ok(utxos)
    }
}
```

---

## Complete Ingestion Implementation

### Full Example

```rust
use bitcoin::Block;
use neo4rs::Graph;

pub struct BlockIngestion {
    neo4j_client: Neo4jClient,
    utxo_cache: UtxoCache,
}

impl BlockIngestion {
    pub async fn ingest_block(&mut self, block: &Block, height: u32) -> Result<()> {
        // Phase 1: Create block node
        self.create_block_node(block, height).await?;

        // Phase 2: Create transaction nodes
        self.create_transaction_nodes(block, height).await?;

        // Phase 3: Create outputs + addresses
        self.create_outputs(block).await?;

        // Phase 4: Create inputs + SPENDS relationships
        self.create_inputs(block, height).await?;

        // Phase 5: Calculate transaction amounts
        self.calculate_transaction_amounts(block).await?;

        // Phase 6: Create simplified layer (PERFORMS, BENEFITS_TO)
        self.create_simplified_layer(block).await?;

        Ok(())
    }

    async fn create_block_node(&self, block: &Block, height: u32) -> Result<()> {
        let cypher = "
            CREATE (b:Block {
                height: $height,
                hash: $hash,
                previousHash: $previousHash,
                merkleRoot: $merkleRoot,
                timestamp: datetime($timestamp),
                txCount: $txCount,
                size: $size,
                weight: $weight,
                bits: $bits,
                difficulty: $difficulty,
                nonce: $nonce,
                version: $version
            })
        ";

        self.neo4j_client.graph()
            .run(query(cypher)
                .param("height", height)
                .param("hash", block.block_hash().to_string())
                .param("previousHash", block.header.prev_blockhash.to_string())
                .param("merkleRoot", block.header.merkle_root.to_string())
                .param("timestamp", block.header.time as i64)
                .param("txCount", block.txdata.len() as i64)
                // ... other params
            )
            .await?;

        Ok(())
    }

    // Implement other phases...
}
```

---

## Performance Optimization

### Batch Size Tuning

```rust
// Small batches (slower, less memory)
const BATCH_SIZE: usize = 10;

// Large batches (faster, more memory)
const BATCH_SIZE: usize = 100;

// Recommended
const BATCH_SIZE: usize = 50;
```

### Parallel Batches

```rust
use tokio::task::JoinSet;

pub async fn ingest_batches_parallel(
    client: Neo4jClient,
    batches: Vec<BatchData>
) -> Result<()> {
    let mut tasks = JoinSet::new();

    for batch in batches {
        let client = client.clone();
        tasks.spawn(async move {
            client.ingest_block_batch(batch).await
        });
    }

    // Wait for all tasks
    while let Some(result) = tasks.join_next().await {
        result??;
    }

    Ok(())
}
```

---

## Error Handling

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Neo4jError {
    #[error("Neo4j connection error: {0}")]
    Connection(#[from] neo4rs::Error),

    #[error("Query execution failed: {0}")]
    QueryExecution(String),

    #[error("Transaction rollback: {0}")]
    TransactionRollback(String),

    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),
}

// Usage
match neo4j_client.create_blocks(blocks).await {
    Ok(_) => {},
    Err(Neo4jError::ConstraintViolation(msg)) => {
        // Handle duplicate blocks
        tracing::warn!("Duplicate block: {}", msg);
    },
    Err(e) => return Err(e),
}
```

---

## Testing Neo4j Integration

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use testcontainers::*;

    #[tokio::test]
    async fn test_neo4j_connection() {
        let container = clients::Cli::default()
            .run(images::neo4j::Neo4j::default());

        let port = container.get_host_port_ipv4(7687);
        let uri = format!("bolt://localhost:{}", port);

        let client = Neo4jClient::new(&uri, "neo4j", "test").await.unwrap();
        client.init_schema().await.unwrap();

        // Test schema creation
        let result = client.graph()
            .execute(query("SHOW CONSTRAINTS"))
            .await
            .unwrap();

        // Verify constraints exist
        // ...
    }
}
```

---

## References

- [neo4rs Documentation](https://docs.rs/neo4rs/latest/neo4rs/)
- [Neo4j Cypher Manual](https://neo4j.com/docs/cypher-manual/current/)
- [CYPHER_EXAMPLES.md](../neo4j/CYPHER_EXAMPLES.md) - Cypher query patterns

---

## Next Steps

1. Read [PERFORMANCE.md](PERFORMANCE.md) for bulk insert optimization
2. Read [PARALLELISM.md](PARALLELISM.md) for concurrent ingestion patterns
3. Read [TESTING.md](TESTING.md) for integration testing strategies
