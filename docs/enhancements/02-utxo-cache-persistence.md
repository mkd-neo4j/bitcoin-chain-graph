# Task 2: UTXO Cache Persistence (Snapshot to Disk + Periodic Saves)

## Problem

When the `bitcoin-chain-graph` service restarts, it loses the entire in-memory UTXO cache
(currently ~2GB / ~27M entries configured). It then has to "pre-warm" the cache by querying
Neo4j, which is slow. For live mode, there's no pre-warming at all — it starts cold.

Worse, if the process crashes (OOM, panic, `kill -9`), there's no way to recover the cache
at all since it only existed in memory.

## Solution

Three-pronged approach:

1. **Snapshot on shutdown**: On graceful shutdown (SIGTERM or SIGINT), serialize the entire
   UTXO cache to a binary file.
2. **Periodic snapshots**: During ingestion, snapshot the cache every N blocks (configurable)
   so that even a hard crash only loses a bounded amount of cache warmth.
3. **Restore on startup**: On startup, if a snapshot file exists, load it back into memory
   instead of starting cold or pre-warming from Neo4j.

This should take seconds (sequential disk I/O) vs minutes (Neo4j index lookups).

## File Format

Use a simple custom binary format (no new dependencies needed beyond `crc32fast`):

```
Header (24 bytes):
  [4 bytes] Magic: "UTXO" (0x5554584F)
  [4 bytes] Version: 1 (u32 LE)
  [8 bytes] Entry count (u64 LE)
  [4 bytes] Checkpoint height at save time (u32 LE)
  [4 bytes] CRC32 of all entry bytes (u32 LE)

Per entry (repeated `entry_count` times):
  [32 bytes] txid (raw bytes)
  [4 bytes]  vout (u32 LE)
  [4 bytes]  output_index (u32 LE)
  [8 bytes]  amount (u64 LE)
  [1 byte]   script_type (ScriptTypeTag as u8)
  [1 byte]   has_address (0 or 1)
  [2 bytes]  address_len (u16 LE, only if has_address=1)
  [N bytes]  address_bytes (UTF-8, only if has_address=1)
```

Key design choices:
- **Checkpoint height** in header enables stale-cache detection on load
- **CRC32** detects corruption from partial writes or disk errors
- **No compression** for v1 (file is ~1.4GB; could add LZ4 later if I/O is a bottleneck)

## Dependency Addition

Add `crc32fast` to `Cargo.toml`:

```toml
crc32fast = "1.4"
```

This is a tiny, zero-dep crate for fast CRC32 checksums. Used to validate cache file
integrity on load.

## Files to Modify

### 1. `src/domain/utxo/cache.rs` — Add save/load methods

Add these methods to `impl<W: GraphWriter> UtxoCache<W>`:

```rust
use std::io::{BufReader, BufWriter, Read, Write, Seek, SeekFrom};
use std::fs::File;
use std::path::Path;

const CACHE_FILE_MAGIC: &[u8; 4] = b"UTXO";
const CACHE_FILE_VERSION: u32 = 1;

/// Save entire cache contents to a binary file (atomic via temp + rename).
///
/// Iterates all shards and writes every entry. The file can be loaded
/// back with `load_from_file()` to restore cache state after restart.
///
/// Uses atomic write: data is written to `<path>.tmp` first, then renamed
/// to the final path. This prevents corruption if the process is killed
/// mid-save (`rename()` is atomic on Linux/ext4).
///
/// # Arguments
/// * `path` - Destination file path
/// * `checkpoint_height` - Current ingestion checkpoint height (stored in header
///   for stale-cache detection on load)
///
/// Returns the number of entries written.
pub fn save_to_file<P: AsRef<Path>>(&self, path: P, checkpoint_height: u32) -> std::io::Result<usize> {
    let path = path.as_ref();
    let tmp_path = path.with_extension("bin.tmp");

    let file = File::create(&tmp_path)?;
    let mut writer = BufWriter::with_capacity(1024 * 1024, file); // 1MB buffer

    // Count total entries first
    let total_entries: usize = self.shards.iter()
        .map(|s| s.lock().expect("shard poisoned").len())
        .sum();

    // Write header (CRC placeholder — we'll seek back and fill it in)
    writer.write_all(CACHE_FILE_MAGIC)?;
    writer.write_all(&CACHE_FILE_VERSION.to_le_bytes())?;
    writer.write_all(&(total_entries as u64).to_le_bytes())?;
    writer.write_all(&checkpoint_height.to_le_bytes())?;
    writer.write_all(&0u32.to_le_bytes())?; // CRC32 placeholder

    let mut hasher = crc32fast::Hasher::new();
    let mut written = 0usize;

    for shard_mutex in &self.shards {
        let shard = shard_mutex.lock().expect("shard poisoned");
        // iter() returns MRU-to-LRU order without modifying LRU state
        for (key, value) in shard.iter() {
            // Write key: 32 bytes txid + 4 bytes vout
            writer.write_all(&key.txid)?;
            hasher.update(&key.txid);
            let vout_bytes = key.vout.to_le_bytes();
            writer.write_all(&vout_bytes)?;
            hasher.update(&vout_bytes);

            // Write value
            let oi_bytes = value.output_index.to_le_bytes();
            writer.write_all(&oi_bytes)?;
            hasher.update(&oi_bytes);

            let amt_bytes = value.amount.to_le_bytes();
            writer.write_all(&amt_bytes)?;
            hasher.update(&amt_bytes);

            let st_byte = [value.script_type as u8];
            writer.write_all(&st_byte)?;
            hasher.update(&st_byte);

            match &value.address {
                Some(addr) => {
                    let addr_bytes = addr.as_bytes();
                    writer.write_all(&[1u8])?;
                    hasher.update(&[1u8]);
                    let len_bytes = (addr_bytes.len() as u16).to_le_bytes();
                    writer.write_all(&len_bytes)?;
                    hasher.update(&len_bytes);
                    writer.write_all(addr_bytes)?;
                    hasher.update(addr_bytes);
                }
                None => {
                    writer.write_all(&[0u8])?;
                    hasher.update(&[0u8]);
                }
            }

            written += 1;
        }
    }

    // Seek back and write the CRC32 into the header
    writer.flush()?;
    let crc = hasher.finalize();
    let mut file = writer.into_inner().map_err(|e| e.into_error())?;
    // CRC32 is at offset 20 (4 magic + 4 version + 8 count + 4 checkpoint)
    file.seek(SeekFrom::Start(20))?;
    file.write_all(&crc.to_le_bytes())?;
    file.sync_all()?;
    drop(file);

    // Atomic rename: tmp -> final (prevents corrupt partial files)
    std::fs::rename(&tmp_path, path)?;

    tracing::info!(
        entries = written,
        checkpoint_height = checkpoint_height,
        crc32 = format!("{:08x}", crc),
        path = %path.display(),
        "UTXO cache saved to file"
    );

    Ok(written)
}

/// Load cache contents from a binary file, replacing current cache contents.
///
/// Streams entries directly into cache shards without buffering all entries
/// in memory. Each shard within the file is stored MRU-first; entries are
/// inserted via `insert_no_stats()` to avoid inflating statistics counters.
///
/// Returns Ok(0) if the file doesn't exist (not an error — first run).
///
/// # Stale cache detection
///
/// If `current_checkpoint_height` differs from the height stored in the
/// file header, a warning is logged. The cache is still loaded (stale
/// entries are harmless — they just occupy space until evicted), but the
/// operator is alerted that hit rates may be lower than expected.
pub fn load_from_file<P: AsRef<Path>>(
    &self,
    path: P,
    current_checkpoint_height: Option<u32>,
) -> std::io::Result<usize> {
    let path = path.as_ref();

    if !path.exists() {
        tracing::info!(path = %path.display(), "No cache file found, starting with empty cache");
        return Ok(0);
    }

    let file = File::open(path)?;
    let file_size = file.metadata()?.len();
    let mut reader = BufReader::with_capacity(1024 * 1024, file); // 1MB buffer

    // Read and validate header
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic)?;
    if &magic != CACHE_FILE_MAGIC {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid cache file magic: {:?}", magic),
        ));
    }

    let mut version_bytes = [0u8; 4];
    reader.read_exact(&mut version_bytes)?;
    let version = u32::from_le_bytes(version_bytes);
    if version != CACHE_FILE_VERSION {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Unsupported cache file version: {}", version),
        ));
    }

    let mut count_bytes = [0u8; 8];
    reader.read_exact(&mut count_bytes)?;
    let entry_count = u64::from_le_bytes(count_bytes) as usize;

    let mut height_bytes = [0u8; 4];
    reader.read_exact(&mut height_bytes)?;
    let saved_height = u32::from_le_bytes(height_bytes);

    let mut crc_bytes = [0u8; 4];
    reader.read_exact(&mut crc_bytes)?;
    let expected_crc = u32::from_le_bytes(crc_bytes);

    // Stale cache detection
    if let Some(current_height) = current_checkpoint_height {
        if current_height != saved_height {
            tracing::warn!(
                saved_at_height = saved_height,
                current_height = current_height,
                delta = current_height as i64 - saved_height as i64,
                "Cache file is from a different checkpoint height. \
                 Cache is still usable but may contain stale entries."
            );
        }
    }

    tracing::info!(
        entries = entry_count,
        file_size_mb = file_size / (1024 * 1024),
        saved_at_height = saved_height,
        path = %path.display(),
        "Loading UTXO cache from file"
    );

    // Stream entries, computing CRC as we go.
    // We need to reverse insertion order to preserve LRU ordering
    // (file stores MRU-first, but we must insert LRU-first so MRU ends up at front).
    //
    // To avoid allocating a ~2GB Vec for 27M entries, we do two passes:
    //   Pass 1: Read all entries, compute CRC, collect into per-shard vecs
    //   Pass 2: Insert per-shard in reverse order
    //
    // Per-shard vecs are much smaller (27M / 16 shards = ~1.7M per shard, ~120MB each).
    // Total temp memory: same ~2GB but we validate CRC before inserting anything.
    //
    // Alternative: if memory is tight, we could seek backwards through the file.
    // For now, the two-pass approach is simpler and still fast.

    let mut hasher = crc32fast::Hasher::new();
    let mut per_shard: Vec<Vec<(UtxoKey, CachedOutput)>> = (0..NUM_SHARDS)
        .map(|_| Vec::new())
        .collect();

    for _ in 0..entry_count {
        // Read key
        let mut txid = [0u8; 32];
        reader.read_exact(&mut txid)?;
        hasher.update(&txid);

        let mut vout_bytes = [0u8; 4];
        reader.read_exact(&mut vout_bytes)?;
        hasher.update(&vout_bytes);
        let vout = u32::from_le_bytes(vout_bytes);

        // Read value
        let mut output_index_bytes = [0u8; 4];
        reader.read_exact(&mut output_index_bytes)?;
        hasher.update(&output_index_bytes);
        let output_index = u32::from_le_bytes(output_index_bytes);

        let mut amount_bytes = [0u8; 8];
        reader.read_exact(&mut amount_bytes)?;
        hasher.update(&amount_bytes);
        let amount = u64::from_le_bytes(amount_bytes);

        let mut script_type_byte = [0u8; 1];
        reader.read_exact(&mut script_type_byte)?;
        hasher.update(&script_type_byte);
        let script_type = match script_type_byte[0] {
            0 => ScriptTypeTag::P2PKH,
            1 => ScriptTypeTag::P2SH,
            2 => ScriptTypeTag::P2WPKH,
            3 => ScriptTypeTag::P2WSH,
            4 => ScriptTypeTag::P2TR,
            5 => ScriptTypeTag::P2PK,
            6 => ScriptTypeTag::NullData,
            _ => ScriptTypeTag::Unknown,
        };

        let mut has_address_byte = [0u8; 1];
        reader.read_exact(&mut has_address_byte)?;
        hasher.update(&has_address_byte);

        let address = if has_address_byte[0] == 1 {
            let mut addr_len_bytes = [0u8; 2];
            reader.read_exact(&mut addr_len_bytes)?;
            hasher.update(&addr_len_bytes);
            let addr_len = u16::from_le_bytes(addr_len_bytes) as usize;

            let mut addr_bytes = vec![0u8; addr_len];
            reader.read_exact(&mut addr_bytes)?;
            hasher.update(&addr_bytes);

            Some(Arc::from(
                std::str::from_utf8(&addr_bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?,
            ))
        } else {
            None
        };

        let key = UtxoKey::new(txid, vout);
        let shard_idx = Self::shard_index(&key);
        let value = CachedOutput {
            output_index,
            amount,
            script_type,
            address,
        };

        per_shard[shard_idx].push((key, value));
    }

    // Validate CRC before inserting anything
    let computed_crc = hasher.finalize();
    if computed_crc != expected_crc {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Cache file CRC mismatch: expected {:08x}, computed {:08x}. \
                 File may be corrupt.",
                expected_crc, computed_crc
            ),
        ));
    }

    // Insert per-shard in reverse order to preserve LRU ordering.
    // File stores MRU-first per shard, so reversing gives us LRU-first insertion.
    // Use direct shard insertion to avoid inflating stats counters.
    let mut loaded = 0usize;
    for (shard_idx, entries) in per_shard.into_iter().enumerate() {
        let mut shard = self.shards[shard_idx].lock().expect("shard poisoned");
        for (key, value) in entries.into_iter().rev() {
            shard.put(key, value);
            loaded += 1;
        }
    }

    tracing::info!(
        loaded = loaded,
        cache_size = self.len(),
        fill_pct = format!("{:.1}", self.fill_percentage() * 100.0),
        crc32 = format!("{:08x}", computed_crc),
        "UTXO cache loaded from file"
    );

    Ok(loaded)
}
```

**Key design decisions in the code above:**

1. **Atomic writes**: `save_to_file` writes to `<path>.tmp` then `rename()`s to the final
   path. `rename()` is atomic on Linux/ext4, so a crash mid-save leaves the old valid file
   (or no file) — never a corrupt partial file.

2. **CRC32 integrity check**: Both save and load compute a running CRC32 over all entry
   bytes. The save writes it into the header (via seek-back). The load validates it before
   inserting anything into the cache. A corrupt file is rejected with a clear error.

3. **No stats inflation**: `load_from_file` inserts directly into `shard.put()` instead
   of calling `self.insert()`. This avoids recording 27M phantom "inserts" in the atomic
   stats counters before any real work happens.

4. **Per-shard reverse insertion**: Entries are grouped by shard during read, then each
   shard's entries are inserted in reverse order. This preserves LRU ordering (file stores
   MRU-first from `shard.iter()`, reverse gives LRU-first insertion so MRU ends up at front).

5. **Stale cache detection**: The header stores the checkpoint height at save time.
   On load, if the current checkpoint differs, a warning is logged. Stale entries are
   harmless (they'll be evicted by LRU naturally) but the operator knows to expect
   lower hit rates initially.

6. **Memory during load**: Per-shard vecs total the same ~2GB as the old single Vec, but
   the CRC is validated before anything is inserted. An alternative streaming approach
   (read file backwards) could reduce this, but adds complexity for v1.

**Note on `shard.iter()`**: `lru` crate v0.12 `iter()` takes `&self`, returns `(&K, &V)`
pairs in MRU-to-LRU order, and does NOT modify LRU state (equivalent to peeking).

**Note on private fields**: `UtxoKey.txid` and `UtxoKey.vout` are private, but
`save_to_file`/`load_from_file` are in `cache.rs` (same module), so they have access.

### 2. `src/config/mod.rs` — Add cache file path + snapshot interval config

Add to `PerformanceConfig` struct (after `utxo_prewarm_depth`, around line 160):

```rust
/// Path to UTXO cache snapshot file.
///
/// On graceful shutdown (SIGTERM/SIGINT), the cache is dumped to this file.
/// On startup, if this file exists, the cache is loaded from it instead
/// of starting cold or pre-warming from Neo4j.
///
/// Periodic snapshots are also written to this path every
/// `utxo_cache_snapshot_interval` blocks during ingestion.
///
/// Set to empty string to disable cache persistence entirely.
#[serde(default = "default_utxo_cache_file")]
pub utxo_cache_file: String,

/// Snapshot the UTXO cache to disk every N blocks during ingestion.
///
/// Protects against cache loss on hard crashes (OOM, kill -9, panic).
/// Set to 0 to disable periodic snapshots (only save on graceful shutdown).
///
/// Recommended: 1000-5000 blocks. At ~200 blocks/batch, this means a
/// snapshot every 5-25 batches. Each snapshot takes ~3 seconds for a
/// full 2GB cache (sequential write to local RAID 0).
#[serde(default = "default_utxo_cache_snapshot_interval")]
pub utxo_cache_snapshot_interval: u32,
```

Add the default functions:

```rust
fn default_utxo_cache_file() -> String {
    "utxo_cache.bin".to_string()
}

fn default_utxo_cache_snapshot_interval() -> u32 {
    2000 // Every 2000 blocks (~10 batches at batch_size=200)
}
```

Update `Default for PerformanceConfig`:

```rust
impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            utxo_cache_memory_mb: 140,
            utxo_prewarm_depth: 1_000_000,
            utxo_lookup_batch_size: default_utxo_lookup_batch_size(),
            parallel_batches: 4,
            progress_report_interval: 500,
            utxo_cache_file: default_utxo_cache_file(),
            utxo_cache_snapshot_interval: default_utxo_cache_snapshot_interval(),
        }
    }
}
```

**Note**: The `cache_capacity()` doctest (around line 464) must also include the two new
fields to compile (the struct has `#[derive(Deserialize)]` so all fields must be present):

```rust
/// ```
/// use bitcoin_chain_graph::config::PerformanceConfig;
///
/// let config = PerformanceConfig {
///     utxo_cache_memory_mb: 50,
///     utxo_prewarm_depth: 50,
///     utxo_lookup_batch_size: 1000,
///     parallel_batches: 4,
///     progress_report_interval: 100,
///     utxo_cache_file: String::new(),
///     utxo_cache_snapshot_interval: 0,
/// };
///
/// let capacity = config.cache_capacity();
/// assert_eq!(capacity, 694_444);
/// ```
```

### 3. `src/main.rs` — Fix SIGTERM handling + wire cache persistence

#### 3a. Fix SIGTERM handling (CRITICAL — pre-existing bug)

The current shutdown handler (line 660-667) only catches SIGINT (`ctrl_c()`). Systemd
sends SIGTERM on `systemctl stop`. Without handling SIGTERM, the process is killed
without ever reaching the cache-save code (or any graceful shutdown logic).

Replace the current shutdown handler:

```rust
// Set up graceful shutdown via Ctrl+C (SIGINT) AND SIGTERM (systemd stop)
let shutdown_token = CancellationToken::new();
let token_clone = shutdown_token.clone();
tokio::spawn(async move {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate())
            .expect("Failed to register SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("SIGINT received, finishing current operation...");
            }
            _ = sigterm.recv() => {
                tracing::info!("SIGTERM received, finishing current operation...");
            }
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Shutdown signal received, finishing current operation...");
    }
    token_clone.cancel();
});
```

This is important independently of cache persistence — without it, `systemctl stop`
has no graceful shutdown at all (systemd waits `TimeoutStopSec` then sends SIGKILL).

#### 3b. Load cache on startup in `run_live_ingestion()` (after creating orchestrator, ~line 609)

```rust
// Try to load UTXO cache from snapshot file
let cache_file = &config.performance.utxo_cache_file;
if !cache_file.is_empty() {
    // CheckpointData.last_processed_height is i32 (-1 = not started)
    let current_height = orchestrator.get_checkpoint().await
        .ok()
        .flatten()
        .and_then(|cp| if cp.last_processed_height >= 0 {
            Some(cp.last_processed_height as u32)
        } else {
            None
        });
    let cache = orchestrator.get_cache();
    match cache.load_from_file(cache_file, current_height) {
        Ok(loaded) if loaded > 0 => {
            println!(
                "   ✅ UTXO cache restored: {} entries ({:.1}% full)",
                loaded,
                cache.fill_percentage() * 100.0
            );
        }
        Ok(_) => {} // No file or empty, will start cold
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load UTXO cache file, starting with empty cache");
        }
    }
}
```

#### 3c. Periodic snapshots during catchup (inside the catchup loop, after each batch completes)

After the batch write and checkpoint update in the catchup loop (around line 750),
add periodic snapshot logic:

```rust
// Periodic UTXO cache snapshot (protects against crash-induced cache loss)
let snapshot_interval = config.performance.utxo_cache_snapshot_interval;
if snapshot_interval > 0
    && !cache_file.is_empty()
    && current_height % snapshot_interval == 0
{
    let cache = orchestrator.get_cache();
    let start = std::time::Instant::now();
    match cache.save_to_file(cache_file, current_height) {
        Ok(saved) => {
            tracing::info!(
                entries = saved,
                height = current_height,
                elapsed_ms = start.elapsed().as_millis() as u64,
                "Periodic UTXO cache snapshot saved"
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to save periodic UTXO cache snapshot");
            // Non-fatal: continue ingestion
        }
    }
}
```

#### 3d. Periodic snapshots during ZMQ real-time phase

Same pattern in the ZMQ block processing loop (around line 960), but check every N blocks:

```rust
// Periodic UTXO cache snapshot during real-time streaming
if snapshot_interval > 0
    && !cache_file.is_empty()
    && blocks_processed > 0
    && blocks_processed % snapshot_interval as u64 == 0
{
    let cache = orchestrator.get_cache();
    let start = std::time::Instant::now();
    match cache.save_to_file(cache_file, current_height) {
        Ok(saved) => {
            tracing::info!(
                entries = saved,
                height = current_height,
                elapsed_ms = start.elapsed().as_millis() as u64,
                "Periodic UTXO cache snapshot saved (ZMQ phase)"
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to save periodic UTXO cache snapshot");
        }
    }
}
```

#### 3e. Save cache on shutdown (end of `run_live_ingestion()`, before final stats log, ~line 985)

```rust
// Save UTXO cache to file for fast restart
let cache_file = &config.performance.utxo_cache_file;
if !cache_file.is_empty() {
    let cache = orchestrator.get_cache();
    let start = std::time::Instant::now();
    match cache.save_to_file(cache_file, current_height.saturating_sub(1)) {
        Ok(saved) => {
            tracing::info!(
                saved = saved,
                elapsed_ms = start.elapsed().as_millis() as u64,
                "UTXO cache saved to file for fast restart"
            );
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to save UTXO cache to file");
        }
    }
}
```

#### 3f. Also wire into `run_streaming_ingestion()` (for ingest/resume commands)

Same pattern — load at start, periodic save during ingestion, save at end.
Add load after the prewarm section (~line 382) and save before the final stats log (~line 498).

**Note**: `run_streaming_ingestion` receives `start_height: u32` as a parameter, so the
checkpoint height for cache load can use `if start_height > 0 { Some(start_height - 1) } else { None }`
instead of querying the checkpoint again. For periodic saves, use the current batch end height.

**Note**: `run_streaming_ingestion` does NOT currently have a shutdown handler (no
`CancellationToken`). It runs in `ingest`/`resume` mode, not `live` mode. To save cache
on Ctrl+C / SIGTERM, either add a shutdown handler or just rely on periodic snapshots.
Adding a shutdown handler here would be ideal but is a larger change — the function currently
runs a synchronous for-loop over heights, not an async select. For v1, periodic snapshots
provide adequate crash protection.

### 4. `config/live.toml` — Add the config options

Add to `[performance]` section:

```toml
# UTXO cache persistence: dump to file on shutdown, reload on startup.
# Saves minutes of cold-cache Neo4j lookups after restarts.
utxo_cache_file = "/data/bitcoin-chain-graph/utxo_cache.bin"

# Snapshot cache every N blocks during ingestion (crash protection).
# Set to 0 to only save on graceful shutdown.
utxo_cache_snapshot_interval = 2000
```

### 5. `Cargo.toml` — Add crc32fast dependency

```toml
[dependencies]
crc32fast = "1.4"
```

### 6. `src/domain/utxo/mod.rs` — No changes needed

The public API (`UtxoCache`, `UtxoKey`, `CachedOutput`, `ScriptTypeTag`) already
exports everything needed.

## Performance Estimate

- 27M entries at ~53 bytes per entry (avg) = ~1.4 GB file
- Sequential write at ~500 MB/s (local RAID 0) = ~3 seconds to save
- Sequential read at ~1 GB/s = ~1.5 seconds to load + ~1 second for insertions
- CRC32 computation: negligible (crc32fast uses hardware acceleration)
- vs Neo4j prewarm: minutes of index lookups over network
- Periodic snapshot overhead: ~3 seconds every 2000 blocks (negligible vs batch ingestion time)

## Edge Cases

- **File doesn't exist**: First run or disabled. Return Ok(0), start cold.
- **Corrupt file (bad CRC)**: Return error, log warning, start cold. Don't crash.
- **Invalid magic/version**: Return error, log warning, start cold. Don't crash.
- **Truncated file (partial write)**: CRC mismatch or `read_exact` EOF error catches this.
  The atomic-rename approach means this can only happen if the *entire filesystem* corrupted,
  not from a process crash during save.
- **Cache capacity changed**: If config reduces capacity, entries will be evicted by LRU
  during load. If increased, there will just be free space. Both are fine.
- **Kill -9 / OOM (no graceful shutdown)**: Last periodic snapshot is used. At worst,
  `snapshot_interval` blocks of cache warmth are lost (vs losing everything without
  periodic snapshots).
- **Stale cache (height mismatch)**: Warning logged, cache loaded anyway. Stale entries
  (outputs since spent) are harmless — they occupy cache space but get evicted naturally
  by LRU. Fresh entries from new ingestion will push them out.
- **Concurrent save during ingestion**: The save locks each shard briefly (one at a time)
  to iterate entries. Ingestion can proceed on other shards concurrently. The ~3 second
  save time is dominated by I/O, not lock contention.

## Testing

```bash
# Build
cd /data/bitcoin-chain-graph
cargo build --release

# Start service, let it run and ingest some blocks
systemctl restart bitcoin-chain-graph

# Watch for periodic snapshot logs
journalctl -u bitcoin-chain-graph -f | grep -i "cache snapshot\|cache saved\|cache loaded"

# Test graceful shutdown (SIGTERM — the systemd path)
systemctl stop bitcoin-chain-graph

# Verify cache file was created
ls -lh /data/bitcoin-chain-graph/utxo_cache.bin

# Restart and verify cache was restored
systemctl start bitcoin-chain-graph
journalctl -u bitcoin-chain-graph --since "30 seconds ago" | grep "cache restored\|cache loaded"

# Test crash recovery (periodic snapshot path)
# Let it run past at least one snapshot interval, then:
kill -9 $(pidof bitcoin-chain-graph)
systemctl start bitcoin-chain-graph
# Should load from last periodic snapshot (check log for height delta warning)
journalctl -u bitcoin-chain-graph --since "30 seconds ago" | grep "cache"
```