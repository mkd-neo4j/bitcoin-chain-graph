# API Design: Adaptive Transaction Memory Control

## The "One Knob"

```toml
[ingestion]
max_transaction_memory_mb = 600
```

This single setting replaces:
- `ingestion.batch_size` (5000) — removed
- `ingestion.checkpoint_interval` (10) — removed (ghost: never used in code)
- `ingestion.max_batch_memory_mb` (512) — removed (ghost: never wired to code)
- `performance.utxo_cache_snapshot_interval` (2000) — removed (ghost: live.toml only)

Stays independent:
- `neo4j.write_batch_size` (20000) — UNWIND chunk size, separate concern

## How It Works

### Current Flow (fixed batch_size)
```
main.rs loads 5000 blocks from disk
    ↓
orchestrator.ingest_blocks_batch(&blocks, 5000)
    ↓
blocks.chunks(5000) → 1 chunk of 5000
    ↓
begin_txn → process all 7 phases → commit_txn
    ↓
BOOM on modern blocks (5000 × 8MB = 40GB transaction)
```

### New Flow (adaptive memory budget)
```
main.rs loads blocks from disk (large read batch, e.g. 5000)
    ↓
orchestrator.ingest_blocks_batch(&blocks)
    ↓
compute_adaptive_chunks(blocks, max_transaction_memory_mb)
  → early blocks: [chunk of 5000]
  → modern blocks: [chunk of 60, chunk of 60, chunk of 60, ...]
    ↓
for each chunk:
    begin_txn → process all 7 phases → commit_txn → save snapshot
```

### The Key Insight

We already parse all blocks before writing. In `process_batch_chunk()` (ingestion.rs:697-708), the code counts entities:

```rust
let total_txs: usize = chunk.iter().map(|(_, b, _)| b.txdata.len()).sum();
let total_outputs: usize = chunk.iter()
    .flat_map(|(_, b, _)| b.txdata.iter())
    .map(|tx| tx.output.len()).sum();
let total_inputs: usize = chunk.iter()
    .flat_map(|(_, b, _)| b.txdata.iter())
    .map(|tx| tx.input.len()).sum();
```

We move this counting BEFORE the chunking loop, and use it to decide chunk boundaries.

## Memory Estimation

### Per-Entity Costs (Neo4j transaction state + Rust heap)

| Entity | Rust heap | Neo4j txn state | Total |
|--------|-----------|-----------------|-------|
| Block node + NEXT_BLOCK | ~200 B | ~300 B | ~500 B |
| Transaction node + INCLUDED_IN | ~150 B | ~250 B | ~400 B |
| Output node + HAS_OUTPUT + LOCKED_TO | ~200 B | ~350 B | ~550 B |
| Input node + HAS_INPUT + SPENDS | ~250 B | ~300 B | ~550 B |
| PERFORMS rel (aggregated) | ~100 B | ~150 B | ~250 B |
| BENEFITS_TO rel (aggregated) | ~100 B | ~150 B | ~250 B |

### Simplified Estimator

For a block with T transactions:
- Outputs ≈ 2 × T (average)
- Inputs ≈ 2.5 × T (average)
- Performs/BenefitsTo ≈ 0.8 × T each (aggregated, fewer than raw)

```
memory_per_block ≈ 500                           # block node
                 + T × 400                       # transaction nodes
                 + 2T × 550                      # output nodes
                 + 2.5T × 550                    # input nodes
                 + 0.8T × 250                    # PERFORMS
                 + 0.8T × 250                    # BENEFITS_TO
               ≈ 500 + T × 3,275

Simplified: memory_per_block ≈ T × 3,300 bytes
```

### Validation

| Block era | Avg T/block | Memory/block | Blocks in 600MB txn |
|-----------|-------------|-------------|---------------------|
| 2009-2012 | 5 | 16 KB | ~37,500 |
| 2013-2015 | 500 | 1.6 MB | ~375 |
| 2017-2020 | 2,000 | 6.6 MB | ~90 |
| 2021+ | 3,000 | 9.9 MB | ~60 |

This matches the user's observation: early blocks are trivial, modern blocks blow up.

## Adaptive Chunking Algorithm

```rust
/// Compute adaptive chunk boundaries based on memory budget.
/// Returns a Vec of (start_index, end_index) ranges.
fn compute_adaptive_chunks(
    blocks: &[(u32, Block, String)],
    max_memory_bytes: usize,
) -> Vec<std::ops::Range<usize>> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut accumulated_memory = 0usize;

    for (i, (_, block, _)) in blocks.iter().enumerate() {
        let block_memory = estimate_block_memory(block);

        // If adding this block exceeds budget AND we have at least 1 block
        if accumulated_memory + block_memory > max_memory_bytes && i > start {
            chunks.push(start..i);
            start = i;
            accumulated_memory = 0;
        }

        accumulated_memory += block_memory;
    }

    // Final chunk (always at least 1 block)
    if start < blocks.len() {
        chunks.push(start..blocks.len());
    }

    chunks
}

/// Estimate Neo4j transaction memory for a single block.
/// Based on entity count × average bytes per entity.
fn estimate_block_memory(block: &Block) -> usize {
    const BYTES_PER_BLOCK: usize = 500;
    const BYTES_PER_TX: usize = 400;
    const BYTES_PER_OUTPUT: usize = 550;
    const BYTES_PER_INPUT: usize = 550;

    let tx_count = block.txdata.len();
    let output_count: usize = block.txdata.iter().map(|tx| tx.output.len()).sum();
    let input_count: usize = block.txdata.iter().map(|tx| tx.input.len()).sum();

    BYTES_PER_BLOCK
        + tx_count * BYTES_PER_TX
        + output_count * BYTES_PER_OUTPUT
        + input_count * BYTES_PER_INPUT
}
```

### Safety Margin

The estimator intentionally overestimates by ~30% to account for:
- Neo4j index maintenance overhead
- PERFORMS/BENEFITS_TO aggregation (unpredictable dedup ratio)
- Neo4j internal bookkeeping per transaction

A user setting of `max_transaction_memory_mb = 600` would actually target ~460MB of real data.

## What Changes Where

### `src/config/mod.rs`

**Remove from IngestionConfig:**
- `batch_size: usize` (replaced by adaptive)
- `checkpoint_interval: u32` (ghost — never used)

**Add to IngestionConfig:**
- `max_transaction_memory_mb: usize` (default: 600)

**Remove ghost from deserialization tolerance:**
- `max_batch_memory_mb` in TOML — just stop including it

### `config/default.toml`

```toml
[ingestion]
max_transaction_memory_mb = 600    # NEW: the one knob
# batch_size = 5000                # REMOVED
# checkpoint_interval = 10         # REMOVED (was ghost)
# max_batch_memory_mb = 512        # REMOVED (was ghost)
```

### `config/live.toml`

Same changes plus remove `utxo_cache_snapshot_interval`.

### `src/domain/ingestion.rs`

**`ingest_blocks_batch()`:**
- Remove `batch_size: usize` parameter
- Add `max_transaction_memory_mb: usize` parameter (or read from stored config)
- Replace `blocks.chunks(batch_size)` with `compute_adaptive_chunks(blocks, max_memory)`
- Log adaptive chunk sizes for observability

**Add methods:**
- `compute_adaptive_chunks()` — chunk boundary calculation
- `estimate_block_memory()` — per-block memory estimator

### `src/main.rs`

- Remove `let batch_size = config.ingestion.batch_size`
- Pass `max_transaction_memory_mb` to orchestrator
- The outer loading loop can keep a large read batch (e.g. 5000) — the adaptive chunking happens inside the orchestrator

### Tests

- Update config construction in tests to use `max_transaction_memory_mb`
- Add unit tests for `estimate_block_memory()` with known blocks
- Add unit tests for `compute_adaptive_chunks()` with varying block sizes
- Integration test: mix of small and large blocks produces multiple chunks

## Logging / Observability

Each adaptive chunk should log:
```
INFO batch=1/3 blocks=60 txs=180000 outputs=360000 inputs=450000 estimated_mb=580 "Adaptive chunk"
INFO batch=2/3 blocks=55 txs=165000 outputs=330000 inputs=412000 estimated_mb=545 "Adaptive chunk"
INFO batch=3/3 blocks=12 txs=36000 outputs=72000 inputs=90000 estimated_mb=119 "Adaptive chunk"
```

This lets users tune `max_transaction_memory_mb` based on actual observed values.

## Edge Cases

1. **Single block exceeds budget**: Always include at least 1 block per chunk (can't split a block across transactions). Log a warning.
2. **Empty blocks**: Some blocks have only coinbase — negligible memory, pack many together.
3. **Very small budget**: `max_transaction_memory_mb = 1` → effectively per-block transactions. Slow but safe.
4. **Very large budget**: `max_transaction_memory_mb = 4000` → may hit Neo4j server limits. That's the user's problem, not ours.
5. **Estimation accuracy**: The estimator is intentionally conservative. If Neo4j OOMs despite staying under budget, user should lower the setting.
