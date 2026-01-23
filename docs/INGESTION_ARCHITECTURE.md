# Bitcoin Blockchain Ingestion Architecture

How to load Bitcoin blockchain data from raw block files into Neo4j according to the [DATA_MODEL.md](DATA_MODEL.md) specification.

---

## Overview

This ingestion process reads Bitcoin Core's raw block files (`.blk` files) and transforms them into our dual-layer Neo4j graph model. The process must handle dependencies carefully - outputs must exist before inputs can reference them, and addresses must be derived before relationships can be created.

---

## Data Source: Bitcoin Core Raw Block Files

### Location
Bitcoin Core stores blockchain data in the `blocks/` directory within the data directory:

**Default locations:**
- Linux: `~/.bitcoin/blocks/`
- macOS: `~/Library/Application Support/Bitcoin/blocks/`
- Windows: `%APPDATA%\Bitcoin\blocks\`

### File Structure
- `blk00000.dat`, `blk00001.dat`, `blk00002.dat`, ... (raw block data)
- `blocks/index/` (LevelDB index for block locations)

Each `.blk` file contains:
- **Magic bytes** (4 bytes): Network identifier (`0xD9B4BEF9` for mainnet)
- **Block size** (4 bytes): Size of the following block
- **Block data**: Raw serialized block (header + transactions)

### Reading Strategy
1. Parse `.blk` files sequentially (blk00000.dat → blk00001.dat → ...)
2. Each file contains multiple blocks concatenated together
3. Blocks within files may not be in height order (due to how Bitcoin Core writes them)
4. Use block header's `previousHash` to reconstruct chain order

---

## Processing Phases

Ingestion must follow a strict ordering to satisfy dependency requirements:

### Phase 1: Create Block Nodes
- Read block header from `.blk` file
- Create `Block` node with all properties
- Create `NEXT_BLOCK` relationship to previous block (if not genesis)

**Why first?** Transaction nodes reference their containing block via `blockHeight` and `INCLUDED_IN` relationship.

---

### Phase 2: Create Transaction Nodes
For each transaction in the block:
- Parse transaction data (txid, version, locktime, etc.)
- Calculate `isCoinbase` (check if first input has null previous output)
- Create `Transaction` node
- Create `INCLUDED_IN` relationship to the Block node

**Why second?** Outputs need to exist before Inputs can reference them via SPENDS. But Transactions must exist before Outputs.

**Note:** `totalInput`, `totalOutput`, and `fee` cannot be fully calculated yet (see Phase 6).

---

### Phase 3: Create Output Nodes and Address Relationships
For each output in each transaction:
1. Parse output data (outputIndex, amount, scriptPubKey)
2. Derive `scriptType` from scriptPubKey (see [ADDRESS_DERIVATION.md](ADDRESS_DERIVATION.md))
3. Derive `address` from scriptPubKey (if parseable)
4. Create `Output` node with:
   - `outputId = {txid}:{outputIndex}`
   - `isSpent = false` (initially unspent)
   - `spentInTxid = null`
   - `spentAtHeight = null`
5. Create `HAS_OUTPUT` relationship: `Transaction → Output`
6. If address was successfully derived:
   - Create or MERGE `Address` node
   - Create `LOCKED_TO` relationship: `Output → Address`

**Why third?** Outputs must exist before Phase 4 can create SPENDS relationships to them.

**Special case:** OP_RETURN outputs have `scriptType = 'NULL_DATA'` and no address - skip LOCKED_TO relationship (see [SPECIAL_CASES.md](SPECIAL_CASES.md)).

---

### Phase 4: Create Input Nodes and SPENDS Relationships
For each input in each transaction:
1. Parse input data (inputIndex, previousTxid, previousOutputIndex, scriptSig, sequence, witness)
2. Create `Input` node with `inputId = {txid}:{inputIndex}`
3. Create `HAS_INPUT` relationship: `Input → Transaction`
4. **Lookup the previous output** being spent:
   - Query: `MATCH (o:Output {outputId: $previousTxid + ':' + $previousOutputIndex})`
5. Create `SPENDS` relationship: `Input → Output`
6. **Update the spent output** with spent metadata:
   - `SET o.isSpent = true`
   - `SET o.spentInTxid = {current transaction txid}`
   - `SET o.spentAtHeight = {current block height}`

**Why fourth?** Cannot create SPENDS until the referenced Output nodes exist from Phase 3.

**Coinbase exception:** Coinbase transactions have one input with no previous output. Skip steps 4-6 (see [SPECIAL_CASES.md](SPECIAL_CASES.md)).

**Critical dependency:** This phase requires that ALL previous transactions have completed Phase 3 before this transaction's Phase 4 runs. This means you must process blocks in chain order (by height).

---

### Phase 5: Calculate Transaction Amounts
Now that all inputs have SPENDS relationships:
1. For each non-coinbase transaction:
   - Calculate `totalInput` by summing amounts of all spent outputs:
     ```cypher
     MATCH (t:Transaction {txid: $txid})<-[:HAS_INPUT]-(i:Input)-[:SPENDS]->(prevOut:Output)
     WITH t, sum(prevOut.amount) as totalIn
     SET t.totalInput = totalIn
     ```
   - Calculate `totalOutput` by summing transaction outputs:
     ```cypher
     MATCH (t:Transaction {txid: $txid})-[:HAS_OUTPUT]->(o:Output)
     WITH t, sum(o.amount) as totalOut
     SET t.totalOutput = totalOut
     ```
   - Calculate fee: `SET t.fee = t.totalInput - t.totalOutput`

2. For coinbase transactions:
   - `totalInput = 0` (no inputs spent)
   - `totalOutput = sum(output amounts)`
   - `fee = 0`

**Why fifth?** Cannot calculate totalInput until SPENDS relationships exist and point to outputs with known amounts.

---

### Phase 6: Derive Simplified Layer Relationships
Create the "follow the money" relationships:

#### PERFORMS Relationship (Address → Transaction)
For each transaction input that spends a previous output:
```cypher
MATCH (t:Transaction {txid: $txid})<-[:HAS_INPUT]-(i:Input)
MATCH (i)-[:SPENDS]->(prevOut:Output)-[:LOCKED_TO]->(addr:Address)
MERGE (addr)-[:PERFORMS]->(t)
```

This answers: "Which address performed this transaction?" (i.e., whose funds were spent)

#### BENEFITS_TO Relationship (Transaction → Address)
For each transaction output that goes to an address:
```cypher
MATCH (t:Transaction {txid: $txid})-[:HAS_OUTPUT]->(o:Output)-[:LOCKED_TO]->(addr:Address)
MERGE (t)-[:BENEFITS_TO]->(addr)
```

This answers: "Which addresses benefited from this transaction?" (i.e., who received funds)

**Why last?** Both relationships require traversing the complete graph structure created in Phases 1-4.

**Note:** Multiple inputs from the same address create only one PERFORMS relationship (use MERGE). Multiple outputs to the same address create only one BENEFITS_TO relationship.

---

## Processing Strategy

### Block-by-Block Sequential Processing

**Required:** Blocks MUST be processed in height order (0 → 1 → 2 → ...) because:
- Transaction inputs reference outputs from previous transactions
- Previous transactions might be in earlier blocks
- Cannot create SPENDS relationship until referenced output exists

**Process:**
1. Read all blocks from `.blk` files
2. Sort blocks by height (using `previousHash` linkage to reconstruct order)
3. Process each block sequentially through all 6 phases before moving to next block

### Within-Block Transaction Ordering

Bitcoin blocks store transactions in a specific order:
- **First transaction** is always the coinbase (mining reward)
- **Remaining transactions** can reference outputs from earlier transactions in the same block

**Required:** Process transactions within a block in order (index 0 → 1 → 2 → ...) because:
- Transaction at index N might spend output from transaction at index M where M < N
- Must complete Phase 3 (create outputs) for transaction M before starting Phase 4 (create inputs that spend them) for transaction N

---

## Batch Processing Considerations

### Neo4j Transaction Boundaries

**Recommendation:** Use one Neo4j write transaction per Bitcoin block.

**Rationale:**
- Each Bitcoin block is self-contained (average ~2,000 transactions)
- Rolling back a failed block is clean (all-or-nothing)
- Memory usage is bounded and predictable
- Progress tracking is block-level (easy to resume on failure)

**Alternative:** Batch multiple small blocks (early blockchain) into single Neo4j transaction for performance.

### Memory Management

**Challenge:** Must keep UTXO set in memory or database to look up spent outputs.

**Strategy options:**
1. **Query Neo4j for each input:** Simple but slow
   ```cypher
   MATCH (o:Output {outputId: $prevTxid + ':' + $prevIndex})
   ```

2. **Cache recent outputs in memory:** Faster, assumes locality
   - Keep last N blocks' outputs in memory map
   - Fall back to Neo4j query for older outputs

3. **Build UTXO index:** Fastest, requires separate index structure
   - Maintain in-memory or disk-based UTXO set
   - Update as outputs are created and spent

**Recommendation for initial implementation:** Start with option 1 (query Neo4j) for simplicity. Optimize later if needed.

---

## Error Handling and Resumption

### Checkpoint Strategy

**Track ingestion progress:**
```cypher
CREATE (checkpoint:IngestionCheckpoint {
  lastProcessedHeight: -1,
  lastProcessedHash: null,
  lastProcessedFile: null,
  lastProcessedFileOffset: null,
  timestamp: datetime(),
  status: "in_progress"
})
```

**Initialize before processing Genesis block:**
- Set `lastProcessedHeight = -1` (no blocks processed yet)
- Set `lastProcessedHash = null`
- Set `lastProcessedFile = null` (will be set to "blk00000.dat" after first block)

**Update after each successful block:**
```cypher
MATCH (c:IngestionCheckpoint)
SET c.lastProcessedHeight = $blockHeight,
    c.lastProcessedHash = $blockHash,
    c.lastProcessedFile = $blkFileName,
    c.lastProcessedFileOffset = $fileOffset,
    c.timestamp = datetime(),
    c.status = "in_progress"
```

**Example values after processing Genesis block (block 0):**
```cypher
{
  lastProcessedHeight: 0,
  lastProcessedHash: "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f",
  lastProcessedFile: "blk00000.dat",
  lastProcessedFileOffset: 293,  // Optional: byte offset after genesis block
  timestamp: datetime(),
  status: "in_progress"
}
```

**Mark ingestion complete:**
```cypher
MATCH (c:IngestionCheckpoint)
SET c.status = "completed",
    c.timestamp = datetime()
```

### Resume from Failure

**Query checkpoint to resume:**
```cypher
MATCH (c:IngestionCheckpoint)
RETURN c.lastProcessedHeight AS lastHeight,
       c.lastProcessedHash AS lastHash,
       c.lastProcessedFile AS lastFile,
       c.status AS status
```

**Resume logic:**
1. If `lastProcessedHeight = -1`: Start from Genesis block (block 0) in `blk00000.dat`
2. If `lastProcessedHeight >= 0`: Resume from block `lastProcessedHeight + 1`
3. Use `lastProcessedFile` to determine which `.blk` file to continue reading
4. Optionally use `lastProcessedFileOffset` to seek directly to the next block in the file

**File transition handling:**
- If processing moves from `blk00000.dat` to `blk00001.dat`, update `lastProcessedFile`
- Parser can determine when to move to next file by tracking block counts per file
- Checkpoint always reflects the last successfully processed block

**Partial block reprocessing:**
- Since each block is ingested in a single Neo4j transaction, a failed block is automatically rolled back
- On resume, retry the failed block (height `lastProcessedHeight + 1`)
- **At most, 1 block is reprocessed** - clean Neo4j transaction boundaries guarantee consistency

**Data integrity verification before resume (optional):**
```cypher
// Verify last processed block exists in database
MATCH (b:Block {height: $lastProcessedHeight})
WHERE b.hash = $lastProcessedHash
RETURN b

// If not found or hash mismatch, consider re-ingesting from earlier checkpoint
```

**Error recovery:**
- If `status = "error"`: Review logs, fix issue, reset status to "in_progress", resume
- If `status = "paused"`: User-initiated pause, safe to resume
- If database corruption detected: Drop affected blocks and resume from last known good height

### Validation During Ingestion

After each block, optionally verify (see [VALIDATION.md](VALIDATION.md)):
- All transactions have `totalInput = totalOutput + fee` (except coinbase)
- All inputs have corresponding SPENDS relationships (except coinbase)
- All outputs with parseable addresses have LOCKED_TO relationships

---

## Performance Optimization

### Indexes

**Must be created BEFORE ingestion starts:**
```cypher
CREATE CONSTRAINT output_unique FOR (o:Output) REQUIRE o.outputId IS UNIQUE;
CREATE CONSTRAINT transaction_unique FOR (t:Transaction) REQUIRE t.txid IS UNIQUE;
CREATE CONSTRAINT address_unique FOR (a:Address) REQUIRE a.address IS UNIQUE;
CREATE CONSTRAINT block_height_unique FOR (b:Block) REQUIRE b.height IS UNIQUE;
CREATE INDEX output_spent FOR (o:Output) ON (o.isSpent);
```

**Why:** Looking up outputs by `outputId` during Phase 4 will happen millions of times. Without unique constraint, performance will degrade catastrophically.

### MERGE vs CREATE

**For nodes:**
- Use `CREATE` for Blocks, Transactions, Inputs, Outputs (guaranteed unique by validation)
- Use `MERGE` for Addresses (same address appears many times)

**For relationships:**
- Use `CREATE` for HAS_INPUT, HAS_OUTPUT, SPENDS, LOCKED_TO, INCLUDED_IN (1:1 relationships)
- Use `MERGE` for PERFORMS, BENEFITS_TO (many inputs/outputs may map to same address)

### Parallel Processing

**Initial recommendation:** Process blocks sequentially (simpler, safer).

**Future optimization:** Process multiple blocks in parallel IF:
- Blocks are far enough apart (no transaction dependencies between them)
- Example: Process block 1000 and block 500000 simultaneously (no overlapping UTXOs)
- Requires sophisticated dependency analysis

---

## Implementation Checklist

- [ ] Parse Bitcoin Core `.blk` files correctly (magic bytes, block size, block data)
- [ ] Reconstruct block ordering by height (use previousHash links)
- [ ] Implement address derivation for all script types (see [ADDRESS_DERIVATION.md](ADDRESS_DERIVATION.md))
- [ ] Handle special cases: coinbase, OP_RETURN, genesis block (see [SPECIAL_CASES.md](SPECIAL_CASES.md))
- [ ] Process phases in correct order (1→2→3→4→5→6)
- [ ] Process transactions within block in order
- [ ] Create Neo4j constraints and indexes before ingestion
- [ ] Implement checkpointing for resume-on-failure
- [ ] Add validation after each block (see [VALIDATION.md](VALIDATION.md))
- [ ] Test with early blocks (simple P2PKH) before modern blocks (SegWit, Taproot)

---

## Next Steps

1. Read [ADDRESS_DERIVATION.md](ADDRESS_DERIVATION.md) to understand how to parse scriptPubKey and extract addresses
2. Read [SPECIAL_CASES.md](SPECIAL_CASES.md) to handle coinbase transactions, OP_RETURN, and genesis block
3. Read [CYPHER_EXAMPLES.md](CYPHER_EXAMPLES.md) for concrete Cypher query patterns for each phase
4. Read [VALIDATION.md](VALIDATION.md) for data integrity checks during and after ingestion

---

## References

- [Bitcoin Developer Reference - Block Chain](https://developer.bitcoin.org/reference/block_chain.html)
- [Bitcoin Core Data Directory](https://en.bitcoin.it/wiki/Data_directory)
- [Bitcoin Raw Block Format](https://en.bitcoin.it/wiki/Block)
