# Performance Optimization Tasks - Overview

## Context

The `bitcoin-chain-graph` Rust application ingests the Bitcoin blockchain into Neo4j.
At block height ~236,000 it processes ~5.4 blocks/sec, with ~700k blocks to go.

The main bottleneck is Phase 4 (Input ingestion): the `CREATE_INPUTS_FAST_QUERY` does
3 operations per input in a single UNWIND - creating Input nodes, HAS_INPUT relationships,
and SPENDS relationships + marking outputs spent. The SPENDS part takes 3-27 seconds per
5000-record batch, while everything else takes 200-700ms.

## Tasks

### Task 1: Drop isSpent Properties + Split Input Query + Pre-compute outputId
**File**: `01-split-input-query-and-precompute-outputid.md`
**Impact**: High - directly addresses the bottleneck
**Files changed**: 4 files (queries.rs, conversions.rs, neo4j/mod.rs, schema.rs)

Drop the redundant `isSpent`/`spentInTxid`/`spentAtHeight` properties from Output nodes entirely
(spent status is already derivable from the SPENDS relationship). This eliminates all write locks
on Output nodes during input ingestion - the entire bottleneck.

Also split the monolithic input query into two: one for fast node creation (CREATE Input +
HAS_INPUT), and one for SPENDS relationships (now read-only lookups, no SET on Output).
Pre-compute `previousOutputId` in Rust instead of Cypher string concatenation.

### Task 2: UTXO Cache Persistence
**File**: `02-utxo-cache-persistence.md`
**Impact**: Medium - faster restarts (seconds vs minutes)
**Files changed**: 5 files (cache.rs, config/mod.rs, main.rs, live.toml, Cargo.toml)

Dump the in-memory UTXO cache (~2GB) to a binary file on graceful shutdown. Load it back
on startup instead of pre-warming from Neo4j. Also add periodic snapshots during ingestion
to protect against hard crashes, and fix the pre-existing SIGTERM handling bug (systemd
sends SIGTERM but the current code only catches SIGINT).

## Build & Deploy

After making changes:
```bash
cd /data/bitcoin-chain-graph
cargo build --release
systemctl restart bitcoin-chain-graph
journalctl -u bitcoin-chain-graph -f
```

## Current Architecture Reference

```
src/
  config/mod.rs          - Config structs (PerformanceConfig, Neo4jConfig, etc.)
  domain/
    models.rs            - Domain types (BlockData, InputData, OutputData, etc.)
    conversions.rs       - bitcoin crate -> domain model conversions
    ingestion.rs         - 7-phase orchestrator (calls writer methods)
    utxo/
      mod.rs             - Module exports
      cache.rs           - Sharded LRU cache (UtxoKey, CachedOutput, UtxoCache)
  writer/
    mod.rs               - Module exports
    traits.rs            - GraphWriter trait definition
    neo4j/
      mod.rs             - Neo4jWriter impl (execute_batched, write_inputs_fast, etc.)
      queries.rs         - All Cypher query constants
      conversions.rs     - Domain -> BoltType conversions (input_to_bolt_map, etc.)
      schema.rs          - Schema initialization
    mock.rs              - MockWriter for tests
    error.rs             - WriterError types
  parser/                - Block file parsing + RPC + ZMQ
  main.rs                - CLI app (ingest, resume, live, status commands)
```

## Key Connection Details (for reference)

- **Neo4j**: bolt+s://bitcoin.04j.uk:7687, user=neo4j
- **Bitcoin RPC**: localhost:8332, user=btcgraph
- **Service**: systemd `bitcoin-chain-graph.service`
- **Config**: `/data/bitcoin-chain-graph/config/live.toml`
- **Binary**: `/data/bitcoin-chain-graph/target/release/bitcoin-chain-graph`