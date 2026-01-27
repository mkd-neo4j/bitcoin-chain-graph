//! ZMQ Block Listener - Real-time block notifications from Bitcoin Core
//!
//! Subscribes to Bitcoin Core's ZMQ `hashblock` topic to receive instant
//! notifications when new blocks are mined. This enables the transition
//! from catchup mode to real-time streaming without polling.
//!
//! ## Bitcoin Core Configuration
//! Requires `zmqpubhashblock=tcp://127.0.0.1:28332` in bitcoin.conf.
//!
//! ## Testing Strategy
//!
//! - **Unit tests** (in this file): Exercise `parse_hashblock_message()` with
//!   valid/invalid inputs. No network or ZMQ socket required.
//! - **Integration tests**: Require a running bitcoind with ZMQ enabled.
//!   Not included in CI. Run manually with a local bitcoind.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::time::timeout;
use zeromq::{Socket, SocketRecv, SubSocket};

use crate::config::BitcoinRpcConfig;

// ─── Metrics ─────────────────────────────────────────────────────────────────

/// Lock-free ZMQ listener statistics using atomic counters.
///
/// Mirrors the `AtomicUtxoCacheStats` pattern: a single atomic fetch-add
/// per event, no mutexes, with a `snapshot()` method for periodic reporting.
struct AtomicZmqStats {
    messages_received: AtomicU64,
    reconnections: AtomicU64,
    recv_errors: AtomicU64,
    parse_errors: AtomicU64,
    sequence_gaps: AtomicU64,
}

impl AtomicZmqStats {
    fn new() -> Self {
        Self {
            messages_received: AtomicU64::new(0),
            reconnections: AtomicU64::new(0),
            recv_errors: AtomicU64::new(0),
            parse_errors: AtomicU64::new(0),
            sequence_gaps: AtomicU64::new(0),
        }
    }

    fn record_message(&self) {
        self.messages_received.fetch_add(1, Ordering::Relaxed);
    }

    fn record_reconnection(&self) {
        self.reconnections.fetch_add(1, Ordering::Relaxed);
    }

    fn record_recv_error(&self) {
        self.recv_errors.fetch_add(1, Ordering::Relaxed);
    }

    fn record_parse_error(&self) {
        self.parse_errors.fetch_add(1, Ordering::Relaxed);
    }

    fn record_sequence_gap(&self) {
        self.sequence_gaps.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> ZmqListenerStats {
        ZmqListenerStats {
            messages_received: self.messages_received.load(Ordering::Relaxed),
            reconnections: self.reconnections.load(Ordering::Relaxed),
            recv_errors: self.recv_errors.load(Ordering::Relaxed),
            parse_errors: self.parse_errors.load(Ordering::Relaxed),
            sequence_gaps: self.sequence_gaps.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of ZMQ listener performance metrics.
#[derive(Debug, Clone, Default)]
pub struct ZmqListenerStats {
    pub messages_received: u64,
    pub reconnections: u64,
    pub recv_errors: u64,
    pub parse_errors: u64,
    pub sequence_gaps: u64,
}

// ─── Message Parsing ─────────────────────────────────────────────────────────

/// Parse a ZMQ hashblock message into a block hash hex string.
///
/// Bitcoin Core ZMQ multipart format:
///   Frame 0: topic ("hashblock")
///   Frame 1: 32-byte block hash (internal byte order)
///   Frame 2: 4-byte sequence number (little-endian u32)
///
/// Returns `(block_hash_hex, sequence_number)` on success.
/// The hash is reversed to standard display order (matching RPC output).
pub fn parse_hashblock_message(frames: &[Vec<u8>]) -> Result<(String, Option<u32>)> {
    if frames.len() < 2 {
        anyhow::bail!(
            "Invalid ZMQ message: expected at least 2 frames, got {}",
            frames.len()
        );
    }

    // Validate topic frame
    let topic = std::str::from_utf8(&frames[0]).unwrap_or("<non-utf8>");
    if topic != "hashblock" {
        anyhow::bail!(
            "Unexpected ZMQ topic: expected 'hashblock', got '{}'",
            topic
        );
    }

    // Validate hash length (must be exactly 32 bytes)
    if frames[1].len() != 32 {
        anyhow::bail!(
            "Invalid block hash length: expected 32 bytes, got {}",
            frames[1].len()
        );
    }

    // Reverse from Bitcoin Core internal byte order to standard display order
    let mut hash_bytes = frames[1].clone();
    hash_bytes.reverse();
    let hash_hex = hex::encode(&hash_bytes);

    // Parse sequence number (frame 2, 4 bytes little-endian)
    let sequence = if frames.len() >= 3 && frames[2].len() == 4 {
        Some(u32::from_le_bytes([
            frames[2][0],
            frames[2][1],
            frames[2][2],
            frames[2][3],
        ]))
    } else {
        None
    };

    Ok((hash_hex, sequence))
}

// ─── Listener ────────────────────────────────────────────────────────────────

/// ZMQ Block Listener for real-time block notifications
///
/// Maintains a persistent connection to Bitcoin Core's ZMQ PUB socket,
/// subscribed to the `hashblock` topic. Call [`connect`] once at startup,
/// then call [`recv_block_hash`] in a loop to receive notifications.
///
/// This avoids the "slow joiner" problem and missed notifications that
/// occur when reconnecting for every block.
pub struct ZmqBlockListener {
    endpoint: String,
    socket: Option<SubSocket>,
    max_reconnect_attempts: u32,
    reconnect_base_delay_ms: u64,
    recv_timeout: Duration,
    last_sequence: Option<u32>,
    stats: AtomicZmqStats,
}

impl ZmqBlockListener {
    /// Create a new ZMQ listener with default reconnection settings.
    ///
    /// The endpoint should match the `zmqpubhashblock` setting in bitcoin.conf
    /// (e.g., "tcp://127.0.0.1:28332").
    pub fn new(endpoint: &str) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            socket: None,
            max_reconnect_attempts: 5,
            reconnect_base_delay_ms: 1000,
            recv_timeout: Duration::from_secs(60 * 60),
            last_sequence: None,
            stats: AtomicZmqStats::new(),
        }
    }

    /// Create from RPC configuration (preferred constructor for production).
    pub fn from_config(config: &BitcoinRpcConfig) -> Self {
        Self {
            endpoint: config.zmq_endpoint.clone(),
            socket: None,
            max_reconnect_attempts: config.zmq_max_reconnect_attempts,
            reconnect_base_delay_ms: config.zmq_reconnect_base_delay_ms,
            recv_timeout: if config.zmq_recv_timeout_mins > 0 {
                Duration::from_secs(config.zmq_recv_timeout_mins * 60)
            } else {
                Duration::from_secs(u64::MAX)
            },
            last_sequence: None,
            stats: AtomicZmqStats::new(),
        }
    }

    /// Get a snapshot of listener statistics.
    pub fn stats(&self) -> ZmqListenerStats {
        self.stats.snapshot()
    }

    /// Establish persistent ZMQ connection and subscribe to `hashblock`.
    ///
    /// Must be called once before [`recv_block_hash`]. The connection remains
    /// open across multiple recv calls, ensuring no notifications are missed.
    pub async fn connect(&mut self) -> Result<()> {
        let mut socket = SubSocket::new();
        socket
            .connect(&self.endpoint)
            .await
            .with_context(|| format!("Failed to connect ZMQ to {}", self.endpoint))?;

        socket
            .subscribe("hashblock")
            .await
            .context("Failed to subscribe to hashblock topic")?;

        tracing::info!(endpoint = %self.endpoint, "ZMQ connected and subscribed to hashblock");
        self.socket = Some(socket);
        Ok(())
    }

    /// Reconnect with exponential backoff.
    ///
    /// Follows the same retry pattern as `RpcBlockProvider` (1s, 2s, 4s)
    /// and `Neo4jWriter` (200ms * 2^attempt).
    async fn reconnect_with_backoff(&mut self) -> Result<()> {
        for attempt in 0..self.max_reconnect_attempts {
            if attempt > 0 {
                let delay =
                    Duration::from_millis(self.reconnect_base_delay_ms * 2u64.pow(attempt - 1));
                tracing::warn!(
                    attempt = attempt + 1,
                    max_attempts = self.max_reconnect_attempts,
                    delay_ms = delay.as_millis() as u64,
                    endpoint = %self.endpoint,
                    "ZMQ reconnection attempt"
                );
                tokio::time::sleep(delay).await;
            }

            match self.connect().await {
                Ok(()) => {
                    self.stats.record_reconnection();
                    tracing::info!(
                        attempt = attempt + 1,
                        endpoint = %self.endpoint,
                        "ZMQ reconnected successfully"
                    );
                    return Ok(());
                }
                Err(e) => {
                    tracing::error!(
                        attempt = attempt + 1,
                        max_attempts = self.max_reconnect_attempts,
                        error = %e,
                        "ZMQ reconnection failed"
                    );
                }
            }
        }

        anyhow::bail!(
            "ZMQ reconnection failed after {} attempts to {}",
            self.max_reconnect_attempts,
            self.endpoint
        )
    }

    /// Receive the next block hash notification from the persistent connection.
    ///
    /// Blocks until a new `hashblock` message arrives or the recv timeout
    /// expires. Returns the block hash as a hex string in standard Bitcoin
    /// display order (reversed from Bitcoin Core's internal byte order).
    ///
    /// On connection loss, reconnects with exponential backoff.
    pub async fn recv_block_hash(&mut self) -> Result<String> {
        // Reconnect if needed
        if self.socket.is_none() {
            tracing::warn!("ZMQ socket not connected, reconnecting...");
            self.reconnect_with_backoff().await?;
        }

        let socket = self
            .socket
            .as_mut()
            .context("ZMQ socket not connected after reconnect attempt")?;

        // Receive with timeout to detect stale connections
        let message = match timeout(self.recv_timeout, socket.recv()).await {
            Ok(Ok(msg)) => msg,
            Ok(Err(e)) => {
                tracing::error!(error = %e, "ZMQ recv failed, dropping socket for reconnect");
                self.stats.record_recv_error();
                self.socket = None;
                return Err(anyhow::anyhow!("ZMQ recv failed: {}", e));
            }
            Err(_) => {
                tracing::warn!(
                    timeout_mins = self.recv_timeout.as_secs() / 60,
                    "ZMQ recv timed out, dropping socket for reconnect"
                );
                self.stats.record_recv_error();
                self.socket = None;
                return Err(anyhow::anyhow!(
                    "ZMQ recv timed out after {} minutes",
                    self.recv_timeout.as_secs() / 60
                ));
            }
        };

        // Parse multipart message frames
        let frames: Vec<Vec<u8>> = message.into_vec().into_iter().map(|f| f.to_vec()).collect();

        let (hash_hex, sequence) = match parse_hashblock_message(&frames) {
            Ok(parsed) => parsed,
            Err(e) => {
                tracing::error!(error = %e, "Failed to parse ZMQ message");
                self.stats.record_parse_error();
                return Err(e);
            }
        };

        self.stats.record_message();

        // Sequence gap detection
        if let Some(seq) = sequence {
            if let Some(last_seq) = self.last_sequence {
                let expected = last_seq.wrapping_add(1);
                if seq != expected {
                    let gap = seq.wrapping_sub(last_seq);
                    tracing::warn!(
                        last_sequence = last_seq,
                        current_sequence = seq,
                        expected,
                        gap,
                        "ZMQ sequence gap detected — possible missed blocks"
                    );
                    self.stats.record_sequence_gap();
                }
            }
            self.last_sequence = Some(seq);
        }

        tracing::info!(
            block_hash = %hash_hex,
            sequence = ?sequence,
            "New block detected via ZMQ"
        );

        Ok(hash_hex)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a valid hashblock ZMQ message.
    fn make_hashblock_message(hash: [u8; 32], seq: u32) -> Vec<Vec<u8>> {
        vec![
            b"hashblock".to_vec(),
            hash.to_vec(),
            seq.to_le_bytes().to_vec(),
        ]
    }

    #[test]
    fn test_parse_valid_message() {
        let mut internal_hash = [0u8; 32];
        internal_hash[31] = 0x01;

        let frames = make_hashblock_message(internal_hash, 42);
        let (hash_hex, seq) = parse_hashblock_message(&frames).unwrap();

        assert_eq!(hash_hex.len(), 64, "Hash should be 64 hex chars");
        assert_eq!(seq, Some(42));
        // internal_hash[31]=0x01 reversed becomes display hash starting with "01"
        assert!(
            hash_hex.starts_with("01"),
            "Hash should be reversed from internal order"
        );
    }

    #[test]
    fn test_parse_too_few_frames() {
        let frames = vec![b"hashblock".to_vec()];
        let result = parse_hashblock_message(&frames);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("expected at least 2 frames"));
    }

    #[test]
    fn test_parse_wrong_topic() {
        let frames = vec![b"hashtx".to_vec(), vec![0u8; 32]];
        let result = parse_hashblock_message(&frames);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unexpected ZMQ topic"));
    }

    #[test]
    fn test_parse_wrong_hash_length() {
        let frames = vec![b"hashblock".to_vec(), vec![0u8; 16]];
        let result = parse_hashblock_message(&frames);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid block hash length"));
    }

    #[test]
    fn test_parse_missing_sequence_frame() {
        let frames = vec![b"hashblock".to_vec(), vec![0u8; 32]];
        let (_, seq) = parse_hashblock_message(&frames).unwrap();
        assert_eq!(seq, None, "Missing sequence frame should return None");
    }

    #[test]
    fn test_parse_malformed_sequence_frame() {
        let frames = vec![
            b"hashblock".to_vec(),
            vec![0u8; 32],
            vec![0u8; 2], // Wrong size for u32
        ];
        let (_, seq) = parse_hashblock_message(&frames).unwrap();
        assert_eq!(seq, None, "Malformed sequence frame should return None");
    }

    #[test]
    fn test_parse_genesis_block_hash() {
        // Bitcoin genesis block hash in internal (little-endian) byte order
        let internal = hex::decode(
            "6fe28c0ab6f1b372c1a6a246ae63f74f931e8365e15a089c68d6190000000000",
        )
        .unwrap();

        let frames = vec![
            b"hashblock".to_vec(),
            internal,
            0u32.to_le_bytes().to_vec(),
        ];

        let (hash_hex, _) = parse_hashblock_message(&frames).unwrap();
        assert_eq!(
            hash_hex,
            "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f",
            "Should match genesis block hash in display order"
        );
    }
}
