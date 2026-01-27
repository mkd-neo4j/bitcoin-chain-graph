//! RPC Block Provider - Fetches blocks from a live Bitcoin Core node
//!
//! Uses Bitcoin Core JSON-RPC to fetch blocks. Supports both single-block
//! fetching (real-time mode) and parallel batch fetching (catchup mode).
//!
//! ## RPC Strategy
//! - Uses `getblockhash <height>` to get block hash from height
//! - Uses `getblock <hash> 0` (verbosity=0) to get raw hex block data
//! - Hex mode avoids bitcoind's expensive JSON serialization (~2.5x smaller)
//! - Reuses existing `bitcoin::consensus::deserialize` for parsing

use anyhow::{Context, Result};
use bitcoin::{Block, consensus::deserialize};
use futures::stream::{self, StreamExt};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use tracing;

use crate::config::BitcoinRpcConfig;

/// RPC Block Provider - fetches blocks from a live Bitcoin Core node
///
/// Connects to Bitcoin Core via JSON-RPC over HTTP. Supports:
/// - Single block fetching for real-time mode
/// - Parallel batch fetching for catchup mode
/// - Hex mode (verbosity=0) for efficient transfer
pub struct RpcBlockProvider {
    client: Client,
    url: String,
    user: String,
    password: String,
    batch_size: usize,
    max_concurrent: usize,
}

impl RpcBlockProvider {
    /// Create a new RPC block provider from configuration
    pub fn new(config: &BitcoinRpcConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            client,
            url: config.url.clone(),
            user: config.user.clone(),
            password: config.password.clone(),
            batch_size: config.batch_size,
            max_concurrent: config.rpc_max_concurrent,
        })
    }

    /// Get the configured batch size for parallel fetching
    pub fn batch_size(&self) -> usize {
        self.batch_size
    }

    /// Execute a JSON-RPC call to Bitcoin Core with retry logic
    ///
    /// Retries up to 3 times with exponential backoff (1s, 2s, 4s)
    /// for transient network failures.
    async fn rpc_call(&self, method: &str, params: &[Value]) -> Result<Value> {
        let body = serde_json::json!({
            "jsonrpc": "1.0",
            "id": "btcgraph",
            "method": method,
            "params": params,
        });

        let mut last_error = None;

        for attempt in 0..3u32 {
            if attempt > 0 {
                let delay = Duration::from_secs(1 << (attempt - 1));
                tracing::warn!(
                    method = method,
                    attempt = attempt + 1,
                    delay_secs = delay.as_secs(),
                    "Retrying RPC call"
                );
                tokio::time::sleep(delay).await;
            }

            match self.execute_rpc(&body).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::debug!(
                        method = method,
                        attempt = attempt + 1,
                        error = %e,
                        "RPC call failed"
                    );
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap())
    }

    /// Execute a single RPC request (no retry)
    async fn execute_rpc(&self, body: &Value) -> Result<Value> {
        let response = self.client
            .post(&self.url)
            .basic_auth(&self.user, Some(&self.password))
            .json(body)
            .send()
            .await
            .context("RPC request failed")?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "RPC HTTP error {} for {}: {}",
                status,
                body.get("method").and_then(|m| m.as_str()).unwrap_or("unknown"),
                text
            );
        }

        let result: Value = response.json().await
            .context("Failed to parse RPC response JSON")?;

        // Check for JSON-RPC error
        if let Some(error) = result.get("error") {
            if !error.is_null() {
                anyhow::bail!(
                    "RPC error for {}: {}",
                    body.get("method").and_then(|m| m.as_str()).unwrap_or("unknown"),
                    error
                );
            }
        }

        Ok(result["result"].clone())
    }

    /// Get the current chain tip height
    ///
    /// Calls `getblockcount` RPC method.
    pub async fn get_tip_height(&self) -> Result<u32> {
        let result = self.rpc_call("getblockcount", &[]).await
            .context("getblockcount failed")?;

        result.as_u64()
            .map(|h| h as u32)
            .context("Invalid block count from RPC")
    }

    /// Get the block hash at a given height
    ///
    /// Calls `getblockhash <height>` RPC method.
    pub async fn get_block_hash(&self, height: u32) -> Result<String> {
        let result = self.rpc_call("getblockhash", &[Value::from(height)]).await
            .with_context(|| format!("getblockhash failed for height {}", height))?;

        result.as_str()
            .map(|s| s.to_string())
            .with_context(|| format!("Invalid block hash from RPC for height {}", height))
    }

    /// Get raw block data as hex string
    ///
    /// Calls `getblock <hash> 0` (verbosity=0) to get the raw hex-encoded block.
    /// This is significantly more efficient than JSON mode because:
    /// - bitcoind skips JSON serialization (~2.5x smaller payload)
    /// - We reuse the existing binary block parser
    async fn get_block_hex(&self, hash: &str) -> Result<String> {
        let result = self.rpc_call(
            "getblock",
            &[Value::from(hash), Value::from(0)],
        ).await
            .with_context(|| format!("getblock (hex) failed for hash {}", hash))?;

        result.as_str()
            .map(|s| s.to_string())
            .with_context(|| format!("Invalid block hex from RPC for hash {}", hash))
    }

    /// Fetch a single block by height
    ///
    /// Pipeline: getblockhash(height) -> getblock(hash, 0) -> hex::decode -> deserialize
    ///
    /// Returns the block tuple compatible with IngestionOrchestrator:
    /// `(height, Block, source_name)` where source_name is "rpc".
    ///
    /// Returns None if the block doesn't exist (past chain tip).
    pub async fn get_block(&self, height: u32) -> Result<Option<(u32, Block, String)>> {
        // Step 1: Get block hash from height
        let hash = match self.get_block_hash(height).await {
            Ok(h) => h,
            Err(e) => {
                // Block might not exist yet (past chain tip)
                let err_str = format!("{}", e);
                if err_str.contains("Block height out of range") {
                    return Ok(None);
                }
                return Err(e);
            }
        };

        // Step 2: Get raw hex block data (verbosity=0)
        let hex_str = self.get_block_hex(&hash).await?;

        // Step 3: Decode hex to bytes
        let block_bytes = hex::decode(&hex_str)
            .with_context(|| format!("Failed to decode block hex at height {}", height))?;

        // Step 4: Deserialize using bitcoin crate (same as file-based path)
        let block: Block = deserialize(&block_bytes)
            .with_context(|| format!("Failed to deserialize block at height {}", height))?;

        Ok(Some((height, block, "rpc".to_string())))
    }

    /// Fetch multiple blocks in parallel for catchup mode
    ///
    /// Limits concurrency to `max_concurrent` in-flight block fetches at a time
    /// using `buffer_unordered`. This prevents overwhelming bitcoind or SSH tunnels
    /// with hundreds of simultaneous requests (thundering herd).
    ///
    /// Returns blocks sorted by height (matching IngestionOrchestrator expectations).
    pub async fn get_block_batch(
        &self,
        start_height: u32,
        count: usize,
    ) -> Result<Vec<(u32, Block, String)>> {
        let results: Vec<_> = stream::iter(0..count as u32)
            .map(|offset| {
                let height = start_height + offset;
                self.get_block(height)
            })
            .buffer_unordered(self.max_concurrent)
            .collect()
            .await;

        let mut blocks = Vec::with_capacity(count);
        for result in results {
            match result? {
                Some(block_tuple) => blocks.push(block_tuple),
                None => {} // Past chain tip — other futures already running
            }
        }

        // Sort by height (buffer_unordered may return out of order)
        blocks.sort_by_key(|(h, _, _)| *h);

        Ok(blocks)
    }

    /// Verify connectivity to the Bitcoin Core node
    ///
    /// Fetches the chain tip height and returns it. Useful for startup validation.
    pub async fn verify_connection(&self) -> Result<u32> {
        let tip = self.get_tip_height().await
            .context("Failed to connect to Bitcoin Core RPC - check url, credentials, and that bitcoind is running")?;

        tracing::info!(chain_tip = tip, url = %self.url, "Connected to Bitcoin Core RPC");
        Ok(tip)
    }
}
