# Abstraciton for Live Node Compatibility

## Goal
Adapt the codebase to run against a **live** Bitcoin node, supporting both high-speed backlog ingestion and seamless transition to real-time streaming.

## The Challenge
1.  **Lock Contention:** We cannot read the internal LevelDB (`blocks/index`) while `bitcoind` is running.
2.  **Control Flow:** We need to switch from "Stop when done" to "Wait for more".

## The Solution: `BlockSource` Abstraction
We will refactor the hard dependency on `SingleBlockLoader` (which assumes Files+LevelDB) into a trait `BlockProvider`.

### 1. Define Traits
```rust
#[async_trait]
pub trait BlockProvider: Send + Sync {
    /// Get a block by height
    async fn get_block(&self, height: u32) -> Result<Option<Block>>;
    
    /// Get chain tip height
    async fn get_tip_height(&self) -> Result<u32>;
}
```

### 2. Implementations
We will have two implementations:

#### A. `FileBlockProvider` (Existing Logic - Offline High Speed)
- **Use Case:** Initial bulk load when node is stopped.
- **Mechanism:** Uses `BlockIndexReader` (LevelDB) + `SingleBlockLoader` (Disk).
- **Pros:** 10x-50x faster (direct disk I/O, no JSON/RPC overhead).

#### B. `RpcBlockProvider` (New Logic - Live Mode)
- **Use Case:** Live ingestion and Real-Time streaming.
- **Mechanism:** Uses Bitcoin Core RPC (`getblock`).
- **Optimization:**
    - **Backlog:** "Batch RPC" (request 10-50 blocks in parallel) to ensure decent speed.
    - **Real-Time:** ZMQ listener triggers a single `getblock` fetch.

### 3. Unified "Live" Workflow (Auto-Switching)
The user requires the application to automatically "Catch Up" and then "Flip to Realtime" without restarting.
Because `bitcoind` must be running for the Realtime phase, we **MUST** use `RpcBlockProvider` for the *entire* session (including catchup) to avoid the LevelDB lock issue.

**Performance trade-off:** We sacrifice the raw I/O speed of distinct file access for the convenience of a single "always on" process. We will mitigate this by parallelizing RPC requests during catchup.

#### Workflow:
1.  **Startup:**
    -   Connect to Neo4j (load `Checkpoint`).
    -   Connect to Bitcoin Core RPC (get `Chain Tip`).
2.  **Phase A: Catchup (Backlog)**
    -   Condition: `Checkpoint < Tip`
    -   Action: Loop consuming blocks as fast as possible.
    -   Optimization: **"Parallel Fetch, Sequential Write"**
        -   **Protocol:** Use **JSON-RPC Batching** (send 1 HTTP request for 50 blocks).
        -   **Format:** Use **HEX Mode** (`getblock <hash> 0`).
            -   *Benefit:* Avoids `bitcoind` converting 2MB binary -> 5MB JSON.
            -   *Benefit:* We reuse our super-fast binary `bitcoin::Block` parser.
        -   **Fetch:** Spawn tasks to grab blocks `N` to `N+50`.
        -   **Buffer & Sort:** Collect and ensure strictly ordered.
        -   **Ingest:** Pass batch to `IngestionOrchestrator`.
3.  **Phase B: Transition**
    -   Condition: `Checkpoint == Tip`
    -   Action: Switch to "Listening" mode.
4.  **Phase C: Real-Time**
    -   Action: Listen to **ZMQ** socket.
    -   Trigger: New Block Hash -> Fetch Single -> Ingest Single.

## Proposed Changes

### [NEW] `src/parser/provider.rs`
- Define `BlockProvider` trait.
- Implement `RpcBlockProvider` (needs `bitcoind` RPC client).
    - `get_block_batch(start, end)`: Optimized parallel fetcher.

### [MODIFY] `src/parser/single_block_loader.rs`
- Rename/Refactor to `FileBlockProvider` (Keep for "Emergency Offline Bulk Load" use case).

### [MODIFY] `src/domain/ingestion.rs`
- Update `IngestionOrchestrator` to accept `Box<dyn BlockProvider>`.

### [MODIFY] `src/main.rs`
- Add command `live` (or modify `resume` to detect mode).
- **Control Loop:**
    ```rust
    // Simplified Logic
    loop {
        let current = checkpoint.height;
        let tip = provider.get_tip().await?;
        
        if current < tip {
            // Catchup Mode
            let batch = provider.get_batch(current + 1, batch_size).await?;
            orchestrator.ingest_batch(batch).await?;
        } else {
            // Realtime Mode
            info!("Caught up! Waiting for blocks...");
            zmq_listener.wait_for_block().await?;
            // Loop continues and will catch the new block
        }
    }
    ```

## Verification Plan
1.  **Mock RPC:** Verify `RpcBlockProvider` can fetch blocks.
2.  **Switching:** Verify `main.rs` selects correct provider.
