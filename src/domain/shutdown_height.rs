//! Tracks the last committed block height for correct UTXO cache shutdown saves.
//!
//! When `run_live_mode()` receives a shutdown signal, it must save the UTXO cache
//! at the height of the last successfully committed batch — not `current_height`,
//! which may point to a block that hasn't been ingested yet.

/// Tracks the last successfully committed block height for shutdown cache saves.
///
/// # Motivation
///
/// In `run_live_mode()`, `current_height` tracks the *next* block to fetch, not
/// the last committed block. Using `current_height.saturating_sub(1)` for the
/// shutdown save overwrites the correct per-chunk snapshot from `try_save_snapshot()`.
///
/// This tracker records each successful commit and provides the correct height
/// for shutdown saves.
#[derive(Debug, Clone)]
pub struct ShutdownHeightTracker {
    /// The height of the last successfully committed block, or `None` if no
    /// blocks have been committed this session.
    last_committed_height: Option<u32>,
}

impl Default for ShutdownHeightTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ShutdownHeightTracker {
    /// Creates a new tracker with no committed height.
    pub fn new() -> Self {
        Self {
            last_committed_height: None,
        }
    }

    /// Records a successful RPC catchup batch commit.
    ///
    /// # Arguments
    ///
    /// * `batch_end` - The height of the last block in the committed batch
    pub fn record_batch_commit(&mut self, batch_end: u32) {
        self.last_committed_height = Some(batch_end);
    }

    /// Records a successful single-block commit (ZMQ real-time path).
    ///
    /// # Arguments
    ///
    /// * `height` - The height of the committed block
    pub fn record_block_commit(&mut self, height: u32) {
        self.last_committed_height = Some(height);
    }

    /// Returns the height to use for shutdown cache save, or `None` if no
    /// blocks were committed this session (in which case the existing cache
    /// file should be preserved).
    pub fn save_height(&self) -> Option<u32> {
        self.last_committed_height
    }
}
