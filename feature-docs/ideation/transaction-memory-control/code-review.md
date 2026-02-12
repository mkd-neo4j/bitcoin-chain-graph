# Code Review: Transaction Memory Control

## The Problem

A single Neo4j transaction wraps **ALL 7 phases** for a batch of N blocks. As blocks grow (post-2017: 2,000-4,000 txs/block), the transaction memory explodes.

**Memory per block in Neo4j transaction state:**

| Block Era | Tx/Block | Outputs/Block | ~Memory/Block |
|-----------|----------|---------------|---------------|
| 2009-2012 | 1-10 | 1-20 | ~8 KB |
| 2013-2017 | 100-500 | 200-1,000 | 100-400 KB |
| 2020+ | 2,000-4,000 | 4,000-8,000 | **7-10 MB** |

**Batch of 50 modern blocks → 350-500 MB in one transaction.**
**Batch of 200 modern blocks → 1.4-2.0 GB → hits the 2GB limit.**

## Current Configuration Landscape (The Problem)

There are **too many interrelated knobs** that all affect transaction memory:

### Config file settings:
| Setting | Default | What it does |
|---------|---------|-------------|
| `ingestion.batch_size` | 5000 | Blocks per Neo4j transaction |
| `neo4j.write_batch_size` | 5000 | Records per UNWIND query (sub-chunking within txn) |
| `neo4j.max_connections` | 20 | Connection pool size |
| `neo4j.query_timeout_secs` | 120 | Per-query timeout |
| `performance.utxo_cache_memory_mb` | 140 | UTXO cache size |
| `performance.utxo_lookup_batch_size` | 1000 | UTXO batch lookup size |
| `ingestion.checkpoint_interval` | 10 | Checkpoint every N blocks |

### Hardcoded:
| Constant | Value | What it does |
|----------|-------|-------------|
| `NUM_BUCKETS` | 8 | Phase 6 parallel tasks |
| `NUM_SHARDS` | 16 | UTXO cache shards |

### Ghost settings (in TOML but never read by code!):
- `max_batch_memory_mb = 512` — defined in config files but **NOT** in `src/config/mod.rs`
- `utxo_cache_snapshot_interval = 2000` — same, never read

### The core confusion:
- `ingestion.batch_size` = "blocks per batch" but actually controls **blocks per Neo4j transaction**
- `neo4j.write_batch_size` = "UNWIND chunk size" but sounds like it controls the batch
- `checkpoint_interval` = "checkpoint every N blocks" but the snapshot also saves per-batch
- The actual **memory-critical knob** is `ingestion.batch_size` but its name doesn't convey that

## Data Flow: What Goes Into ONE Transaction

```
ingestion.batch_size = N blocks
    ↓
BEGIN_TRANSACTION
    ↓
Phase 1: N Block nodes + N NEXT_BLOCK rels
Phase 2: Σ(outputs) Output nodes + LOCKED_TO rels
Phase 3: Σ(txs) Transaction nodes
Phase 3.5: Σ(outputs) HAS_OUTPUT rels
Phase 4: Σ(inputs) Input nodes + SPENDS rels
Phase 6: PERFORMS + BENEFITS_TO rels (8 parallel tasks)
Phase 7: Cache cleanup (in-memory only)
Checkpoint update
    ↓
COMMIT_TRANSACTION  ← everything held in memory until here
    ↓
Save UTXO snapshot
```

**For N modern blocks**: ~N × 50,000 entities (nodes + relationships) in one transaction.

## Key Insight

The only setting that actually matters for Neo4j transaction memory is **how many blocks go into one transaction**. Everything else is either:
- Sub-chunking within the transaction (`write_batch_size`) — doesn't reduce peak memory
- Unrelated to memory (`checkpoint_interval`, `utxo_cache_memory_mb`)
- Connection management (`max_connections`, `query_timeout_secs`)

The user's request for "one setting that handles everything" is architecturally sound — because there really is only one lever: **entities per transaction**.
