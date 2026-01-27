# Cypher Query Examples for Bitcoin Blockchain

Complete Cypher query patterns for ingesting Bitcoin blockchain data into Neo4j and querying the resulting graph.

All amounts are stored as **integers in satoshis** (1 BTC = 100,000,000 satoshis).

---

## Table of Contents

1. [Schema Setup (Constraints & Indexes)](#schema-setup)
2. [Checkpoint Management](#checkpoint-management)
3. [Phase 1: Block Ingestion](#phase-1-block-ingestion)
4. [Phase 2: Output Ingestion](#phase-2-output-ingestion)
5. [Phase 3: Transaction Ingestion (with Amounts)](#phase-3-transaction-ingestion-with-amounts)
6. [Phase 4: Input Ingestion](#phase-4-input-ingestion)
7. [Phase 5: (Removed)](#phase-5-removed)
8. [Phase 6: Simplified Layer (Bulk)](#phase-6-simplified-layer-bulk)
9. [UTXO Lookup Queries](#utxo-lookup-queries)
10. [Query Examples - Simplified Layer](#query-examples-simplified-layer)
11. [Query Examples - Detailed UTXO Layer](#query-examples-detailed-utxo-layer)
12. [Analysis Queries](#analysis-queries)

---

## Schema Setup

**Run these BEFORE ingesting any data.**

The application creates these automatically via `init_schema()` in `src/writer/neo4j/schema.rs`.

```cypher
// ============================================
// CONSTRAINTS (enforce uniqueness) — 6 total
// ============================================

// Block constraints
CREATE CONSTRAINT block_height_unique IF NOT EXISTS
FOR (b:Block) REQUIRE b.height IS UNIQUE;

CREATE CONSTRAINT block_hash_unique IF NOT EXISTS
FOR (b:Block) REQUIRE b.hash IS UNIQUE;

// Transaction constraints
CREATE CONSTRAINT transaction_unique IF NOT EXISTS
FOR (t:Transaction) REQUIRE t.txid IS UNIQUE;

// Output constraints
CREATE CONSTRAINT output_unique IF NOT EXISTS
FOR (o:Output) REQUIRE o.outputId IS UNIQUE;

// Input constraints
CREATE CONSTRAINT input_unique IF NOT EXISTS
FOR (i:Input) REQUIRE i.inputId IS UNIQUE;

// Address constraints
CREATE CONSTRAINT address_unique IF NOT EXISTS
FOR (a:Address) REQUIRE a.address IS UNIQUE;

// ============================================
// INDEXES (improve query performance) — 7 total
// ============================================

// Transaction indexes
CREATE INDEX transaction_timestamp IF NOT EXISTS
FOR (t:Transaction) ON (t.timestamp);

CREATE INDEX transaction_block IF NOT EXISTS
FOR (t:Transaction) ON (t.blockHeight);

CREATE INDEX transaction_coinbase IF NOT EXISTS
FOR (t:Transaction) ON (t.isCoinbase);

// Output indexes
CREATE INDEX output_spent IF NOT EXISTS
FOR (o:Output) ON (o.isSpent);

CREATE INDEX output_amount IF NOT EXISTS
FOR (o:Output) ON (o.amount);

CREATE INDEX output_script_type IF NOT EXISTS
FOR (o:Output) ON (o.scriptType);

// Block indexes
CREATE INDEX block_timestamp IF NOT EXISTS
FOR (b:Block) ON (b.timestamp);
```

---

## Checkpoint Management

Queries for managing ingestion checkpoints to enable resume-on-failure.

These match the constants in `src/writer/neo4j/queries.rs`.

### Create Initial Checkpoint

```cypher
// Create checkpoint before starting ingestion
// Uses sentinel -999 (not -1) to avoid neo4rs driver bug that misreads -1 as 255
CREATE (c:IngestionCheckpoint {
  lastProcessedHeight: -999,
  lastProcessedHash: '0000000000000000000000000000000000000000000000000000000000000000',
  lastProcessedFile: 'blk00000.dat',
  lastProcessedFileOffset: 0,
  timestamp: datetime(),
  status: 'in_progress'
})
```

### Update Checkpoint After Successful Block

```cypher
// Update checkpoint after successfully ingesting a block
// Uses MERGE to guarantee the checkpoint node exists
MERGE (c:IngestionCheckpoint)
SET c.lastProcessedHeight = $height,
    c.lastProcessedHash = $hash,
    c.lastProcessedFile = $file,
    c.lastProcessedFileOffset = $offset,
    c.timestamp = datetime(),
    c.status = $status
```

**Parameters:**
- `$height` - Block height just processed (e.g., 0 for genesis)
- `$hash` - Block hash (e.g., "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f")
- `$file` - Name of .blk file or source identifier (e.g., "blk00000.dat", "rpc")
- `$offset` - Byte offset in file after this block
- `$status` - Checkpoint status (e.g., "in_progress")

### Query Checkpoint for Resume

```cypher
// Get checkpoint state to resume ingestion
MATCH (c:IngestionCheckpoint)
RETURN c.lastProcessedHeight AS lastProcessedHeight,
       c.lastProcessedHash AS lastProcessedHash,
       c.lastProcessedFile AS lastProcessedFile,
       c.lastProcessedFileOffset AS lastProcessedFileOffset,
       c.timestamp AS timestamp,
       c.status AS status
```

**Resume logic:**
- If `lastProcessedHeight = -999`: Start from genesis block (block 0)
- If `lastProcessedHeight >= 0`: Resume from block `lastProcessedHeight + 1`

### Verify Last Processed Block (Optional)

```cypher
// Verify last processed block exists in database with correct hash
MATCH (c:IngestionCheckpoint)
MATCH (b:Block {height: c.lastProcessedHeight})
WHERE b.hash = c.lastProcessedHash
RETURN b.height AS verifiedHeight,
       b.hash AS verifiedHash,
       "VERIFIED" AS status

UNION

// If block doesn't exist or hash mismatch, return error
MATCH (c:IngestionCheckpoint)
WHERE NOT EXISTS {
  MATCH (b:Block {height: c.lastProcessedHeight})
  WHERE b.hash = c.lastProcessedHash
}
RETURN c.lastProcessedHeight AS verifiedHeight,
       c.lastProcessedHash AS verifiedHash,
       "MISMATCH" AS status
```

### Set Checkpoint Status

```cypher
// Set checkpoint status (used for complete, paused, error states)
MATCH (c:IngestionCheckpoint)
SET c.status = $status,
    c.timestamp = datetime()
```

**Status values:** `"in_progress"`, `"completed"`, `"paused"`, `"error"`

### Mark Ingestion Complete

```cypher
// Mark ingestion as completed
MATCH (c:IngestionCheckpoint)
SET c.status = 'completed',
    c.timestamp = datetime()
```

### Reset Checkpoint (Start Fresh)

```cypher
// Delete existing checkpoint to start fresh ingestion
MATCH (c:IngestionCheckpoint) DELETE c
```

Then create a new checkpoint with initial values.

### Query Ingestion Progress

```cypher
// Get ingestion progress summary
MATCH (c:IngestionCheckpoint)
OPTIONAL MATCH (b:Block)
WITH c, max(b.height) AS maxBlockInDb
RETURN c.lastProcessedHeight AS lastProcessed,
       c.lastProcessedFile AS currentFile,
       c.status AS status,
       c.timestamp AS lastUpdate,
       maxBlockInDb,
       maxBlockInDb = c.lastProcessedHeight AS isSynced
```

---

## Phase 1: Block Ingestion

### Actual Query (from queries.rs: `CREATE_BLOCKS_QUERY`)

The application uses UNWIND for batch operations and MERGE for idempotency:

```cypher
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
```

**Parameters:**
- `$blocks` - List of block objects with all properties

**Notes:**
- Uses `MERGE` on `hash` (unique identifier) for idempotent reprocessing
- `NEXT_BLOCK` relationship is created via `FOREACH` conditional pattern
- Genesis block (height 0) skips NEXT_BLOCK creation via `WHERE block.height > 0`
- Timestamp is stored as `datetime` converted from Unix epoch seconds

### Genesis Block Properties

```
height: 0
hash: "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f"
previousHash: "0000000000000000000000000000000000000000000000000000000000000000"
merkleRoot: "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b"
timestamp: 1231006505 (2009-01-03T18:15:05Z)
txCount: 1
size: 285
weight: 1140
bits: "1d00ffff"
difficulty: 1.0
nonce: 2083236893
version: 1
```

---

## Phase 2: Output Ingestion

Outputs are ingested BEFORE transactions to support same-block UTXO references (Bitcoin allows spending outputs from earlier transactions in the same block).

### Actual Query: Create Outputs (from queries.rs: `CREATE_OUTPUTS_QUERY`)

```cypher
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
```

**Parameters:**
- `$outputs` - List of output objects with `outputId`, `outputIndex`, `txid`, `amount` (satoshis), `scriptPubKey`, `scriptType`

**Notes:**
- Uses `ON CREATE` / `ON MATCH` to preserve `isSpent` state on reprocessing
- `amount` is in satoshis (INTEGER, not FLOAT)
- `scriptType` values: `P2PKH`, `P2SH`, `P2WPKH`, `P2WSH`, `P2TR`, `P2PK`, `NULL_DATA`, `UNKNOWN`

### Actual Query: Create LOCKED_TO Relationships (from queries.rs: `CREATE_LOCKED_TO_QUERY`)

Run separately for outputs that have derivable addresses:

```cypher
UNWIND $outputs AS out
MATCH (o:Output {outputId: out.outputId})
MERGE (a:Address {address: out.address})
MERGE (o)-[:LOCKED_TO]->(a)
```

**Parameters:**
- `$outputs` - Filtered list of outputs that have an `address` field (excludes NULL_DATA and UNKNOWN)

**Notes:**
- Address nodes have only the `address` property (no `type` property is stored)
- NULL_DATA (OP_RETURN) outputs are excluded — they have no LOCKED_TO relationship

---

## Phase 3: Transaction Ingestion (with Amounts)

Transactions are created WITH pre-calculated amounts. The `totalInput`, `totalOutput`, and `fee` fields are calculated in Rust using the UTXO cache during ingestion, avoiding expensive Neo4j graph traversals.

### Actual Query (from queries.rs: `CREATE_TRANSACTIONS_QUERY`)

```cypher
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
```

**Parameters:**
- `$transactions` - List of transaction objects with ALL properties including amounts

**Amount Calculation (done in Rust, not Cypher):**
- `totalOutput` = sum of all output amounts (satoshis)
- `totalInput` = sum of all input amounts from UTXO cache lookups (satoshis), with Neo4j fallback on cache miss
- `fee` = `totalInput - totalOutput` (using `saturating_sub` to avoid underflow)
- Coinbase transactions: `totalInput = 0`, `fee = 0`

---

## Phase 4: Input Ingestion

### Actual Query (from queries.rs: `CREATE_INPUTS_QUERY`)

```cypher
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
```

**Parameters:**
- `$inputs` - List of input objects with `inputId`, `inputIndex`, `txid`, `previousTxid`, `previousOutputIndex`, `scriptSig`, `sequence`, `witness` (array), `blockHeight`

**Notes:**
- `HAS_INPUT` direction is `(Transaction)-[:HAS_INPUT]->(Input)` (Transaction owns Input)
- `previousTxid` and `previousOutputIndex` are used for the SPENDS lookup but NOT stored as Input node properties
- Coinbase inputs (where `previousOutputIndex = 4294967295`) skip SPENDS creation
- Spent outputs are updated with `isSpent = true`, `spentInTxid`, and `spentAtHeight`

---

## Phase 5: (Removed)

Phase 5 (Calculate Transaction Amounts) has been removed. Transaction amounts (`totalInput`, `totalOutput`, `fee`) are now calculated in Rust during Phase 3 using an in-memory UTXO cache, avoiding expensive Neo4j graph traversals. This provides a 10-100x performance improvement over the previous approach of running 3 Cypher queries per block.

See [INGESTION_ARCHITECTURE.md](../architecture/INGESTION_ARCHITECTURE.md) for details on the UTXO cache-based amount calculation.

---

## Phase 6: Simplified Layer (Bulk)

The simplified layer creates PERFORMS and BENEFITS_TO relationships using pre-aggregated data calculated in Rust, not Neo4j graph traversals.

### Actual Query: PERFORMS (from queries.rs: `CREATE_PERFORMS_BULK_QUERY`)

```cypher
UNWIND $performs AS p
MERGE (addr:Address {address: p.fromAddress})
WITH addr, p
MATCH (t:Transaction {txid: p.toTxid})
MERGE (addr)-[r:PERFORMS]->(t)
SET r.inputCount = p.inputCount,
    r.amountSpent = p.amountSpent
```

**Parameters:**
- `$performs` - List of `{fromAddress, toTxid, inputCount, amountSpent}` pre-aggregated in Rust

### Actual Query: BENEFITS_TO (from queries.rs: `CREATE_BENEFITS_TO_BULK_QUERY`)

```cypher
UNWIND $benefitsTo AS b
MATCH (t:Transaction {txid: b.fromTxid})
WITH t, b
MERGE (addr:Address {address: b.toAddress})
MERGE (t)-[r:BENEFITS_TO]->(addr)
SET r.outputCount = b.outputCount,
    r.amountReceived = b.amountReceived
```

**Parameters:**
- `$benefitsTo` - List of `{fromTxid, toAddress, outputCount, amountReceived}` pre-aggregated in Rust

**Notes:**
- PERFORMS relationships have properties: `inputCount` (INTEGER), `amountSpent` (INTEGER, satoshis)
- BENEFITS_TO relationships have properties: `outputCount` (INTEGER), `amountReceived` (INTEGER, satoshis)
- Data is aggregated per (address, transaction) pair in Rust to avoid duplicates
- In batch mode, writes are partitioned into 4 address-hash-based buckets to avoid Neo4j deadlocks

---

## UTXO Lookup Queries

Used by the UTXO cache when a cache miss occurs during amount calculation.

### Single Output Lookup (from queries.rs: `LOOKUP_OUTPUT_QUERY`)

```cypher
MATCH (o:Output {outputId: $outputId})
OPTIONAL MATCH (o)-[:LOCKED_TO]->(a:Address)
RETURN o.outputId AS outputId,
       o.outputIndex AS outputIndex,
       o.amount AS amount,
       o.scriptPubKey AS scriptPubKey,
       o.scriptType AS scriptType,
       a.address AS address
```

### Batch Output Lookup (from queries.rs: `LOOKUP_OUTPUTS_BATCH_QUERY`)

```cypher
UNWIND $outputIds AS oid
MATCH (o:Output {outputId: oid})
OPTIONAL MATCH (o)-[:LOCKED_TO]->(a:Address)
RETURN o.outputId AS outputId,
       o.outputIndex AS outputIndex,
       o.amount AS amount,
       o.scriptPubKey AS scriptPubKey,
       o.scriptType AS scriptType,
       a.address AS address
```

**Notes:**
- Batch lookup reduces N round-trips to 1 UNWIND query
- Outputs not found are silently skipped (MATCH filters them out)
- Used by `get_many_with_fallback()` in the UTXO cache

### Mark Output as Spent (from queries.rs: `MARK_OUTPUT_SPENT_QUERY`)

```cypher
MATCH (o:Output {outputId: $outputId})
SET o.isSpent = true,
    o.spentInTxid = $spentInTxid,
    o.spentAtHeight = $spentAtHeight
```

---

## Query Examples - Simplified Layer

### Follow the Money: Trace Funds from Address A to Address B

```cypher
// Find shortest path between two addresses
MATCH path = shortestPath(
  (alice:Address {address: $aliceAddress})
  -[:PERFORMS|BENEFITS_TO*1..10]-
  (bob:Address {address: $bobAddress})
)
RETURN path
```

### Find All Transactions Performed by an Address

```cypher
MATCH (addr:Address {address: $address})-[:PERFORMS]->(t:Transaction)
RETURN t
ORDER BY t.timestamp DESC
LIMIT 100
```

### Find All Transactions Benefiting an Address

```cypher
MATCH (t:Transaction)-[:BENEFITS_TO]->(addr:Address {address: $address})
RETURN t
ORDER BY t.timestamp DESC
LIMIT 100
```

### Get Address Transaction History (Both Sent and Received)

```cypher
MATCH (addr:Address {address: $address})

// Outgoing transactions (sent)
OPTIONAL MATCH (addr)-[:PERFORMS]->(sentTx:Transaction)

// Incoming transactions (received)
OPTIONAL MATCH (receivedTx:Transaction)-[:BENEFITS_TO]->(addr)

RETURN addr,
       collect(DISTINCT sentTx) AS sent,
       collect(DISTINCT receivedTx) AS received
```

### Find First-Degree Connections (Who did this address transact with?)

```cypher
// Find addresses this address sent to
MATCH (addr:Address {address: $address})-[:PERFORMS]->(t:Transaction)-[:BENEFITS_TO]->(recipient:Address)
WHERE recipient <> addr  // Exclude change outputs back to self
RETURN DISTINCT recipient.address AS sentTo,
       count(t) AS transactionCount,
       sum(t.totalOutput) AS totalAmount  // satoshis
ORDER BY transactionCount DESC
LIMIT 50
```

---

## Query Examples - Detailed UTXO Layer

### Find All Unspent Outputs for an Address

```cypher
MATCH (addr:Address {address: $address})<-[:LOCKED_TO]-(o:Output)
WHERE o.isSpent = false
RETURN o.outputId, o.amount, o.scriptType  // amount in satoshis
ORDER BY o.amount DESC
```

### Calculate Address Balance

```cypher
MATCH (addr:Address {address: $address})<-[:LOCKED_TO]-(o:Output {isSpent: false})
RETURN addr.address, sum(o.amount) AS balanceSatoshis
```

### Trace UTXO Spend Chain

```cypher
// Follow a specific output through its spend chain
MATCH path = (original:Output {outputId: $outputId})
  <-[:SPENDS*0..10]-(descendant:Input)
RETURN path
```

### Find Transaction Details with Full UTXO Information

```cypher
MATCH (t:Transaction {txid: $txid})

// Get inputs and the outputs they spent
// Note: HAS_INPUT direction is (Transaction)-[:HAS_INPUT]->(Input)
OPTIONAL MATCH (t)-[:HAS_INPUT]->(i:Input)-[:SPENDS]->(prevOut:Output)-[:LOCKED_TO]->(inputAddr:Address)

// Get outputs and their destination addresses
OPTIONAL MATCH (t)-[:HAS_OUTPUT]->(o:Output)-[:LOCKED_TO]->(outputAddr:Address)

RETURN t,
       collect(DISTINCT {
         input: i,
         spentOutput: prevOut,
         fromAddress: inputAddr
       }) AS inputs,
       collect(DISTINCT {
         output: o,
         toAddress: outputAddr
       }) AS outputs
```

### Find Change Outputs (Heuristic: Same address appears in input and output)

```cypher
// Common pattern: Change goes back to sender
MATCH (t:Transaction)
MATCH (t)-[:HAS_INPUT]->(i:Input)-[:SPENDS]->(prevOut:Output)-[:LOCKED_TO]->(addr:Address)
MATCH (t)-[:HAS_OUTPUT]->(changeOut:Output)-[:LOCKED_TO]->(addr)
RETURN t, addr, changeOut
LIMIT 100
```

---

## Analysis Queries

### Get Block Summary

```cypher
MATCH (b:Block {height: $height})
OPTIONAL MATCH (b)<-[:INCLUDED_IN]-(t:Transaction)
RETURN b,
       count(t) AS transactionCount,
       sum(t.totalOutput) AS totalBlockValueSatoshis,
       sum(t.fee) AS totalFeesSatoshis
```

### Find Largest Transactions in Date Range

```cypher
MATCH (t:Transaction)
WHERE t.timestamp >= datetime($startDate)
  AND t.timestamp <= datetime($endDate)
  AND t.isCoinbase = false
RETURN t.txid, t.totalOutput, t.timestamp  // totalOutput in satoshis
ORDER BY t.totalOutput DESC
LIMIT 100
```

### Find Most Active Addresses (by Transaction Count)

```cypher
// Count both sent and received transactions
MATCH (addr:Address)

OPTIONAL MATCH (addr)-[:PERFORMS]->(sentTx:Transaction)
OPTIONAL MATCH (receivedTx:Transaction)-[:BENEFITS_TO]->(addr)

WITH addr,
     count(DISTINCT sentTx) AS sentCount,
     count(DISTINCT receivedTx) AS receivedCount

RETURN addr.address,
       sentCount,
       receivedCount,
       sentCount + receivedCount AS totalTxCount
ORDER BY totalTxCount DESC
LIMIT 100
```

### Find Addresses with Large Balances

```cypher
MATCH (addr:Address)<-[:LOCKED_TO]-(o:Output {isSpent: false})
WITH addr, sum(o.amount) AS balanceSatoshis
WHERE balanceSatoshis > 1000000000  // More than 10 BTC (in satoshis)
RETURN addr.address, balanceSatoshis
ORDER BY balanceSatoshis DESC
```

### Analyze Transaction Fees Over Time

```cypher
MATCH (t:Transaction {isCoinbase: false})
WHERE t.timestamp >= datetime($startDate)
  AND t.timestamp <= datetime($endDate)
WITH date(t.timestamp) AS day, avg(t.fee) AS avgFeeSatoshis, count(t) AS txCount
RETURN day, avgFeeSatoshis, txCount
ORDER BY day
```

### Find Potential Mixing/Tumbling Patterns (Many-to-Many Transactions)

```cypher
// Transactions with many inputs AND many outputs
MATCH (t:Transaction)
MATCH (t)-[:HAS_INPUT]->(i:Input)
MATCH (t)-[:HAS_OUTPUT]->(o:Output)
WITH t, count(DISTINCT i) AS inputCount, count(DISTINCT o) AS outputCount
WHERE inputCount >= 10 AND outputCount >= 10
RETURN t.txid, inputCount, outputCount, t.timestamp
ORDER BY inputCount DESC, outputCount DESC
LIMIT 100
```

### Find OP_RETURN Outputs (Data Transactions)

```cypher
MATCH (o:Output {scriptType: 'NULL_DATA'})
MATCH (o)<-[:HAS_OUTPUT]-(t:Transaction)
RETURN t.txid, o.scriptPubKey, t.timestamp
ORDER BY t.timestamp DESC
LIMIT 100
```

---

## Performance Optimization Tips

### Use PROFILE to Analyze Query Performance

```cypher
PROFILE
MATCH (addr:Address {address: $address})
MATCH (addr)<-[:LOCKED_TO]-(o:Output {isSpent: false})
RETURN sum(o.amount) AS balanceSatoshis
```

### Use Parameters (Not String Concatenation)

**Good:**
```cypher
MATCH (t:Transaction {txid: $txid})
RETURN t
```

**Bad:**
```cypher
MATCH (t:Transaction {txid: "abc123..."})
RETURN t
```

### Limit Result Sets

Always use `LIMIT` when expecting large result sets:
```cypher
MATCH (addr:Address)-[:PERFORMS]->(t:Transaction)
RETURN t
ORDER BY t.timestamp DESC
LIMIT 1000  // Prevent returning millions of rows
```

### Use Indexes Effectively

Ensure property filters are on indexed properties:
```cypher
// Uses index on Transaction.timestamp
MATCH (t:Transaction)
WHERE t.timestamp >= datetime('2024-01-01')
RETURN t
```

---

## Complete Block Ingestion Example

### Full workflow for ingesting one block:

The application uses UNWIND-based batch queries for all phases. The conceptual per-block workflow is:

```
Phase 1: MERGE Block node + NEXT_BLOCK relationship
Phase 2: MERGE Output nodes + HAS_OUTPUT + LOCKED_TO relationships
         (concurrent: populate UTXO cache with outputs)
Phase 3: MERGE Transaction nodes with pre-calculated amounts + INCLUDED_IN
Phase 4: MERGE Input nodes + HAS_INPUT + SPENDS relationships
         (also marks spent outputs)
Phase 5: (removed — amounts calculated in Rust during Phase 3)
Phase 6: MERGE PERFORMS + BENEFITS_TO relationships (pre-aggregated data)
Phase 7: Evict spent outputs from UTXO cache
```

All phases use `MERGE` (not `CREATE`) for idempotent reprocessing.

See [INGESTION_ARCHITECTURE.md](../architecture/INGESTION_ARCHITECTURE.md) for detailed phase descriptions.

---

## References

- [Neo4j Cypher Manual](https://neo4j.com/docs/cypher-manual/current/)
- [Neo4j Cypher Refcard](https://neo4j.com/docs/cypher-refcard/current/)
- [DATA_MODEL.md](DATA_MODEL.md) - Schema reference
- [INGESTION_ARCHITECTURE.md](../architecture/INGESTION_ARCHITECTURE.md) - Processing phases
- [VALIDATION.md](VALIDATION.md) - Data integrity checks
