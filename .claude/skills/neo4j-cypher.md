---
name: neo4j-cypher
description: Neo4j Cypher patterns for the Bitcoin Chain Graph data model
---

# Neo4j Cypher — Bitcoin Chain Graph

## Data Model

### Node Labels and Key Properties

| Label | Key Property | Other Properties |
|-------|-------------|------------------|
| Block | height (unique), hash (unique) | previousHash, merkleRoot, timestamp, bits, difficulty, nonce, version, txCount, size, weight |
| Transaction | txid (unique) | blockHeight, blockHash, timestamp, version, locktime, size, vsize, weight, isCoinbase, totalInput, totalOutput, fee |
| Output | outputId (unique, "{txid}:{index}") | outputIndex, amount, scriptPubKey, scriptType, isSpent, spentInTxid, spentAtHeight |
| Input | inputId (unique, "{txid}:{index}") | inputIndex, scriptSig, sequence, witness |
| Address | address (unique) | (no other properties) |
| IngestionCheckpoint | (singleton) | lastProcessedHeight, lastProcessedHash, lastProcessedFile, status, timestamp |

### Relationships

```
(:Block)-[:NEXT_BLOCK]->(:Block)
(:Transaction)-[:INCLUDED_IN]->(:Block)
(:Transaction)-[:HAS_OUTPUT]->(:Output)
(:Transaction)-[:HAS_INPUT]->(:Input)
(:Input)-[:SPENDS]->(:Output)
(:Output)-[:LOCKED_TO]->(:Address)
(:Address)-[:PERFORMS]->(:Transaction)    # with inputCount, amountSpent
(:Transaction)-[:BENEFITS_TO]->(:Address) # with outputCount, amountReceived
```

### Unique Constraints

```cypher
CREATE CONSTRAINT block_height IF NOT EXISTS FOR (b:Block) REQUIRE b.height IS UNIQUE;
CREATE CONSTRAINT block_hash IF NOT EXISTS FOR (b:Block) REQUIRE b.hash IS UNIQUE;
CREATE CONSTRAINT tx_txid IF NOT EXISTS FOR (t:Transaction) REQUIRE t.txid IS UNIQUE;
CREATE CONSTRAINT output_id IF NOT EXISTS FOR (o:Output) REQUIRE o.outputId IS UNIQUE;
CREATE CONSTRAINT input_id IF NOT EXISTS FOR (i:Input) REQUIRE i.inputId IS UNIQUE;
CREATE CONSTRAINT address IF NOT EXISTS FOR (a:Address) REQUIRE a.address IS UNIQUE;
```

## Query Patterns

### Bulk Writes with UNWIND (required pattern)

```cypher
// Create nodes
UNWIND $items AS item
CREATE (n:Label {id: item.id})
SET n.prop1 = item.prop1, n.prop2 = item.prop2

// Idempotent (for reprocessing)
UNWIND $items AS item
MERGE (n:Label {id: item.id})
SET n.prop1 = item.prop1, n.prop2 = item.prop2
```

### Relationship Creation

```cypher
UNWIND $data AS d
MATCH (a:Label1 {key: d.fromKey})
MATCH (b:Label2 {key: d.toKey})
MERGE (a)-[r:REL_TYPE]->(b)
SET r.prop = d.prop
```

### Rollback Pattern (reverse order)

```cypher
// 1. Revert spent outputs
MATCH (b:Block {height: $height})-[:INCLUDED_IN]-(tx:Transaction)
MATCH (tx)-[:HAS_INPUT]->(:Input)-[:SPENDS]->(o:Output)
SET o.isSpent = false, o.spentInTxid = null, o.spentAtHeight = null

// 2. Delete inputs
MATCH (b:Block {height: $height})-[:INCLUDED_IN]-(tx:Transaction)-[:HAS_INPUT]->(i:Input)
DETACH DELETE i

// 3. Delete outputs
MATCH (b:Block {height: $height})-[:INCLUDED_IN]-(tx:Transaction)-[:HAS_OUTPUT]->(o:Output)
DETACH DELETE o

// 4. Delete transactions
MATCH (b:Block {height: $height})-[:INCLUDED_IN]-(tx:Transaction)
DETACH DELETE tx

// 5. Delete block
MATCH (b:Block {height: $height})
DETACH DELETE b
```

## Rules for Adding New Queries

1. Define as `pub const` in `src/writer/neo4j/queries.rs`
2. Document with `///` comments including parameter descriptions
3. Always parameterize — never use string interpolation
4. Use UNWIND for any operation on multiple items
5. Use `IF NOT EXISTS` for constraint/index creation
6. MERGE for idempotent operations, CREATE for fast forward-only ingestion
7. All integer parameters must be i64 (cast from u32/u64 in Rust)
