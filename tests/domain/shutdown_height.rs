//! Tests for ShutdownHeightTracker
//!
//! Verifies correct shutdown save height tracking for UTXO cache across
//! RPC catchup and ZMQ real-time ingestion paths.

use bitcoin_chain_graph::domain::ShutdownHeightTracker;

// ─── Acceptance Criterion 1 ───────────────────────────────────────────────
// GIVEN RPC catchup batch committed (blocks 207600–207799)
// WHEN SIGTERM arrives after ingest_blocks_batch returns Ok but before
//      current_height = batch_end + 1
// THEN shutdown save uses height 207799 (batch_end), not 207599

#[test]
fn rpc_catchup_single_batch_uses_batch_end() {
    let mut tracker = ShutdownHeightTracker::new();
    tracker.record_batch_commit(207799);

    assert_eq!(
        tracker.save_height(),
        Some(207799),
        "Shutdown save should use batch_end (207799), not current_height - 1"
    );
}

// ─── Acceptance Criterion 2 ───────────────────────────────────────────────
// GIVEN ZMQ real-time inner loop committed block 300005
// WHEN outer loop shutdown check fires
// THEN shutdown save uses height 300005

#[test]
fn zmq_single_block_commit_uses_block_height() {
    let mut tracker = ShutdownHeightTracker::new();
    tracker.record_block_commit(300005);

    assert_eq!(
        tracker.save_height(),
        Some(300005),
        "Shutdown save should use the ZMQ-committed block height (300005)"
    );
}

// ─── Acceptance Criterion 3 ───────────────────────────────────────────────
// GIVEN SIGTERM arrives before any ingest_blocks_batch completes
// THEN save_to_file is NOT called (save_height returns None)

#[test]
fn no_commits_returns_none() {
    let tracker = ShutdownHeightTracker::new();

    assert_eq!(
        tracker.save_height(),
        None,
        "No blocks committed — should return None to skip save and preserve existing cache"
    );
}

// ─── Acceptance Criterion 4 ───────────────────────────────────────────────
// GIVEN multiple RPC catchup batches (batch 1: 0–199, batch 2: 200–399)
// WHEN SIGTERM arrives during third batch fetch (before it commits)
// THEN shutdown save uses height 399

#[test]
fn multiple_rpc_batches_uses_last_committed_batch_end() {
    let mut tracker = ShutdownHeightTracker::new();
    tracker.record_batch_commit(199);
    tracker.record_batch_commit(399);
    // Third batch never commits (SIGTERM during fetch)

    assert_eq!(
        tracker.save_height(),
        Some(399),
        "Should use end of last committed batch (399), not in-progress batch"
    );
}

// ─── Acceptance Criterion 5 ───────────────────────────────────────────────
// GIVEN ZMQ real-time processes blocks 500–503 individually
// WHEN SIGTERM arrives after block 503 commits but before block 504 is fetched
// THEN shutdown save uses height 503

#[test]
fn zmq_sequential_blocks_uses_last_committed() {
    let mut tracker = ShutdownHeightTracker::new();
    tracker.record_block_commit(500);
    tracker.record_block_commit(501);
    tracker.record_block_commit(502);
    tracker.record_block_commit(503);

    assert_eq!(
        tracker.save_height(),
        Some(503),
        "Should use the last individually committed block height (503)"
    );
}

// ─── Edge Case: SIGTERM during RPC fetch (before ingestion) ──────────────
// last_committed_height retains value from prior batch (or None if first)

#[test]
fn sigterm_during_fetch_retains_prior_batch_height() {
    let mut tracker = ShutdownHeightTracker::new();
    tracker.record_batch_commit(199);
    // Next batch is being fetched (not yet committed) — no new record_batch_commit call

    assert_eq!(
        tracker.save_height(),
        Some(199),
        "Should retain prior batch height when current batch hasn't committed"
    );
}

// ─── Edge Case: SIGTERM after ingest_blocks_batch error ──────────────────
// Failed batch does NOT update tracker — save uses prior committed height

#[test]
fn failed_batch_does_not_update_height() {
    let mut tracker = ShutdownHeightTracker::new();
    tracker.record_batch_commit(199);
    // Next batch (200–399) fails — record_batch_commit is NOT called

    assert_eq!(
        tracker.save_height(),
        Some(199),
        "Failed batch should not update tracker — save uses prior height (199)"
    );
}

// ─── Edge Case: Mixed RPC catchup then ZMQ real-time ─────────────────────
// Tracker works across both paths

#[test]
fn rpc_catchup_then_zmq_realtime() {
    let mut tracker = ShutdownHeightTracker::new();
    // RPC catchup batches
    tracker.record_batch_commit(199);
    tracker.record_batch_commit(399);
    // ZMQ real-time blocks
    tracker.record_block_commit(400);
    tracker.record_block_commit(401);

    assert_eq!(
        tracker.save_height(),
        Some(401),
        "ZMQ commits after RPC catchup should update the tracker"
    );
}
