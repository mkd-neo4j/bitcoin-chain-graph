# Memory Management Strategy

Rust implementation strategy for memory-efficient Bitcoin blockchain ingestion with <2GB resident memory target.

---

## Overview

Bitcoin blockchain ingestion faces significant memory challenges:
- **Full UTXO set**: ~85 million outputs × 60 bytes = ~5GB
- **Block files**: Multi-GB files that must be parsed
- **Neo4j batch buffers**: Accumulating nodes/relationships before bulk insert

This document describes memory-efficient strategies to keep resident memory <2GB while maintaining high throughput.

---

## Memory Budget

Target allocation for 2GB total resident memory:

| Component | Budget | Strategy |
|-----------|--------|----------|
| UTXO Cache | 500MB-1GB | LRU cache for recent outputs |
| Block Parser Buffer | 100-200MB | Streaming parser, one block at a time |
| Neo4j Batch Buffer | 200-400MB | Batch accumulator before bulk insert |
| Neo4j Driver | 100-200MB | Connection pool, query buffers |
| Runtime Overhead | 100-200MB | Rust runtime, stack, misc allocations |
| **Total** | **~2GB** | |

---

## 1. Streaming Block File Parser

### Challenge
Bitcoin Core `.blk` files can be 128MB each, with hundreds of files totaling hundreds of GB. Loading entire files into memory is not feasible.

### Solution: Streaming Parser

**Approach:**
- Open file as buffered reader
- Read block-by-block sequentially
- Parse one block, process it, then discard before reading next
- Never hold more than one block in memory at a time

**Implementation Pattern:**

```rust
use std::fs::File;
use std::io::{BufReader, Read};
use bitcoin::Block;

pub struct BlockFileReader {
    reader: BufReader<File>,
    file_path: String,
}

impl BlockFileReader {
    pub fn new(path: &str) -> Result<Self> {
        let file = File::open(path)?;
        Ok(Self {
            reader: BufReader::with_capacity(8 * 1024 * 1024, file), // 8MB buffer
            file_path: path.to_string(),
        })
    }

    /// Read next block from file (streaming, no full file load)
    pub fn next_block(&mut self) -> Result<Option<Block>> {
        // Read magic bytes (4 bytes)
        let mut magic = [0u8; 4];
        match self.reader.read_exact(&mut magic) {
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(None); // End of file
            }
            Err(e) => return Err(e.into()),
        }

        // Verify magic bytes
        if magic != [0xF9, 0xBE, 0xB4, 0xD9] { // Mainnet magic
            return Err(Error::InvalidMagicBytes);
        }

        // Read block size (4 bytes, little-endian)
        let mut size_bytes = [0u8; 4];
        self.reader.read_exact(&mut size_bytes)?;
        let block_size = u32::from_le_bytes(size_bytes) as usize;

        // Read block data (exactly block_size bytes)
        let mut block_data = vec![0u8; block_size];
        self.reader.read_exact(&mut block_data)?;

        // Deserialize block using bitcoin crate
        let block: Block = bitcoin::consensus::deserialize(&block_data)?;

        Ok(Some(block))
    }
}

// Usage example
let mut reader = BlockFileReader::new("/path/to/blk00000.dat")?;
while let Some(block) = reader.next_block()? {
    // Process block
    process_block(&block)?;
    // Block is dropped here - memory freed immediately
}
```

**Memory Impact:**
- **Before**: Loading 128MB file = 128MB resident memory
- **After**: One block at a time ≈ 1-4MB per block (early chain: ~300KB, modern: ~2-4MB)
- **Savings**: ~124MB per file, hundreds of GB total across all files

### Buffer Size Tuning

```rust
// Smaller buffer (slower I/O, less memory)
BufReader::with_capacity(1 * 1024 * 1024, file); // 1MB

// Larger buffer (faster I/O, more memory)
BufReader::with_capacity(16 * 1024 * 1024, file); // 16MB

// Recommended: 8MB (good balance)
BufReader::with_capacity(8 * 1024 * 1024, file); // 8MB
```

---

## 2. UTXO Cache Strategy

### Challenge
To create `SPENDS` relationships, inputs must look up previous outputs. With 85M+ UTXOs, keeping all in memory requires ~5GB.

### Solution: LRU Cache + Neo4j Fallback

**Approach:**
- Keep only **recent outputs** in memory (configurable, default 100k)
- Use LRU (Least Recently Used) eviction policy
- On cache miss, query Neo4j for older outputs
- Exploit **temporal locality**: Most inputs spend recent outputs (same block or recent blocks)

**Implementation:**

```rust
use lru::LruCache;
use std::num::NonZeroUsize;

pub struct UtxoCache {
    cache: LruCache<String, CachedOutput>, // Key: outputId (txid:index)
    neo4j_client: Neo4jClient,
    hits: u64,
    misses: u64,
}

#[derive(Clone)]
pub struct CachedOutput {
    pub output_id: String,
    pub amount: f64,
    pub script_pubkey: Vec<u8>,
    pub address: Option<String>,
}

impl UtxoCache {
    pub fn new(capacity: usize, neo4j_client: Neo4jClient) -> Self {
        Self {
            cache: LruCache::new(NonZeroUsize::new(capacity).unwrap()),
            neo4j_client,
            hits: 0,
            misses: 0,
        }
    }

    /// Insert output into cache
    pub fn insert(&mut self, output_id: String, output: CachedOutput) {
        self.cache.put(output_id, output);
    }

    /// Look up output (cache-first, then Neo4j)
    pub async fn get(&mut self, output_id: &str) -> Result<CachedOutput> {
        // Try cache first
        if let Some(output) = self.cache.get(output_id) {
            self.hits += 1;
            return Ok(output.clone());
        }

        // Cache miss - query Neo4j
        self.misses += 1;
        let output = self.fetch_from_neo4j(output_id).await?;

        // Insert into cache for future lookups
        self.cache.put(output_id.to_string(), output.clone());

        Ok(output)
    }

    /// Fetch output from Neo4j
    async fn fetch_from_neo4j(&self, output_id: &str) -> Result<CachedOutput> {
        let query = "
            MATCH (o:Output {outputId: $outputId})
            OPTIONAL MATCH (o)-[:LOCKED_TO]->(a:Address)
            RETURN o.outputId as outputId,
                   o.amount as amount,
                   o.scriptPubKey as scriptPubKey,
                   a.address as address
        ";

        let result = self.neo4j_client
            .execute(query, &[("outputId", output_id)])
            .await?;

        // Parse result into CachedOutput
        // (error handling omitted for brevity)
        let row = result.first().ok_or(Error::OutputNotFound)?;
        Ok(CachedOutput {
            output_id: row.get("outputId")?,
            amount: row.get("amount")?,
            script_pubkey: hex::decode(row.get::<String>("scriptPubKey")?)?,
            address: row.get("address").ok(),
        })
    }

    /// Mark output as spent (remove from cache since it's no longer in UTXO set)
    pub fn mark_spent(&mut self, output_id: &str) {
        self.cache.pop(output_id);
    }

    /// Get cache statistics
    pub fn stats(&self) -> (u64, u64, f64) {
        let total = self.hits + self.misses;
        let hit_rate = if total > 0 {
            (self.hits as f64) / (total as f64)
        } else {
            0.0
        };
        (self.hits, self.misses, hit_rate)
    }
}

// Usage example
let mut utxo_cache = UtxoCache::new(100_000, neo4j_client);

// When processing outputs (Phase 3)
for output in transaction.output {
    let cached = CachedOutput {
        output_id: format!("{}:{}", txid, output.index),
        amount: output.value.to_btc(),
        script_pubkey: output.script_pubkey.to_bytes(),
        address: derive_address(&output.script_pubkey),
    };
    utxo_cache.insert(cached.output_id.clone(), cached);
}

// When processing inputs (Phase 4)
for input in transaction.input {
    let prev_output_id = format!("{}:{}", input.previous_output.txid, input.previous_output.vout);
    let prev_output = utxo_cache.get(&prev_output_id).await?;

    // Use prev_output to create SPENDS relationship
    // ...

    // Mark as spent (remove from cache)
    utxo_cache.mark_spent(&prev_output_id);
}

// Print cache statistics
let (hits, misses, hit_rate) = utxo_cache.stats();
println!("UTXO Cache - Hits: {}, Misses: {}, Hit Rate: {:.2}%",
         hits, misses, hit_rate * 100.0);
```

**Memory Calculation:**

```rust
// Per cached output:
struct CachedOutput {
    output_id: String,      // ~70 bytes (txid:index)
    amount: f64,            // 8 bytes
    script_pubkey: Vec<u8>, // ~25-40 bytes avg
    address: Option<String>,// ~35 bytes avg (if present)
}
// Total: ~138 bytes per entry

// For 100,000 entries:
// 100,000 × 138 bytes ≈ 13.8 MB
// LRU metadata overhead: ~10% = 1.4 MB
// Total: ~15 MB

// For 1,000,000 entries:
// 1,000,000 × 138 bytes ≈ 138 MB
// Overhead: ~14 MB
// Total: ~152 MB

// For 10,000,000 entries (Ultra Performance):
// 10,000,000 × 138 bytes ≈ 1.38 GB
// Overhead: ~138 MB
// Total: ~1.5 GB
```

**Configuration:**

```rust
// Low memory (cache recent ~1 hour of blocks)
let utxo_cache = UtxoCache::new(50_000, neo4j_client);  // ~7 MB

// Medium memory (cache recent ~6 hours of blocks)
let utxo_cache = UtxoCache::new(200_000, neo4j_client); // ~28 MB

// High memory (cache recent ~24 hours of blocks)
let utxo_cache = UtxoCache::new(1_000_000, neo4j_client); // ~138 MB

// Ultra performance (cache recent ~7 days of blocks)
let utxo_cache = UtxoCache::new(10_000_000, neo4j_client); // ~1.5 GB
```

**Expected Cache Hit Rate:**
- **Early blocks (2009-2012)**: 95-99% (most inputs spend outputs from same or recent block)
- **Modern blocks (2020+)**: 80-95% (more complex transaction patterns, older UTXO spending)

---

## 3. Neo4j Batch Buffer Management

### Challenge
Creating nodes/relationships one-at-a-time is slow. Batching improves performance, but accumulating large batches consumes memory.

### Solution: Bounded Batch Accumulator

**Approach:**
- Accumulate batch up to maximum size (blocks or memory threshold)
- When threshold reached, flush batch to Neo4j
- Clear buffer and start new batch
- Never exceed memory budget

**Implementation:**

```rust
pub struct Neo4jBatchBuilder {
    blocks: Vec<BlockData>,
    transactions: Vec<TransactionData>,
    outputs: Vec<OutputData>,
    inputs: Vec<InputData>,
    addresses: Vec<AddressData>,

    max_blocks: usize,
    max_memory_mb: usize,
    current_memory_estimate: usize,
}

impl Neo4jBatchBuilder {
    pub fn new(max_blocks: usize, max_memory_mb: usize) -> Self {
        Self {
            blocks: Vec::with_capacity(max_blocks),
            transactions: Vec::with_capacity(max_blocks * 2000), // Avg 2k tx/block
            outputs: Vec::with_capacity(max_blocks * 4000),
            inputs: Vec::with_capacity(max_blocks * 4000),
            addresses: Vec::with_capacity(max_blocks * 2000),
            max_blocks,
            max_memory_mb,
            current_memory_estimate: 0,
        }
    }

    /// Add block to batch
    pub fn add_block(&mut self, block: BlockData) -> Result<()> {
        self.blocks.push(block);
        self.current_memory_estimate += std::mem::size_of::<BlockData>();

        // Check if batch is full
        if self.should_flush() {
            return Err(Error::BatchFull);
        }

        Ok(())
    }

    /// Add transaction to batch
    pub fn add_transaction(&mut self, tx: TransactionData) -> Result<()> {
        self.current_memory_estimate += std::mem::size_of::<TransactionData>();
        self.transactions.push(tx);
        Ok(())
    }

    // Similar methods for outputs, inputs, addresses...

    /// Check if batch should be flushed
    fn should_flush(&self) -> bool {
        // Flush if block limit reached
        if self.blocks.len() >= self.max_blocks {
            return true;
        }

        // Flush if memory threshold reached
        let memory_mb = self.current_memory_estimate / (1024 * 1024);
        if memory_mb >= self.max_memory_mb {
            return true;
        }

        false
    }

    /// Get batch data for Neo4j bulk insert
    pub fn take_batch(&mut self) -> BatchData {
        let batch = BatchData {
            blocks: std::mem::take(&mut self.blocks),
            transactions: std::mem::take(&mut self.transactions),
            outputs: std::mem::take(&mut self.outputs),
            inputs: std::mem::take(&mut self.inputs),
            addresses: std::mem::take(&mut self.addresses),
        };

        // Reset memory estimate
        self.current_memory_estimate = 0;

        // Recreate vectors with capacity hints
        self.blocks = Vec::with_capacity(self.max_blocks);
        self.transactions = Vec::with_capacity(self.max_blocks * 2000);
        // ... reset other vectors

        batch
    }

    /// Clear batch without returning data
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.transactions.clear();
        self.outputs.clear();
        self.inputs.clear();
        self.addresses.clear();
        self.current_memory_estimate = 0;
    }
}

// Usage example
let mut batch_builder = Neo4jBatchBuilder::new(50, 256); // 50 blocks or 256 MB

for block_height in start_height..end_height {
    let block = parse_block(block_height)?;

    // Add to batch
    batch_builder.add_block(block.data)?;

    for tx in block.transactions {
        batch_builder.add_transaction(tx.data)?;
        // ... add outputs, inputs
    }

    // Check if batch is full
    if batch_builder.should_flush() {
        let batch = batch_builder.take_batch();
        neo4j_client.ingest_batch(batch).await?;
        // Batch memory is now freed
    }
}

// Flush remaining batch
if !batch_builder.is_empty() {
    let batch = batch_builder.take_batch();
    neo4j_client.ingest_batch(batch).await?;
}
```

**Memory Tuning:**

| Batch Size (blocks) | Estimated Memory | Throughput | Use Case |
|---------------------|------------------|------------|----------|
| 10 | ~50 MB | Lower | Low memory environments |
| 50 | ~250 MB | Medium | Balanced |
| 100 | ~500 MB | Higher | High memory available |
| 200 | ~1 GB | Highest | Maximum throughput |

---

## 4. Zero-Copy and Borrowing Strategies

### Challenge
Copying large byte arrays (blocks, transactions, scripts) wastes CPU and memory.

### Solution: Borrow Data Where Possible

**Pattern 1: Parse Once, Borrow Everywhere**

```rust
// BAD: Multiple copies
fn process_block(block_bytes: Vec<u8>) {
    let block: Block = deserialize(&block_bytes); // Copy 1
    let tx = block.txdata[0].clone(); // Copy 2
    let script = tx.output[0].script_pubkey.to_bytes(); // Copy 3
}

// GOOD: Parse once, borrow references
fn process_block(block: &Block) {
    for tx in &block.txdata {
        for output in &tx.output {
            let script_bytes = output.script_pubkey.as_bytes(); // Borrow, no copy
            let address = derive_address(script_bytes); // Pass by reference
        }
    }
}
```

**Pattern 2: Use `Cow<'a, [u8]>` for Conditional Ownership**

```rust
use std::borrow::Cow;

// Use borrowed slice if available, owned Vec if needed
fn store_script<'a>(script: Cow<'a, [u8]>) -> String {
    hex::encode(script.as_ref()) // Works with both borrowed and owned
}

// Usage with borrowed data (zero-copy)
let script_bytes: &[u8] = output.script_pubkey.as_bytes();
store_script(Cow::Borrowed(script_bytes));

// Usage with owned data (when necessary)
let modified_script: Vec<u8> = modify_script(script_bytes);
store_script(Cow::Owned(modified_script));
```

---

## 5. Memory Profiling and Monitoring

### Runtime Monitoring

```rust
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TrackingAllocator;

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ret = System.alloc(layout);
        if !ret.is_null() {
            ALLOCATED.fetch_add(layout.size(), Ordering::SeqCst);
        }
        ret
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        ALLOCATED.fetch_sub(layout.size(), Ordering::SeqCst);
    }
}

#[global_allocator]
static GLOBAL: TrackingAllocator = TrackingAllocator;

// Usage
pub fn get_allocated_memory() -> usize {
    ALLOCATED.load(Ordering::SeqCst)
}

// In ingestion loop
if block_height % 100 == 0 {
    let allocated_mb = get_allocated_memory() / (1024 * 1024);
    tracing::info!("Memory usage: {} MB", allocated_mb);
}
```

### External Profiling Tools

**Linux:**
```bash
# Heap profiling with heaptrack
heaptrack ./target/release/bitcoin-chain-graph ingest
heaptrack_gui heaptrack.bitcoin-chain-graph.*.gz

# Memory usage monitoring
/usr/bin/time -v ./target/release/bitcoin-chain-graph ingest

# Valgrind massif
valgrind --tool=massif ./target/release/bitcoin-chain-graph ingest
ms_print massif.out.*
```

**macOS:**
```bash
# Instruments
instruments -t "Allocations" ./target/release/bitcoin-chain-graph ingest
```

---

## 6. Memory Configuration Matrix

| Scenario | UTXO Cache | Batch Size | Max Memory | Throughput | Neo4j Heap |
|----------|-----------|------------|------------|------------|------------|
| **Constrained** (1GB) | 50k | 10 blocks | ~1GB | 5-10 blocks/sec | 512MB |
| **Standard** (2GB) | 200k | 50 blocks | ~2GB | 10-20 blocks/sec | 1GB |
| **High Performance** (4GB) | 1M | 100 blocks | ~4GB | 20-50 blocks/sec | 2GB |
| **Maximum** (8GB) | 2M | 200 blocks | ~8GB | 50-100 blocks/sec | 4GB |
| **Ultra Performance** (40GB) | 10M | 500 blocks | ~20GB | 200-400 blocks/sec (early), 10-20 blocks/sec (modern) | 16GB |

### Configuration Examples

**Standard scenario (2GB total):**

```toml
# config.toml for Standard scenario (2GB)
[memory]
utxo_cache_size = 200000
batch_max_blocks = 50
batch_max_memory_mb = 256
parser_buffer_mb = 8
```

**Ultra Performance scenario (40GB available):**

```toml
# config.toml for Ultra Performance scenario
[memory]
utxo_cache_size = 10000000  # 10 million entries (~1.3GB)
batch_max_blocks = 500
batch_max_memory_mb = 4096  # 4GB batch buffer
parser_buffer_mb = 16

[neo4j]
max_connections = 100
connection_timeout_secs = 30
unwind_batch_size = 10000

[parallelism]
num_worker_threads = 8  # Match CPU core count
max_concurrent_blocks = 16
```

**Neo4j configuration for Ultra Performance (neo4j.conf):**

```conf
# Heap allocation - 16GB for graph operations
dbms.memory.heap.initial_size=16g
dbms.memory.heap.max_size=16g

# Page cache - 20GB for data caching
dbms.memory.pagecache.size=20g

# Parallel execution
dbms.cypher.parallel_execution_enabled=true
dbms.cypher.parallel_runtime_workers=8

# Bolt connection pool
dbms.connector.bolt.thread_pool_min_size=16
dbms.connector.bolt.thread_pool_max_size=100
```

---

## Summary

Memory optimization strategies:
1. ✅ **Stream block files** - Never load entire files
2. ✅ **LRU UTXO cache** - Keep only recent outputs in memory
3. ✅ **Bounded batching** - Flush before exceeding memory budget
4. ✅ **Zero-copy parsing** - Borrow instead of copy
5. ✅ **Monitor allocations** - Track memory usage at runtime

**Result**: <2GB resident memory for full blockchain ingestion with high throughput.

---

## Next Steps

1. Read [BINARY_PARSING.md](BINARY_PARSING.md) for streaming parser implementation
2. Read [NEO4J_INTEGRATION.md](NEO4J_INTEGRATION.md) for batch ingestion patterns
3. Read [PERFORMANCE.md](PERFORMANCE.md) for throughput optimization
