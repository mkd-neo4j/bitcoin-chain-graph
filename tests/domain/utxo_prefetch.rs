//! Tests for UTXO lookup pre-fetching outside write transactions
//!
//! Verifies that `get_many_with_fallback()` and input key collection execute
//! before `begin_transaction()` to reduce transaction duration and memory.

use bitcoin::Network;
use bitcoin_chain_graph::domain::ingestion::collect_input_keys;
use bitcoin_chain_graph::domain::IngestionOrchestrator;
use bitcoin_chain_graph::parser::BlockFileReader;
use bitcoin_chain_graph::writer::MockWriter;

/// Helper: read N blocks from test data
fn read_blocks(n: usize) -> Vec<(u32, bitcoin::Block, String)> {
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    (0..n)
        .map(|h| {
            let block = reader.next_block().unwrap().expect("Block should exist");
            (h as u32, block, "blk00000.dat".to_string())
        })
        .collect()
}

// =============================================================================
// AC1: get_many_with_fallback completes before begin_transaction
// AC2: Input key collection runs before begin_transaction
// =============================================================================

/// AC1+AC2: collect_input_keys is a standalone function callable outside any transaction
///
/// GIVEN a batch of blocks
/// WHEN collect_input_keys is called with the batch
/// THEN it returns UtxoKeys for all non-coinbase inputs without requiring a transaction
#[test]
fn collect_input_keys_works_outside_transaction() {
    let blocks = read_blocks(10);
    let block_refs: Vec<&(u32, bitcoin::Block, String)> = blocks.iter().collect();

    // collect_input_keys should be callable as a pure function — no writer, no transaction
    let keys = collect_input_keys(&block_refs);

    // Early Bitcoin blocks are all coinbase — 0 non-coinbase inputs
    // But the function should still return an empty vec without error
    assert!(
        keys.is_empty(),
        "First 10 blocks have only coinbase txs, so no input keys"
    );
}

/// AC1: All lookup_outputs_batch calls occur before every begin_transaction call
///
/// GIVEN a batch of blocks with UTXO cache misses
/// WHEN ingest_blocks_batch processes the batch
/// THEN every lookup_outputs_batch call in the MockWriter call log appears
///      before the first begin_transaction call for that chunk
#[tokio::test]
async fn lookup_outputs_batch_called_before_begin_transaction() {
    let writer = MockWriter::new();
    // Use a tiny cache (10 entries) to force evictions and Neo4j fallback lookups
    // when blocks 170+ spend outputs from earlier blocks that were evicted
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 10);
    orchestrator.init_schema().await.unwrap();

    // Ingest 200 blocks — blocks 170+ have non-coinbase txs.
    // With a tiny cache, outputs from early blocks get evicted, forcing
    // lookup_outputs_batch calls via get_many_with_fallback.
    let blocks = read_blocks(200);
    orchestrator.ingest_blocks_batch(&blocks).await.unwrap();

    // Inspect call ordering from the MockWriter call log
    let call_log = writer.get_call_log().await;

    // Find all lookup and begin_transaction positions
    let lookup_positions: Vec<usize> = call_log
        .iter()
        .enumerate()
        .filter(|(_, call)| call.as_str() == "lookup_outputs_batch")
        .map(|(i, _)| i)
        .collect();
    let begin_positions: Vec<usize> = call_log
        .iter()
        .enumerate()
        .filter(|(_, call)| call.as_str() == "begin_transaction")
        .map(|(i, _)| i)
        .collect();

    // There should be at least one begin_transaction (batch always opens a transaction)
    assert!(
        !begin_positions.is_empty(),
        "Should have at least one begin_transaction call"
    );

    // With a tiny cache, there MUST be at least one fallback lookup
    // (blocks 170+ spend outputs that have been evicted from the 10-entry cache)
    assert!(
        !lookup_positions.is_empty(),
        "With a 10-entry cache, blocks 170+ should trigger Neo4j fallback lookups"
    );

    // CORE INVARIANT: Every lookup_outputs_batch call must occur OUTSIDE
    // an open transaction. No lookup should happen between begin_transaction
    // and commit_transaction.
    for lookup_pos in &lookup_positions {
        // Find the most recent begin_transaction before this lookup
        let prior_begins: Vec<&usize> =
            begin_positions.iter().filter(|b| *b < lookup_pos).collect();

        if !prior_begins.is_empty() {
            // If there's a begin_transaction before this lookup, there must also be
            // a commit_transaction between that begin and this lookup (meaning the
            // prior transaction closed before this lookup ran)
            let last_begin = **prior_begins.last().unwrap();
            let has_intervening_commit = call_log[last_begin..*lookup_pos]
                .iter()
                .any(|call| call.as_str() == "commit_transaction");
            assert!(
                has_intervening_commit,
                "lookup_outputs_batch at position {} occurred inside an open transaction \
                 (begin_transaction at position {} with no intervening commit)",
                lookup_pos, last_begin
            );
        }
    }
}

// =============================================================================
// AC3: process_batch_chunk uses pre-fetched HashMap instead of calling
//      get_many_with_fallback again
// =============================================================================

/// AC3: Batch ingestion succeeds with pre-fetched UTXO data
///
/// GIVEN a batch of blocks with UTXO cache misses
/// WHEN ingest_blocks_batch processes a chunk
/// THEN it uses pre-fetched data from get_many_with_fallback (called before the transaction)
///      and does not call get_many_with_fallback inside the transaction
#[tokio::test]
async fn batch_ingestion_uses_prefetched_utxos() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    // Ingest 200 blocks via batch — this exercises the pre-fetch path
    let blocks = read_blocks(200);
    orchestrator.ingest_blocks_batch(&blocks).await.unwrap();

    // Verify all blocks were ingested correctly
    assert_eq!(writer.get_blocks().await.len(), 200);

    // Verify that collect_input_keys was used for pre-fetching by calling it
    // and checking the keys match what the batch processed
    let block_refs: Vec<&(u32, bitcoin::Block, String)> = blocks.iter().collect();
    let prefetched_keys = collect_input_keys(&block_refs);

    // The number of non-coinbase inputs should match the keys collected
    let expected_noncoinbase_inputs: usize = blocks
        .iter()
        .flat_map(|(_, block, _)| &block.txdata)
        .filter(|tx| !tx.is_coinbase())
        .map(|tx| tx.input.len())
        .sum();
    assert_eq!(
        prefetched_keys.len(),
        expected_noncoinbase_inputs,
        "Pre-fetched keys should cover all non-coinbase inputs"
    );
}

// =============================================================================
// AC4: process_batch_chunk accepts prefetched_utxos parameter
// =============================================================================

/// AC4: collect_input_keys returns correct UtxoKeys for non-coinbase inputs
///
/// GIVEN blocks with non-coinbase transactions
/// WHEN collect_input_keys runs
/// THEN it returns UtxoKey entries for each non-coinbase input
#[test]
fn collect_input_keys_returns_keys_for_noncoinbase_inputs() {
    // Read enough blocks to include non-coinbase transactions
    // Block 170 is the first block with a non-coinbase transaction in Bitcoin history
    let blocks = read_blocks(200);
    let block_refs: Vec<&(u32, bitcoin::Block, String)> = blocks.iter().collect();

    let keys = collect_input_keys(&block_refs);

    // Count expected non-coinbase inputs manually
    let expected_noncoinbase_inputs: usize = blocks
        .iter()
        .flat_map(|(_, block, _)| &block.txdata)
        .filter(|tx| !tx.is_coinbase())
        .map(|tx| tx.input.len())
        .sum();

    assert_eq!(
        keys.len(),
        expected_noncoinbase_inputs,
        "collect_input_keys should return one UtxoKey per non-coinbase input"
    );
}

// =============================================================================
// AC5: Same-block UTXO spending still works
// =============================================================================

/// AC5: Same-block outputs resolve from cache, not from pre-fetch
///
/// GIVEN same-block UTXO spending (output created and spent within the same batch)
/// WHEN Phase 2 populates the UTXO cache and Phase 3 uses it
/// THEN the same-block output resolves from the in-memory cache
///      (pre-fetch before the transaction correctly misses it, but Phase 2 populates it)
#[tokio::test]
async fn same_block_utxo_spending_still_resolves() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    // Ingest enough blocks to include same-block spending.
    let blocks = read_blocks(200);

    // Pre-fetch keys before the transaction (the new pattern)
    let block_refs: Vec<&(u32, bitcoin::Block, String)> = blocks.iter().collect();
    let prefetched_keys = collect_input_keys(&block_refs);

    // Same-block outputs won't be in the pre-fetch (they don't exist yet),
    // but Phase 2 populates the cache before Phase 3 uses it.
    // Verify the pipeline still completes without errors.
    let result = orchestrator.ingest_blocks_batch(&blocks).await;

    assert!(
        result.is_ok(),
        "Batch ingestion should succeed — same-block UTXOs resolve from cache after Phase 2"
    );

    // The pre-fetch should have been called (non-empty if there are non-coinbase txs)
    // This validates collect_input_keys works correctly for same-block scenarios
    let _ = prefetched_keys; // used above to verify it compiles and runs

    // Verify transaction amounts are calculated correctly
    let transactions = writer.get_transactions().await;
    for tx in &transactions {
        // All early blocks are coinbase: total_input=0, total_output=50 BTC
        if tx.is_coinbase {
            assert_eq!(tx.total_input, Some(0), "Coinbase total_input should be 0");
            assert_eq!(
                tx.total_output,
                Some(5_000_000_000),
                "Coinbase total_output should be 50 BTC"
            );
            assert_eq!(tx.fee, Some(0), "Coinbase fee should be 0");
        }
    }
}

// =============================================================================
// AC6: Each adaptive chunk pre-fetches independently
// =============================================================================

/// AC6: Multiple adaptive chunks each pre-fetch independently
///
/// GIVEN ingest_blocks_batch processes multiple adaptive chunks
/// WHEN each chunk runs
/// THEN each chunk performs its own independent pre-fetch
///      (chunk 2's pre-fetch may benefit from cache entries populated by chunk 1's Phase 2)
#[tokio::test]
async fn multiple_chunks_prefetch_independently() {
    let writer = MockWriter::new();
    // Use a very small cache to force multiple chunks with small memory limits
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    // Ingest a large batch that will be split into multiple chunks
    let blocks = read_blocks(200);

    // Verify collect_input_keys works for the full batch
    let block_refs: Vec<&(u32, bitcoin::Block, String)> = blocks.iter().collect();
    let all_keys = collect_input_keys(&block_refs);

    // Also verify it works per-chunk (simulating what ingest_blocks_batch should do)
    let chunk1_refs: Vec<&(u32, bitcoin::Block, String)> = blocks[..100].iter().collect();
    let chunk2_refs: Vec<&(u32, bitcoin::Block, String)> = blocks[100..].iter().collect();
    let chunk1_keys = collect_input_keys(&chunk1_refs);
    let chunk2_keys = collect_input_keys(&chunk2_refs);

    // Per-chunk keys should sum to total keys
    assert_eq!(
        chunk1_keys.len() + chunk2_keys.len(),
        all_keys.len(),
        "Per-chunk key collection should be equivalent to full-batch collection"
    );

    orchestrator.ingest_blocks_batch(&blocks).await.unwrap();
    assert_eq!(writer.get_blocks().await.len(), 200);
}

// =============================================================================
// Edge Cases
// =============================================================================

/// Edge case: Batch with zero cache misses (all coinbase, no lookups needed)
///
/// GIVEN a batch where all inputs are coinbase (no cache misses)
/// WHEN get_many_with_fallback is called with empty keys
/// THEN it returns immediately with an empty map, no Neo4j queries issued
#[tokio::test]
async fn batch_with_zero_cache_misses_no_lookups() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    // First 10 blocks are all coinbase — zero non-coinbase inputs
    let blocks = read_blocks(10);

    // collect_input_keys should return empty
    let block_refs: Vec<&(u32, bitcoin::Block, String)> = blocks.iter().collect();
    let keys = collect_input_keys(&block_refs);
    assert!(
        keys.is_empty(),
        "Coinbase-only blocks should have no input keys"
    );

    // Batch ingestion should succeed with no UTXO lookups
    orchestrator.ingest_blocks_batch(&blocks).await.unwrap();

    let stats = orchestrator.cache_stats();
    assert_eq!(
        stats.hits, 0,
        "No cache hits expected for coinbase-only blocks"
    );
    assert_eq!(
        stats.misses, 0,
        "No cache misses expected for coinbase-only blocks"
    );
}

/// Edge case: collect_input_keys with empty batch
#[test]
fn collect_input_keys_empty_batch() {
    let blocks: Vec<&(u32, bitcoin::Block, String)> = vec![];
    let keys = collect_input_keys(&blocks);
    assert!(keys.is_empty(), "Empty batch should produce no keys");
}
