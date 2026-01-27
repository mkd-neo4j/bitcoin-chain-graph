# Bitcoin Blockchain Ingestion Architecture

How Bitcoin blockchain data is loaded into Neo4j according to the [DATA_MODEL.md](../neo4j/DATA_MODEL.md) specification.

---

## Overview

This ingestion process reads Bitcoin Core's raw block files (`.blk` files) and transforms them into our dual-layer Neo4j graph model. The process must handle dependencies carefully — outputs must exist before inputs can reference them, and addresses must be derived before relationships can be created.

Transaction amounts (`totalInput`, `totalOutput`, `fee`) are calculated in Rust using an in-memory UTXO cache, avoiding expensive Neo4j graph traversals (see [Phase 3](#phase-3-create-transaction-nodes-with-amounts)).

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
1. Parse `.blk` files sequentially (blk00000.dat -> blk00001.dat -> ...)
2. Each file contains multiple blocks concatenated together
3. Blocks within files may not be in height order (due to how Bitcoin Core writes them)
4. Use block header's `previousHash` to reconstruct chain order

---

## Processing Phases

Ingestion follows a strict phase ordering to satisfy dependency requirements. Phases 2 and 3 are swapped from the traditional ordering to support same-block UTXO references (see rationale below).

### Phase 1: Create Block Nodes
- Read block header from `.blk` file
- Create `Block` node with all properties (height, hash, timestamp, difficulty, version, bits, nonce, size, weight, txCount)
- Create `NEXT_BLOCK` relationship to previous block (if not genesis)

**Why first?** Transaction nodes reference their containing block via `blockHeight` and `INCLUDED_IN` relationship.

**Source:** `ingest_block_node()` in `src/domain/ingestion.rs`

---

### Phase 2: Create Output Nodes, Address Relationships, and Populate UTXO Cache

For each output in each transaction:
1. Parse output data (outputIndex, amount, scriptPubKey)
2. Derive `scriptType` from scriptPubKey (see [ADDRESS_DERIVATION.md](../bitcoin/ADDRESS_DERIVATION.md))
3. Derive `address` from scriptPubKey (if parseable)
4. Create `Output` node with:
   - `outputId = {txid}:{outputIndex}`
   - `isSpent = false` (initially unspent)
   - `spentInTxid = null`
   - `spentAtHeight = null`
5. Create `HAS_OUTPUT` relationship: `Transaction -> Output`
6. If address was successfully derived:
   - Create or MERGE `Address` node
   - Create `LOCKED_TO` relationship: `Output -> Address`
7. **Insert output into UTXO cache** for use in Phase 3 amount calculations

Neo4j write and cache population run concurrently via `tokio::join!` (Neo4j is I/O-bound, cache inserts are CPU-bound).

**Why second (before transactions)?** Bitcoin allows transactions to spend outputs created by earlier transactions in the **same block**. By creating outputs and populating the UTXO cache before calculating transaction amounts, same-block UTXO references resolve correctly. For example, in block 546, Transaction 2 spends an output from Transaction 1 in the same block.

**Special case:** OP_RETURN outputs have `scriptType = 'NULL_DATA'` and no address — skip LOCKED_TO relationship (see [SPECIAL_CASES.md](../bitcoin/SPECIAL_CASES.md)).

**Source:** `ingest_outputs_and_cache()` in `src/domain/ingestion.rs`

---

### Phase 3: Create Transaction Nodes WITH Amounts

For each transaction in the block:
1. Parse transaction data (txid, version, locktime, size, vsize, weight, isCoinbase)
2. **Calculate amounts in Rust using the UTXO cache:**
   - `total_output = sum(tx.output.amount)` — trivial, from current block data
   - `total_input`:
     - **Coinbase:** `0` (no inputs)
     - **Regular:** Batch lookup all input previous outputs from UTXO cache via `get_many_with_fallback()`. Cache misses fall back to a single UNWIND Neo4j query.
   - `fee = total_input.saturating_sub(total_output)` (coinbase: `0`)
3. Create `Transaction` node with all properties including `totalInput`, `totalOutput`, `fee`
4. Create `INCLUDED_IN` relationship to the Block node

**Why third (after outputs)?** Outputs must be in the UTXO cache before amount calculation can reference them. This is the key change from the original design where transactions were Phase 2 and outputs were Phase 3.

**Performance:** Calculating amounts in Rust with cache lookups is 10-100x faster than the original Phase 5 approach of Neo4j graph traversals (3 Cypher queries per block).

**Batch mode:** In `ingest_blocks_batch()`, PERFORMS relationship data is also aggregated during this phase (see [Phase 6](#phase-6-create-simplified-layer-relationships)) to avoid redundant cache lookups.

**Source:** `ingest_transactions_with_amounts()` in `src/domain/ingestion.rs`

---

### Phase 4: Create Input Nodes and SPENDS Relationships
For each input in each transaction:
1. Parse input data (inputIndex, previousTxid, previousOutputIndex, scriptSig, sequence, witness)
2. Create `Input` node with `inputId = {txid}:{inputIndex}`
3. Create `HAS_INPUT` relationship: `Input -> Transaction`
4. **Lookup the previous output** being spent
5. Create `SPENDS` relationship: `Input -> Output`
6. **Update the spent output** with spent metadata:
   - `SET o.isSpent = true`
   - `SET o.spentInTxid = {current transaction txid}`
   - `SET o.spentAtHeight = {current block height}`

**Coinbase exception:** Coinbase transactions have one input with no previous output. The coinbase input node is created but no SPENDS relationship is generated (identified by `previousOutputIndex = 4294967295`).

**Cache removal is deferred** to Phase 7. Spent outputs must remain in the cache because Phase 6 needs them for PERFORMS relationship lookups.

**Source:** `ingest_inputs()` in `src/domain/ingestion.rs`

---

### Phase 5: REMOVED

> **This phase no longer exists.** Transaction amounts (`totalInput`, `totalOutput`, `fee`) are now calculated in Phase 3 using the UTXO cache in Rust.
>
> The original Phase 5 performed expensive Neo4j graph traversals (3 Cypher queries per block) to walk `Transaction <- Input -> Output` paths. This was the primary performance bottleneck. The UTXO cache approach is 10-100x faster.

---

### Phase 6: Create Simplified Layer Relationships

Create pre-aggregated "follow the money" relationships using data computed in Rust:

#### PERFORMS Relationship (Address -> Transaction)
For each non-coinbase transaction, aggregate input addresses:
- During Phase 3 (or batch mode), UTXO cache lookups resolve the address of each spent output
- Group by (address, txid), sum input counts and amounts
- Write `PERFORMS` relationships with `inputCount` and `amountSpent` properties

This answers: "Which address performed this transaction?" (i.e., whose funds were spent)

#### BENEFITS_TO Relationship (Transaction -> Address)
For each transaction, aggregate output addresses:
- During Phase 2, output addresses are already derived
- Group by (txid, address), sum output counts and amounts
- Write `BENEFITS_TO` relationships with `outputCount` and `amountReceived` properties

This answers: "Which addresses benefited from this transaction?" (i.e., who received funds)

**Batch mode parallelism:** In `ingest_blocks_batch()`, PERFORMS and BENEFITS_TO data is partitioned into 4 buckets by address hash. Each bucket is written in a separate `tokio::spawn` task. This enables parallel Neo4j writes without deadlocks (different buckets target different addresses).

**Note:** Multiple inputs from the same address create only one PERFORMS relationship (use MERGE). Multiple outputs to the same address create only one BENEFITS_TO relationship.

**Source:** `write_simplified_layer_rust()` and `extract_simplified_layer_data()` in `src/domain/ingestion.rs`

---

### Phase 7: UTXO Cache Eviction

Remove spent outputs from the UTXO cache:
- Collect all `UtxoKey`s for non-coinbase inputs in the current block
- Call `utxo_cache.remove_many(&spent_keys)` for batch removal

**Why after Phase 6?** Phase 6 looks up spent outputs to build PERFORMS relationships (needs the address of the previous output). Removing them before Phase 6 would cause cache misses and unnecessary Neo4j fallback queries.

**Source:** `remove_spent_outputs_from_cache()` in `src/domain/ingestion.rs`

---

## Processing Strategy

### Block-by-Block Sequential Processing

**Required:** Blocks MUST be processed in height order (0 -> 1 -> 2 -> ...) because:
- Transaction inputs reference outputs from previous transactions
- Previous transactions might be in earlier blocks
- Cannot create SPENDS relationship until referenced output exists

**Single-block mode** (`ingest_block()`):
1. Process each block through all 7 phases before moving to the next block
2. Update checkpoint after each block

**Batch mode** (`ingest_blocks_batch()`):
1. Accumulate multiple blocks (configurable batch size, default: 5000)
2. Process all blocks in the batch through each phase sequentially
3. Within each phase, bulk-write all accumulated data in a single Neo4j UNWIND query
4. Update checkpoint after each batch

### Within-Block Transaction Ordering

Bitcoin blocks store transactions in a specific order:
- **First transaction** is always the coinbase (mining reward)
- **Remaining transactions** can reference outputs from earlier transactions in the same block

**Required:** Process transactions within a block in order (index 0 -> 1 -> 2 -> ...) because:
- Transaction at index N might spend output from transaction at index M where M < N
- Phase 2 must create outputs for transaction M before Phase 3 can calculate amounts for transaction N

---

## UTXO Cache

### Overview

The UTXO cache is a 16-shard LRU cache that stores recent transaction outputs in memory. It serves as the primary lookup mechanism for transaction amount calculation (Phase 3) and simplified layer construction (Phase 6), with Neo4j as a fallback for cache misses.

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      UtxoCache<W>                       │
│                                                         │
│  ┌──────────┐ ┌──────────┐      ┌──────────┐           │
│  │ Shard 0  │ │ Shard 1  │ ...  │ Shard 15 │  16 shards│
│  │ Mutex<   │ │ Mutex<   │      │ Mutex<   │           │
│  │  LRU     │ │  LRU     │      │  LRU     │           │
│  │  Cache>  │ │  Cache>  │      │  Cache>  │           │
│  └──────────┘ └──────────┘      └──────────┘           │
│                                                         │
│  writer: Arc<W>         (Neo4j fallback on miss)        │
│  stats: AtomicStats     (lock-free hit/miss counters)   │
└─────────────────────────────────────────────────────────┘
```

### Key Design

| Component | Size | Description |
|-----------|------|-------------|
| `UtxoKey` | 36 bytes | Stack-allocated: 32-byte txid (raw bytes) + 4-byte vout. Zero allocation from `OutPoint`. |
| `CachedOutput` | ~36 bytes | `output_index: u32`, `amount: u64`, `script_type: ScriptTypeTag` (1 byte enum), `address: Option<Arc<str>>` |
| Per-entry overhead | ~72 bytes | Key + value + LRU bookkeeping |
| Sharding | 16 shards | Shard index = XOR of txid bytes, masked to 4 bits. Each shard has its own `Mutex<LruCache>`. |

### Memory Budget

Configured via `performance.utxo_cache_memory_mb` (default: 140 MB):

| Memory | Entries | Use Case |
|--------|---------|----------|
| 2 MB | ~28,000 | Low resource / testing |
| 15 MB | ~208,000 | Light ingestion |
| 140 MB | ~1,944,000 | Default (production) |
| 500 MB | ~6,940,000 | High performance |
| 1400 MB | ~19,440,000 | Ultra performance |

### Operations

- **`insert(key, value)`** — Add output to cache (Phase 2)
- **`get_many(&[UtxoKey])`** — Batch lookup, grouping by shard to acquire each lock once
- **`get_many_with_fallback(&[UtxoKey])`** — Cache lookup + single UNWIND Neo4j query for all misses
- **`remove_many(&[UtxoKey])`** — Batch removal of spent outputs (Phase 7)
- **`enable_prewarm_mode()` / `disable_prewarm_mode()`** — Suppress stats during cache pre-warming

### Cache Pre-Warming (Resume)

When resuming ingestion, the cache starts empty. To avoid poor hit rates during the first blocks:
1. Load `utxo_prewarm_depth` blocks backwards from the resume point (default: 1,000,000)
2. Insert all unspent outputs from those blocks into the cache
3. Pre-warming reads from `.blk` files (fast disk I/O), not Neo4j

**Source:** `src/domain/utxo/cache.rs`

---

## Error Handling and Resumption

### Checkpoint Strategy

**Track ingestion progress:**
```cypher
CREATE (checkpoint:IngestionCheckpoint {
  lastProcessedHeight: -999,
  lastProcessedHash: null,
  lastProcessedFile: null,
  lastProcessedFileOffset: null,
  timestamp: datetime(),
  status: "in_progress"
})
```

> **Note:** The sentinel height is `-999` (not `-1`) due to a neo4rs driver limitation with signed integer handling.

**Update after each successful block:**
```cypher
MERGE (c:IngestionCheckpoint)
SET c.lastProcessedHeight = $blockHeight,
    c.lastProcessedHash = $blockHash,
    c.lastProcessedFile = $blkFileName,
    c.lastProcessedFileOffset = $fileOffset,
    c.timestamp = $timestamp,
    c.status = $status
```

**Example values after processing Genesis block (block 0):**
```json
{
  "lastProcessedHeight": 0,
  "lastProcessedHash": "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f",
  "lastProcessedFile": "blk00000.dat",
  "lastProcessedFileOffset": null,
  "timestamp": 1706000000,
  "status": "in_progress"
}
```

**Checkpoint data model:**
```rust
pub struct CheckpointData {
    pub last_processed_height: i32,    // -999 for initial, then block height
    pub last_processed_hash: String,
    pub last_processed_file: String,
    pub last_processed_file_offset: Option<u64>,
    pub timestamp: i64,                // Unix epoch seconds
    pub status: String,                // "in_progress", "completed", "paused", "error"
}
```

**Mark ingestion complete:**
```cypher
MATCH (c:IngestionCheckpoint)
SET c.status = "completed",
    c.timestamp = datetime()
```

### Resume from Failure

**Resume logic:**
1. If `lastProcessedHeight = -999`: Start from Genesis block (block 0) in `blk00000.dat`
2. If `lastProcessedHeight >= 0`: Resume from block `lastProcessedHeight + 1`
3. Pre-warm UTXO cache before resuming forward ingestion

**Partial block reprocessing:**
- Since each block is ingested in a single Neo4j transaction, a failed block is automatically rolled back
- On resume, retry the failed block (height `lastProcessedHeight + 1`)
- **At most, 1 block is reprocessed** — clean Neo4j transaction boundaries guarantee consistency

**Error recovery:**
- If `status = "error"`: Review logs, fix issue, reset status to "in_progress", resume
- If `status = "paused"`: User-initiated pause, safe to resume
- If database corruption detected: Drop affected blocks and resume from last known good height

### Validation During Ingestion

After each block, optionally verify (see [VALIDATION.md](../neo4j/VALIDATION.md)):
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

### MERGE vs CREATE

**For nodes:**
- Use `MERGE` for Blocks, Transactions, Inputs, Outputs (idempotent for resume safety)
- Use `MERGE` for Addresses (same address appears many times)

**For relationships:**
- Use `CREATE` for HAS_INPUT, HAS_OUTPUT, SPENDS, LOCKED_TO, INCLUDED_IN (1:1 relationships)
- Use `MERGE` for PERFORMS, BENEFITS_TO (many inputs/outputs may map to same address)

### UTXO Cache-Based Amount Calculation

The UTXO cache eliminates Neo4j as a bottleneck for amount calculation:
- **Cache hit:** Direct memory lookup (~nanoseconds per entry)
- **Cache miss:** Single UNWIND Neo4j query for all misses in a batch (~milliseconds)
- **Expected hit rate:** 95-99% for sequential ingestion with sufficient cache size
- **Net effect:** Phase 3 processes at memory speed instead of database speed

### Parallel Simplified Layer Writes

In batch mode, PERFORMS and BENEFITS_TO writes are parallelized:
1. Data is partitioned into 4 buckets by address hash
2. Each bucket is written by a separate `tokio::spawn` task
3. Address-based partitioning ensures no two tasks write to the same Address node
4. This avoids Neo4j deadlocks while enabling 4x write parallelism

---

## Implementation Checklist

- [x] Parse Bitcoin Core `.blk` files correctly (magic bytes, block size, block data)
- [x] Reconstruct block ordering by height (using LevelDB block index)
- [x] Implement address derivation for all script types (see [ADDRESS_DERIVATION.md](../bitcoin/ADDRESS_DERIVATION.md))
- [x] Handle special cases: coinbase, OP_RETURN, genesis block (see [SPECIAL_CASES.md](../bitcoin/SPECIAL_CASES.md))
- [x] Process phases in correct order (1 -> 2(outputs) -> 3(tx+amounts) -> 4(inputs) -> 5(removed) -> 6(simplified) -> 7(cache eviction))
- [x] Process transactions within block in order
- [x] Create Neo4j constraints and indexes before ingestion
- [x] Implement checkpointing for resume-on-failure
- [x] UTXO cache with Neo4j fallback for amount calculation
- [x] Batch mode with parallel simplified layer writes
- [x] Cache pre-warming for resume operations
- [ ] Add validation after each block (see [VALIDATION.md](../neo4j/VALIDATION.md))
- [x] Test with early blocks (simple P2PKH) before modern blocks (SegWit, Taproot)

---

## Next Steps

1. Read [ADDRESS_DERIVATION.md](../bitcoin/ADDRESS_DERIVATION.md) to understand how to parse scriptPubKey and extract addresses
2. Read [SPECIAL_CASES.md](../bitcoin/SPECIAL_CASES.md) to handle coinbase transactions, OP_RETURN, and genesis block
3. Read [CYPHER_EXAMPLES.md](../neo4j/CYPHER_EXAMPLES.md) for concrete Cypher query patterns for each phase
4. Read [VALIDATION.md](../neo4j/VALIDATION.md) for data integrity checks during and after ingestion
5. Read [REAL_TIME_ARCHITECTURE.md](REAL_TIME_ARCHITECTURE.md) for live mode (RPC + ZMQ)

---

## References

- [Bitcoin Developer Reference - Block Chain](https://developer.bitcoin.org/reference/block_chain.html)
- [Bitcoin Core Data Directory](https://en.bitcoin.it/wiki/Data_directory)
- [Bitcoin Raw Block Format](https://en.bitcoin.it/wiki/Block)
