# Cypher Query Examples for Bitcoin Blockchain

Complete Cypher query patterns for ingesting Bitcoin blockchain data into Neo4j and querying the resulting graph.

---

## Table of Contents

1. [Schema Setup (Constraints & Indexes)](#schema-setup)
2. [Checkpoint Management](#checkpoint-management)
3. [Phase 1: Block Ingestion](#phase-1-block-ingestion)
4. [Phase 2: Transaction Ingestion](#phase-2-transaction-ingestion)
5. [Phase 3: Output Ingestion](#phase-3-output-ingestion)
6. [Phase 4: Input Ingestion](#phase-4-input-ingestion)
7. [Phase 5: Calculate Transaction Amounts](#phase-5-calculate-transaction-amounts)
8. [Phase 6: Derive Simplified Layer](#phase-6-derive-simplified-layer)
9. [Query Examples - Simplified Layer](#query-examples-simplified-layer)
10. [Query Examples - Detailed UTXO Layer](#query-examples-detailed-utxo-layer)
11. [Analysis Queries](#analysis-queries)

---

## Schema Setup (Constraints & Indexes)

**Run these BEFORE ingesting any data:**

```cypher
// ============================================
// CONSTRAINTS (enforce uniqueness)
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
// INDEXES (improve query performance)
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

// Input indexes (for looking up previous outputs)
CREATE INDEX input_previous_tx IF NOT EXISTS
FOR (i:Input) ON (i.previousTxid);

// Address indexes
CREATE INDEX address_type IF NOT EXISTS
FOR (a:Address) ON (a.type);

// Block indexes
CREATE INDEX block_timestamp IF NOT EXISTS
FOR (b:Block) ON (b.timestamp);
```

---

## Checkpoint Management

Queries for managing ingestion checkpoints to enable resume-on-failure.

### Create Initial Checkpoint

```cypher
// Create checkpoint before starting ingestion
CREATE (c:IngestionCheckpoint {
  lastProcessedHeight: -1,
  lastProcessedHash: null,
  lastProcessedFile: null,
  lastProcessedFileOffset: null,
  timestamp: datetime(),
  status: "in_progress"
})
RETURN c
```

### Update Checkpoint After Successful Block

```cypher
// Update checkpoint after successfully ingesting a block
MATCH (c:IngestionCheckpoint)
SET c.lastProcessedHeight = $blockHeight,
    c.lastProcessedHash = $blockHash,
    c.lastProcessedFile = $blkFileName,
    c.lastProcessedFileOffset = $fileOffset,
    c.timestamp = datetime(),
    c.status = "in_progress"
RETURN c
```

**Parameters:**
- `$blockHeight` - Block height just processed (e.g., 0 for genesis)
- `$blockHash` - Block hash (e.g., "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f")
- `$blkFileName` - Name of .blk file (e.g., "blk00000.dat")
- `$fileOffset` - Byte offset in file after this block (optional, can be null)

### Query Checkpoint for Resume

```cypher
// Get checkpoint state to resume ingestion
MATCH (c:IngestionCheckpoint)
RETURN c.lastProcessedHeight AS lastHeight,
       c.lastProcessedHash AS lastHash,
       c.lastProcessedFile AS lastFile,
       c.lastProcessedFileOffset AS fileOffset,
       c.status AS status,
       c.timestamp AS lastUpdate
```

**Resume logic:**
- If `lastHeight = -1`: Start from genesis block (block 0)
- If `lastHeight >= 0`: Resume from block `lastHeight + 1`

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

### Mark Ingestion Complete

```cypher
// Mark ingestion as completed
MATCH (c:IngestionCheckpoint)
SET c.status = "completed",
    c.timestamp = datetime()
RETURN c
```

### Pause Ingestion

```cypher
// Pause ingestion (user-initiated)
MATCH (c:IngestionCheckpoint)
SET c.status = "paused",
    c.timestamp = datetime()
RETURN c
```

### Mark Ingestion Error

```cypher
// Mark ingestion as errored (for debugging/recovery)
MATCH (c:IngestionCheckpoint)
SET c.status = "error",
    c.timestamp = datetime()
RETURN c
```

### Reset Checkpoint (Start Fresh)

```cypher
// Delete existing checkpoint to start fresh ingestion
MATCH (c:IngestionCheckpoint)
DELETE c
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

### Create Block Node

```cypher
CREATE (b:Block {
  height: $height,
  hash: $hash,
  previousHash: $previousHash,
  merkleRoot: $merkleRoot,
  timestamp: datetime($timestamp),  // ISO 8601 string or epoch seconds
  txCount: $txCount,
  size: $size,
  weight: $weight,
  bits: $bits,
  difficulty: $difficulty,
  nonce: $nonce,
  version: $version,
  chainwork: $chainwork
})
RETURN b
```

### Link to Previous Block (NEXT_BLOCK)

```cypher
// Find previous block and link to new block
MATCH (prevBlock:Block {height: $height - 1})
MATCH (newBlock:Block {height: $height})
CREATE (prevBlock)-[:NEXT_BLOCK]->(newBlock)
RETURN prevBlock, newBlock
```

### Genesis Block (Special Case)

```cypher
// Genesis block has no previous block
CREATE (genesis:Block {
  height: 0,
  hash: "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f",
  previousHash: "0000000000000000000000000000000000000000000000000000000000000000",
  timestamp: datetime('2009-01-03T18:15:05Z'),
  txCount: 1,
  size: 285,
  weight: 1140,
  bits: "1d00ffff",
  difficulty: 1.0,
  nonce: 2083236893,
  version: 1,
  chainwork: "0000000000000000000000000000000000000000000000000000000100010001"
})
RETURN genesis
```

---

## Phase 2: Transaction Ingestion

### Create Transaction Node

```cypher
CREATE (t:Transaction {
  txid: $txid,
  blockHeight: $blockHeight,
  blockHash: $blockHash,
  timestamp: datetime($timestamp),
  version: $version,
  locktime: $locktime,
  size: $size,
  vsize: $vsize,
  weight: $weight,
  isCoinbase: $isCoinbase,
  // Note: totalInput, totalOutput, fee calculated in Phase 5
  totalInput: null,
  totalOutput: null,
  fee: null
})
RETURN t
```

### Create INCLUDED_IN Relationship

```cypher
// Link transaction to its containing block
MATCH (t:Transaction {txid: $txid})
MATCH (b:Block {height: $blockHeight})
CREATE (t)-[:INCLUDED_IN]->(b)
RETURN t, b
```

### Combined: Create Transaction + Link to Block

```cypher
MATCH (b:Block {height: $blockHeight})
CREATE (t:Transaction {
  txid: $txid,
  blockHeight: $blockHeight,
  blockHash: $blockHash,
  timestamp: b.timestamp,  // Inherit from block
  version: $version,
  locktime: $locktime,
  size: $size,
  vsize: $vsize,
  weight: $weight,
  isCoinbase: $isCoinbase,
  totalInput: null,
  totalOutput: null,
  fee: null
})
CREATE (t)-[:INCLUDED_IN]->(b)
RETURN t
```

---

## Phase 3: Output Ingestion

### Create Output Node + Address + Relationships

```cypher
// Create output
MATCH (t:Transaction {txid: $txid})
CREATE (o:Output {
  outputId: $txid + ':' + toString($outputIndex),
  outputIndex: $outputIndex,
  amount: $amount,
  scriptPubKey: $scriptPubKey,
  scriptType: $scriptType,
  isSpent: false,
  spentInTxid: null,
  spentAtHeight: null
})

// Link output to transaction
CREATE (t)-[:HAS_OUTPUT]->(o)

// If address was successfully derived, create/link address
WITH o, t
WHERE $address IS NOT NULL
MERGE (a:Address {address: $address})
ON CREATE SET a.type = $addressType
CREATE (o)-[:LOCKED_TO]->(a)

RETURN o, t
```

### Handle OP_RETURN (NULL_DATA) - No Address

```cypher
// OP_RETURN output has no address
MATCH (t:Transaction {txid: $txid})
CREATE (o:Output {
  outputId: $txid + ':' + toString($outputIndex),
  outputIndex: $outputIndex,
  amount: $amount,  // Usually 0
  scriptPubKey: $scriptPubKey,
  scriptType: 'NULL_DATA',
  isSpent: false,
  spentInTxid: null,
  spentAtHeight: null
})
CREATE (t)-[:HAS_OUTPUT]->(o)
// DO NOT create LOCKED_TO relationship
RETURN o
```

---

## Phase 4: Input Ingestion

### Create Input Node + SPENDS Relationship (Non-Coinbase)

```cypher
// Create input
MATCH (t:Transaction {txid: $txid})
CREATE (i:Input {
  inputId: $txid + ':' + toString($inputIndex),
  inputIndex: $inputIndex,
  previousTxid: $previousTxid,
  previousOutputIndex: $previousOutputIndex,
  scriptSig: $scriptSig,
  sequence: $sequence,
  witness: $witnessArray  // Array of hex strings (SegWit)
})

// Link input to transaction
CREATE (i)-[:HAS_INPUT]->(t)

// Find and link to previous output being spent
WITH i, t
MATCH (prevOut:Output {outputId: $previousTxid + ':' + toString($previousOutputIndex)})
CREATE (i)-[:SPENDS]->(prevOut)

// Update previous output spent status
SET prevOut.isSpent = true,
    prevOut.spentInTxid = $txid,
    prevOut.spentAtHeight = $blockHeight

RETURN i, t, prevOut
```

### Create Coinbase Input (Special Case)

```cypher
// Coinbase input - no previous output
MATCH (t:Transaction {txid: $txid, isCoinbase: true})
CREATE (i:Input {
  inputId: $txid + ':0',
  inputIndex: 0,
  previousTxid: "0000000000000000000000000000000000000000000000000000000000000000",
  previousOutputIndex: 4294967295,
  scriptSig: $coinbaseScriptSig,
  sequence: $sequence,
  witness: []  // No witness for coinbase
})
CREATE (i)-[:HAS_INPUT]->(t)
// DO NOT create SPENDS relationship
RETURN i, t
```

---

## Phase 5: Calculate Transaction Amounts

### Calculate Non-Coinbase Transaction Amounts

```cypher
// Calculate totalInput by summing spent output amounts
MATCH (t:Transaction {txid: $txid, isCoinbase: false})
MATCH (t)<-[:HAS_INPUT]-(i:Input)-[:SPENDS]->(prevOut:Output)
WITH t, sum(prevOut.amount) AS totalIn

// Calculate totalOutput by summing transaction output amounts
MATCH (t)-[:HAS_OUTPUT]->(o:Output)
WITH t, totalIn, sum(o.amount) AS totalOut

// Set amounts and calculate fee
SET t.totalInput = totalIn,
    t.totalOutput = totalOut,
    t.fee = totalIn - totalOut

RETURN t
```

### Calculate Coinbase Transaction Amounts

```cypher
// Coinbase has no inputs, only outputs
MATCH (t:Transaction {txid: $txid, isCoinbase: true})
MATCH (t)-[:HAS_OUTPUT]->(o:Output)
WITH t, sum(o.amount) AS totalOut

SET t.totalInput = 0.0,
    t.totalOutput = totalOut,
    t.fee = 0.0

RETURN t
```

---

## Phase 6: Derive Simplified Layer

### Create PERFORMS Relationships (Address → Transaction)

```cypher
// Find all addresses whose outputs were spent by this transaction
MATCH (t:Transaction {txid: $txid, isCoinbase: false})
MATCH (t)<-[:HAS_INPUT]-(i:Input)-[:SPENDS]->(prevOut:Output)-[:LOCKED_TO]->(addr:Address)
WITH t, addr
// Use MERGE to avoid duplicate relationships if multiple inputs from same address
MERGE (addr)-[:PERFORMS]->(t)
RETURN t, collect(DISTINCT addr) AS senders
```

### Create BENEFITS_TO Relationships (Transaction → Address)

```cypher
// Find all addresses that received outputs from this transaction
MATCH (t:Transaction {txid: $txid})
MATCH (t)-[:HAS_OUTPUT]->(o:Output)-[:LOCKED_TO]->(addr:Address)
WITH t, addr
// Use MERGE to avoid duplicate relationships if multiple outputs to same address
MERGE (t)-[:BENEFITS_TO]->(addr)
RETURN t, collect(DISTINCT addr) AS recipients
```

### Batch Process Simplified Layer for Multiple Transactions

```cypher
// Process all non-coinbase transactions in a block
MATCH (b:Block {height: $blockHeight})<-[:INCLUDED_IN]-(t:Transaction)
WHERE t.isCoinbase = false

// PERFORMS relationships
OPTIONAL MATCH (t)<-[:HAS_INPUT]-(i:Input)-[:SPENDS]->(prevOut:Output)-[:LOCKED_TO]->(senderAddr:Address)
WITH t, collect(DISTINCT senderAddr) AS senders
FOREACH (addr IN senders | MERGE (addr)-[:PERFORMS]->(t))

// BENEFITS_TO relationships
WITH t
MATCH (t)-[:HAS_OUTPUT]->(o:Output)-[:LOCKED_TO]->(recipientAddr:Address)
WITH t, collect(DISTINCT recipientAddr) AS recipients
FOREACH (addr IN recipients | MERGE (t)-[:BENEFITS_TO]->(addr))

RETURN count(t) AS processedCount
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
       sum(t.totalOutput) AS totalAmount
ORDER BY transactionCount DESC
LIMIT 50
```

---

## Query Examples - Detailed UTXO Layer

### Find All Unspent Outputs for an Address

```cypher
MATCH (addr:Address {address: $address})<-[:LOCKED_TO]-(o:Output)
WHERE o.isSpent = false
RETURN o.outputId, o.amount, o.scriptType
ORDER BY o.amount DESC
```

### Calculate Address Balance

```cypher
MATCH (addr:Address {address: $address})<-[:LOCKED_TO]-(o:Output {isSpent: false})
RETURN addr.address, sum(o.amount) AS balance
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
OPTIONAL MATCH (t)<-[:HAS_INPUT]-(i:Input)-[:SPENDS]->(prevOut:Output)-[:LOCKED_TO]->(inputAddr:Address)

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
MATCH (t)<-[:HAS_INPUT]-(i:Input)-[:SPENDS]->(prevOut:Output)-[:LOCKED_TO]->(addr:Address)
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
       sum(t.totalOutput) AS totalBlockValue,
       sum(t.fee) AS totalFees
```

### Find Largest Transactions in Date Range

```cypher
MATCH (t:Transaction)
WHERE t.timestamp >= datetime($startDate)
  AND t.timestamp <= datetime($endDate)
  AND t.isCoinbase = false
RETURN t.txid, t.totalOutput, t.timestamp
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
WITH addr, sum(o.amount) AS balance
WHERE balance > 10.0  // More than 10 BTC
RETURN addr.address, balance
ORDER BY balance DESC
```

### Analyze Transaction Fees Over Time

```cypher
MATCH (t:Transaction {isCoinbase: false})
WHERE t.timestamp >= datetime($startDate)
  AND t.timestamp <= datetime($endDate)
WITH date(t.timestamp) AS day, avg(t.fee) AS avgFee, count(t) AS txCount
RETURN day, avgFee, txCount
ORDER BY day
```

### Find Potential Mixing/Tumbling Patterns (Many-to-Many Transactions)

```cypher
// Transactions with many inputs AND many outputs
MATCH (t:Transaction)
MATCH (t)<-[:HAS_INPUT]-(i:Input)
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
RETURN sum(o.amount) AS balance
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

```cypher
// === Phase 1: Create Block ===
CREATE (b:Block {
  height: $blockHeight,
  hash: $blockHash,
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
});

// Link to previous block
MATCH (prevBlock:Block {height: $blockHeight - 1})
MATCH (newBlock:Block {height: $blockHeight})
CREATE (prevBlock)-[:NEXT_BLOCK]->(newBlock);

// === Phase 2-6: For each transaction in block ===
// (Repeat for each transaction)

// See individual phase examples above for transaction, output, input ingestion
```

---

## References

- [Neo4j Cypher Manual](https://neo4j.com/docs/cypher-manual/current/)
- [Neo4j Cypher Refcard](https://neo4j.com/docs/cypher-refcard/current/)
- [DATA_MODEL.md](DATA_MODEL.md) - Schema reference
- [INGESTION_ARCHITECTURE.md](../architecture/INGESTION_ARCHITECTURE.md) - Processing phases
- [VALIDATION.md](VALIDATION.md) - Data integrity checks
