# Live Mode Architecture

## Goal
The system supports running against a **live** Bitcoin node, automatically catching up via RPC and then seamlessly transitioning to real-time ZMQ streaming without restarting.

## The Challenge
1. **Lock Contention:** Cannot read the internal LevelDB (`blocks/index`) while `bitcoind` is running.
2. **Control Flow:** Need to switch from "stop when done" to "wait for more blocks".

---

## The Solution: Concrete Block Sources

Rather than a trait abstraction, the system uses three concrete structs selected at the CLI command level:

| Struct | Location | Used By | Mode |
|--------|----------|---------|------|
| `SingleBlockLoader` | `src/parser/single_block_loader.rs` | `ingest`, `resume` | Offline (node stopped) |
| `RpcBlockProvider` | `src/parser/rpc_provider.rs` | `live` (catchup) | Online (node running) |
| `ZmqBlockListener` | `src/parser/zmq_listener.rs` | `live` (real-time) | Online (node running) |

### SingleBlockLoader (Offline)
- Uses LevelDB block index + disk I/O to read `.blk` files directly
- 10-50x faster than RPC (no JSON/HTTP overhead)
- **Requires** Bitcoin Core to be stopped (LevelDB lock contention)
- Used by `cargo run -- ingest` and `cargo run -- resume`

### RpcBlockProvider (Live Catchup)
- Connects to Bitcoin Core via JSON-RPC over HTTP
- Pipeline: `getblockhash(height)` -> `getblock(hash, 0)` hex mode -> `hex::decode` -> `bitcoin::consensus::deserialize`
- Hex mode (verbosity=0) avoids bitcoind's expensive JSON serialization (~2.5x smaller payload)
- Reuses the same binary block parser as the offline path
- Parallel batch fetching via `futures::stream::buffer_unordered(max_concurrent)`
- Retry logic: 3 attempts with exponential backoff (1s, 2s, 4s)

### ZmqBlockListener (Real-Time)
- Subscribes to Bitcoin Core's ZMQ PUB socket on the `hashblock` topic
- Receives multipart messages: `[topic, 32-byte block hash, 4-byte sequence number]`
- Block hash is reversed from internal byte order to standard display order
- Persistent connection (stays open across blocks to avoid the "slow joiner" problem)
- Sequence gap detection to identify missed notifications
- Auto-reconnect with exponential backoff on connection loss

---

## Unified Live Workflow

The `live` CLI command (`run_live_ingestion()` in `src/main.rs`) orchestrates the full workflow. Because `bitcoind` must be running for real-time mode, the entire session uses `RpcBlockProvider` for block fetching (including catchup) to avoid LevelDB lock issues.

**Performance trade-off:** Sacrifices raw disk I/O speed for the convenience of a single "always on" process. Mitigated by parallelizing RPC requests during catchup.

### Phase A: Catchup (Backlog)

**Condition:** `checkpoint_height < chain_tip`

```
loop {
    tip = provider.get_tip_height()        // Re-check tip each iteration
    target = min(tip, max_height)
    if current_height > target { break }   // Caught up

    remaining = target - current_height + 1
    fetch_count = min(remaining, batch_size)

    blocks = provider.get_block_batch(current_height, fetch_count)
    orchestrator.ingest_blocks_batch(&blocks, ingestion_batch_size)

    current_height = last_block_height + 1
}
```

Key details:
- Re-checks chain tip on each iteration (tip may advance during catchup)
- Uses `get_block_batch()` which spawns `fetch_count` parallel tasks, limited to `max_concurrent` in-flight requests via `buffer_unordered`
- Results are sorted by height after parallel fetch (buffer_unordered returns out of order)
- Ingests via `orchestrator.ingest_blocks_batch()` (same 6-phase pipeline as offline mode)
- Respects `CancellationToken` for graceful Ctrl+C shutdown

### Phase B: Transition

**Condition:** `current_height > chain_tip`

Logs catchup statistics (blocks processed, duration) and transitions to real-time mode. If `--max-height` was specified and reached, exits instead of transitioning.

### Phase C: Real-Time (ZMQ Streaming)

**Condition:** Caught up to chain tip, waiting for new blocks.

```
zmq_listener = ZmqBlockListener::from_config(rpc_config)
zmq_listener.connect()

loop {
    block_hash = tokio::select! {
        _ = shutdown_token.cancelled() => break,
        result = zmq_listener.recv_block_hash() => handle(result),
    }

    tip = provider.get_tip_height()
    for height in current_height..=tip {
        block = provider.get_block(height)
        orchestrator.ingest_blocks_batch(&[block], 1)
    }
    current_height = tip + 1
}
```

Key details:
- `tokio::select!` enables cancellable waiting (Ctrl+C interrupts ZMQ recv)
- On notification: fetches the actual tip height, then ingests all blocks from `current_height` to tip (handles multi-block gaps if a notification was missed)
- Each block is ingested individually via `ingest_blocks_batch(&[block], 1)`
- Consecutive failure tracking: exits after `zmq_max_consecutive_failures` (default: 10)
- Cancellable backoff between failures: `tokio::select!` on shutdown token + 2-second sleep
- Periodic ZMQ health stats logged every 10 blocks (messages, reconnections, errors, sequence gaps)

### Graceful Shutdown

Uses `tokio_util::sync::CancellationToken`:
1. A background task listens for `SIGINT` (Ctrl+C)
2. On signal, cancels the token
3. Both catchup loop and ZMQ loop check `shutdown_token.is_cancelled()` or use `tokio::select!`
4. Final stats (total blocks, duration, ZMQ metrics) are logged on exit

---

## Module Structure

```
src/parser/
├── rpc_provider.rs      # RpcBlockProvider struct (catchup mode)
├── zmq_listener.rs      # ZmqBlockListener struct (real-time mode)
│                        #   parse_hashblock_message() (public, unit-tested)
│                        #   ZmqListenerStats (metrics snapshot)
│                        #   AtomicZmqStats (lock-free counters)
└── single_block_loader.rs  # SingleBlockLoader (offline mode, not used in live)

src/main.rs
├── run_live_ingestion()  # Orchestrates Phase A → B → C
└── Commands::Live        # CLI subcommand definition

src/config/mod.rs
└── BitcoinRpcConfig      # All RPC + ZMQ configuration fields
```

---

## Configuration

The `[bitcoin_rpc]` section in the config TOML is required for live mode:

```toml
[bitcoin_rpc]
url = "http://localhost:8332"
user = "btcgraph"
password = "your-rpc-password"
batch_size = 50                      # Blocks to fetch per RPC batch (default: 50)
timeout_secs = 30                    # Per-request timeout (default: 30)
zmq_endpoint = "tcp://127.0.0.1:28332"
rpc_max_concurrent = 32              # Max in-flight RPC requests (default: 32)
zmq_max_reconnect_attempts = 5       # Max reconnect retries (default: 5)
zmq_reconnect_base_delay_ms = 1000   # Base backoff delay (default: 1000)
zmq_recv_timeout_mins = 60           # Stale connection detection (default: 60)
zmq_max_consecutive_failures = 10    # Exit threshold (default: 10)
```

Bitcoin Core must be configured with ZMQ support:
```ini
# bitcoin.conf
zmqpubhashblock=tcp://127.0.0.1:28332
```

---

## Error Handling and Resilience

### RPC Retry Logic
- 3 attempts per RPC call with exponential backoff (1s, 2s, 4s)
- Handles transient HTTP errors and Bitcoin Core JSON-RPC errors
- Implemented in `RpcBlockProvider::rpc_call()`

### ZMQ Reconnection
- Up to `zmq_max_reconnect_attempts` (default: 5) with exponential backoff
- Base delay: `zmq_reconnect_base_delay_ms * 2^(attempt-1)` (1s, 2s, 4s, 8s, 16s)
- Socket is dropped and recreated on connection loss or recv timeout
- Implemented in `ZmqBlockListener::reconnect_with_backoff()`

### Sequence Gap Detection
- ZMQ messages include a 4-byte little-endian sequence number
- Listener tracks `last_sequence` and warns on non-consecutive values
- Gaps indicate possible missed blocks (handled by fetching all blocks between `current_height` and new tip)

### Recv Timeout
- Configurable via `zmq_recv_timeout_mins` (default: 60 minutes)
- Detects stale ZMQ connections (e.g., bitcoind restarted without notification)
- On timeout: drops socket, returns error, outer loop handles reconnection

### Consecutive Failure Threshold
- Tracks consecutive ZMQ recv failures
- Exits after `zmq_max_consecutive_failures` (default: 10)
- Counter resets to 0 on any successful recv

---

## Testing

### Unit Tests (no network required)
7 tests in `src/parser/zmq_listener.rs::tests`:
- `test_parse_valid_message` — valid hashblock message parsing
- `test_parse_too_few_frames` — rejects messages with < 2 frames
- `test_parse_wrong_topic` — rejects non-hashblock topics
- `test_parse_wrong_hash_length` — rejects invalid hash sizes
- `test_parse_missing_sequence_frame` — handles missing sequence gracefully
- `test_parse_malformed_sequence_frame` — handles wrong-size sequence frame
- `test_parse_genesis_block_hash` — verifies byte order reversal with known genesis hash

### Integration Tests (require running bitcoind)
Not included in CI. Require a local Bitcoin Core node with ZMQ enabled for end-to-end validation of RPC fetching and ZMQ streaming.
