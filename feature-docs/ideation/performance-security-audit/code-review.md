# Performance & Security Audit — Code Review

## Audit Scope
Full codebase audit across 4 areas: ingestion pipeline, Neo4j writer, parser/I/O, and security.

---

## HIGH Impact Performance Findings

### 1. Redundant SHA256d hash computations (ingestion.rs)
- `tx.txid()` computes SHA256d and is called multiple times per transaction across Phase 2, 3, and 4
- `block.block_hash()` similarly recomputed per phase
- **Fix**: Compute once at start of batch loop and reuse
- **Estimated savings**: 10-20% CPU during ingestion

### 2. Redundant `OutputData::from_output` in single-block Phase 6 (ingestion.rs:1442)
- Re-derives addresses (expensive `extract_address`) despite Phase 2 already computing them
- Batch path already avoids this correctly
- **Fix**: Pass computed output data from Phase 2 to Phase 6

### 3. `mark_output_spent` has no batch variant (traits.rs:196, neo4j/mod.rs:582)
- Trait forces one-at-a-time calls → N round-trips per block
- Bitcoin blocks can have thousands of inputs
- **Fix**: Add `mark_outputs_spent_batch` with UNWIND query

### 4. File opened per block in SingleBlockLoader (single_block_loader.rs:360)
- `File::open()` for every block — hundreds of thousands of syscalls
- **Fix**: Cache last-used file handle, reopen only when file_number changes

### 5. Fast NEXT_BLOCK query uses computed `height - 1` (queries.rs:440)
- Neo4j can't use index on `block.height - 1` expression
- Hot path for every block during forward ingestion
- **Fix**: Pre-compute `previousBlockHeight` in Rust, pass as parameter

### 6. Hash `.to_string()` hex encoding on every entity (conversions.rs:24-146)
- Every block hash, txid, prev_txid hex-encoded via Display trait
- ~9000 hex encode operations per block (3000 txs typical)
- **Fix**: Pre-allocated encode buffer, or store hashes as `[u8; 32]` (breaking)

## MEDIUM Impact Performance Findings

### 7. Per-transaction HashMap allocations (ingestion.rs:983)
- New `HashMap<String, (u32, u64)>` for `performs_map` inside each tx loop
- **Fix**: Reuse a single HashMap, `.clear()` between transactions

### 8. `block_hash.clone()` per transaction in Phase 3 batch (ingestion.rs:948)
- 64-char hex String cloned for every tx in the block
- **Fix**: Use `Arc<str>` for block_hash in PendingTx

### 9. Phase 6 bucket cloning for tokio::spawn (ingestion.rs:1098-1099)
- Deep-clones all PerformsData/BenefitsToData including String fields
- **Fix**: Use `std::mem::take` to move data into tasks

### 10. `.clone()` on multi-MB JSON RPC response (rpc_provider.rs:145)
- Copies entire hex block string (up to 8MB)
- **Fix**: Take ownership of the value

### 11. New Vec allocation per block read (single_block_loader.rs:386)
- `vec![0u8; block_size]` per block (up to 4MB)
- **Fix**: Reuse buffer with `.resize()`

### 12. `format!()` for output_id/input_id (conversions.rs:110,143)
- ~6000 format calls per block
- **Fix**: Pre-allocate with `String::with_capacity`

### 13. No `madvise(SEQUENTIAL)` on mmap (block_file.rs:65)
- OS readahead not optimized for sequential block reading
- **Fix**: `.advise(Advice::Sequential)` after mapping

## LOW Impact Performance Findings

### 14. Redundant ON CREATE/ON MATCH SET in output MERGE query (queries.rs:98-107)
### 15. FOREACH hack for conditional MERGE (queries.rs:41-43)
### 16. Double mutex lock per batch in execute_batched (neo4j/mod.rs:142,165)
### 17. `fill_percentage()` acquires all 16 shard locks twice (cache.rs:697-706)
### 18. `script_type` stored as String instead of &'static str (models.rs:97)
### 19. `addr.to_string()` then `Arc::from()` double allocation (single_block_loader.rs:512-513)
### 20. No fast variants for PERFORMS/BENEFITS_TO queries (traits.rs)

---

## SECURITY Findings

### HIGH Severity

**S1. Credentials logged at startup** — `main.rs:145-149`
- `tracing::info!(config_file = ?config, ...)` logs entire Config via Debug, including passwords
- **Fix**: Custom Debug impl that redacts passwords

**S2. Config structs derive Debug exposing passwords** — `config/mod.rs`
- `Neo4jConfig` and `BitcoinRpcConfig` both `#[derive(Debug)]`
- **Fix**: Custom Debug impl with `"***"` for password fields

**S3. `.expect()` in production signal handler** — `main.rs:739`
- Panics if SIGTERM registration fails
- **Fix**: Match on Result, fall back to ctrl_c only

### MEDIUM Severity

**S4. No size limit on RPC batch requests** — `rpc_provider.rs:244-264`
- **Fix**: Add MAX_BATCH_SIZE constant

**S5. No hex string size validation before decode** — `rpc_provider.rs:218-223`
- Malicious RPC node could send huge responses
- **Fix**: Check length before `hex::decode()`

**S6. Connection errors may leak credentials** — `neo4j/mod.rs:77-91`
- **Fix**: Sanitize neo4rs error messages

**S7. Missing credential validation** — `config/mod.rs:326-404`
- Empty user/password not checked
- **Fix**: Add non-empty checks

**S8. `panic!()` in UTXO cache Clone impl** — `cache.rs:1057`
- Intentional but could crash production
- **Fix**: Remove Clone derive entirely

**S9. `.expect()` on batch bounds** — `main.rs:471-472`
- **Fix**: Use `let Some(...)` pattern

### LOW Severity

**S10. Default passwords in code** — `config/mod.rs:286,436`
**S11. Config file permissions not checked** — `config/loader.rs`
**S12. ZMQ infinite timeout** — `zmq_listener.rs:195`
**S13. RPC URL in logs** — `rpc_provider.rs:274`

### Verified Secure

- Cypher injection: All queries parameterized constants ✅
- Unsafe code: Single `Mmap::map` — necessary and scoped ✅
- Bitcoin arithmetic: u64 satoshis, saturating_sub for fees ✅
- UTXO cache: LRU-bounded, CRC32 validated ✅
- Path traversal: Format string prevents `../` ✅
- Transaction safety: Proper Mutex usage ✅
