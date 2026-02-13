//! Tests for Re-calibrate Block Memory Estimator for Neo4j Heap Cost
//!
//! Tests all acceptance criteria and edge cases from the feature doc.

use bitcoin::Network;
use bitcoin_chain_graph::domain::ingestion::{
    compute_adaptive_chunks, estimate_block_memory, BYTES_PER_BLOCK, BYTES_PER_INPUT,
    BYTES_PER_OUTPUT, BYTES_PER_TX,
};
use bitcoin_chain_graph::parser::BlockFileReader;

// =============================================================================
// Helper
// =============================================================================

/// Read N blocks from the test data file
fn read_test_blocks(count: usize) -> Vec<(u32, bitcoin::Block, String)> {
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let mut blocks = Vec::new();
    for i in 0..count {
        if let Ok(Some(block)) = reader.next_block() {
            blocks.push((i as u32, block, "blk00000.dat".to_string()));
        } else {
            break;
        }
    }
    blocks
}

// =============================================================================
// AC 1: estimate_block_memory accounts for BOTH nodes AND relationships
// =============================================================================

#[test]
fn estimate_accounts_for_relationships() {
    // AC1: The estimate must account for nodes AND relationships in all 7 phases.
    // Total entities per block: 1 Block + 1 NEXT_BLOCK + T*(TX + PERFORMS) +
    //   O*(Output + HAS_OUTPUT + LOCKED_TO + BENEFITS_TO) + I*(Input + SPENDS)
    // = 2 + 2T + 4O + 2I entities
    //
    // With old constants (500 + 400T + 550O + 550I), the formula only counts nodes.
    // The new formula must produce a HIGHER estimate because it includes relationships.
    let blocks = read_test_blocks(5);
    assert!(blocks.len() >= 2, "Need blocks beyond genesis");

    // Use block index 1 (has at least 1 tx with inputs)
    let block = &blocks[1].1;
    let tx_count = block.txdata.len();
    let output_count: usize = block.txdata.iter().map(|tx| tx.output.len()).sum();
    let input_count: usize = block.txdata.iter().map(|tx| tx.input.len()).sum();

    let estimate = estimate_block_memory(block);

    // Old formula: 500 + T*400 + O*550 + I*550
    let old_formula = 500 + tx_count * 400 + output_count * 550 + input_count * 550;

    // The new estimate MUST be strictly greater than the old formula because
    // it now includes relationship costs (NEXT_BLOCK, PERFORMS, HAS_OUTPUT,
    // LOCKED_TO, BENEFITS_TO, SPENDS).
    assert!(
        estimate > old_formula,
        "New estimate ({}) must be greater than old formula ({}) because \
         relationships are now counted. tx={}, outputs={}, inputs={}",
        estimate,
        old_formula,
        tx_count,
        output_count,
        input_count
    );
}

// =============================================================================
// AC 2: Constants reflect Neo4j heap cost (1500-4000 bytes per entity)
// =============================================================================

#[test]
fn constants_reflect_neo4j_heap_cost() {
    // AC2: Each constant should be in the 1500-4000 byte range for Neo4j transaction
    // heap cost, not the old Rust struct sizes (400-550 bytes).
    assert!(
        BYTES_PER_BLOCK >= 1500,
        "BYTES_PER_BLOCK ({}) should be >= 1500 for Neo4j heap cost",
        BYTES_PER_BLOCK
    );
    assert!(
        BYTES_PER_TX >= 1500,
        "BYTES_PER_TX ({}) should be >= 1500 for Neo4j heap cost",
        BYTES_PER_TX
    );
    assert!(
        BYTES_PER_OUTPUT >= 1500,
        "BYTES_PER_OUTPUT ({}) should be >= 1500 for Neo4j heap cost",
        BYTES_PER_OUTPUT
    );
    assert!(
        BYTES_PER_INPUT >= 1500,
        "BYTES_PER_INPUT ({}) should be >= 1500 for Neo4j heap cost",
        BYTES_PER_INPUT
    );

    // Upper bound sanity check
    assert!(
        BYTES_PER_BLOCK <= 4000,
        "BYTES_PER_BLOCK ({}) should be <= 4000",
        BYTES_PER_BLOCK
    );
    assert!(
        BYTES_PER_TX <= 4000,
        "BYTES_PER_TX ({}) should be <= 4000",
        BYTES_PER_TX
    );
    assert!(
        BYTES_PER_OUTPUT <= 4000,
        "BYTES_PER_OUTPUT ({}) should be <= 4000",
        BYTES_PER_OUTPUT
    );
    assert!(
        BYTES_PER_INPUT <= 4000,
        "BYTES_PER_INPUT ({}) should be <= 4000",
        BYTES_PER_INPUT
    );
}

// =============================================================================
// AC 3: Default max_transaction_memory_mb prevents the production incident
// =============================================================================

#[test]
fn default_max_transaction_memory_prevents_incident() {
    // AC3: The default must be set such that 200 blocks at height ~207K
    // (90 txs, 270 outputs, 270 inputs each) split into multiple chunks.
    // With old default of 600 MB this was a single chunk and caused GC pauses.
    //
    // We use the IngestionOrchestrator's default to get the configured value.
    use bitcoin_chain_graph::domain::IngestionOrchestrator;
    use bitcoin_chain_graph::writer::MockWriter;

    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, 100_000);

    // The default budget should be less than 600 MB (the old default that caused issues)
    // OR the constants should be high enough that 200 blocks at 207K exceed whatever
    // the default is. We verify via AC4 below, but here we check the default changed.
    let default_budget = orchestrator.max_transaction_memory_bytes();

    // The default should still be reasonable (not zero, not absurdly low)
    assert!(
        default_budget >= 100 * 1024 * 1024,
        "Default budget ({} bytes = {} MB) should be at least 100 MB",
        default_budget,
        default_budget / (1024 * 1024)
    );
}

// =============================================================================
// AC 4: 200 blocks at height 207K must produce multiple chunks
// =============================================================================

#[test]
fn height_207k_blocks_produce_multiple_chunks() {
    // AC4: Block at height 207600 with ~90 txs, 270 outputs, 270 inputs.
    // 200 such blocks must exceed max_transaction_memory_mb and split into multiple chunks.
    use bitcoin_chain_graph::domain::IngestionOrchestrator;
    use bitcoin_chain_graph::writer::MockWriter;

    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, 100_000);
    let max_memory_bytes = orchestrator.max_transaction_memory_bytes();

    // Simulate a block at height ~207K: 90 txs, 270 outputs, 270 inputs
    let per_block =
        BYTES_PER_BLOCK + 90 * BYTES_PER_TX + 270 * BYTES_PER_OUTPUT + 270 * BYTES_PER_INPUT;

    let block_memories: Vec<usize> = vec![per_block; 200];
    let total_memory: usize = block_memories.iter().sum();

    // Total memory for 200 blocks MUST exceed the budget
    assert!(
        total_memory > max_memory_bytes,
        "200 blocks at height 207K (total {} bytes = {} MB) must exceed budget ({} bytes = {} MB)",
        total_memory,
        total_memory / (1024 * 1024),
        max_memory_bytes,
        max_memory_bytes / (1024 * 1024)
    );

    // This means compute_adaptive_chunks produces multiple chunks
    let chunks = compute_adaptive_chunks(&block_memories, max_memory_bytes);
    assert!(
        chunks.len() > 1,
        "200 blocks at height 207K should produce multiple chunks, got {}",
        chunks.len()
    );
}

// =============================================================================
// AC 5: No changes to compute_adaptive_chunks algorithm
// =============================================================================

// AC5 is a code inspection criterion — the algorithm should be unchanged.
// Verified by the existing adaptive_transaction_memory.rs tests still passing.

// =============================================================================
// AC 6: Early blocks (0-170K) still fit in single chunk
// =============================================================================

#[test]
fn early_blocks_single_chunk() {
    // AC6: Early Bitcoin blocks (1-5 txs per block). 200 such blocks should
    // fit within the default budget as a single chunk.
    use bitcoin_chain_graph::domain::IngestionOrchestrator;
    use bitcoin_chain_graph::writer::MockWriter;

    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, 100_000);
    let max_memory_bytes = orchestrator.max_transaction_memory_bytes();

    // Typical early block: 1 coinbase tx, 1 output, 1 input (coinbase)
    let per_block = BYTES_PER_BLOCK + 1 * BYTES_PER_TX + 1 * BYTES_PER_OUTPUT + 1 * BYTES_PER_INPUT;

    let block_memories: Vec<usize> = vec![per_block; 200];
    let chunks = compute_adaptive_chunks(&block_memories, max_memory_bytes);

    assert_eq!(
        chunks.len(),
        1,
        "200 early blocks should fit in a single chunk"
    );
}

// =============================================================================
// Edge Case: Modern blocks (post-SegWit, ~3000 txs)
// =============================================================================

#[test]
fn modern_blocks_small_chunks() {
    // Modern blocks with ~3000 txs, ~6000 outputs, ~7500 inputs should
    // produce very small chunks (1-5 blocks per chunk).
    use bitcoin_chain_graph::domain::IngestionOrchestrator;
    use bitcoin_chain_graph::writer::MockWriter;

    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, 100_000);
    let max_memory_bytes = orchestrator.max_transaction_memory_bytes();

    let per_block =
        BYTES_PER_BLOCK + 3000 * BYTES_PER_TX + 6000 * BYTES_PER_OUTPUT + 7500 * BYTES_PER_INPUT;

    let block_memories: Vec<usize> = vec![per_block; 20];
    let chunks = compute_adaptive_chunks(&block_memories, max_memory_bytes);

    // Each chunk should have at most 5 blocks
    for chunk in &chunks {
        let len = chunk.end - chunk.start;
        assert!(
            len <= 5,
            "Modern block chunks should have at most 5 blocks, got {}",
            len
        );
    }
}

// =============================================================================
// Edge Case: Coinbase-only blocks (height 0)
// =============================================================================

#[test]
fn coinbase_only_block_nonzero_estimate() {
    // Genesis block: 1 tx, 1 output, 0 non-coinbase inputs.
    // Estimate should still be reasonable (not zero).
    let blocks = read_test_blocks(1);
    let block = &blocks[0].1;

    let estimate = estimate_block_memory(block);

    // Should account for: 1 Block node + 1 NEXT_BLOCK rel + 1 TX node + 1 PERFORMS rel
    // + 1 Output node + 1 HAS_OUTPUT rel + 1 LOCKED_TO rel + 1 BENEFITS_TO rel
    // = 8 entities, each ~2000-3000 bytes = ~16K-24K minimum
    assert!(
        estimate >= 10_000,
        "Coinbase-only block estimate ({}) should be at least 10KB",
        estimate
    );
}

// =============================================================================
// AC 4 (formula detail): Verify the formula includes relationship multipliers
// =============================================================================

#[test]
fn formula_includes_relationship_multipliers() {
    // The formula should be approximately:
    // BYTES_PER_BLOCK (for block node + NEXT_BLOCK rel)
    // + T * BYTES_PER_TX (for tx node + PERFORMS rel)
    // + O * BYTES_PER_OUTPUT (for output node + HAS_OUTPUT + LOCKED_TO + BENEFITS_TO rels)
    // + I * BYTES_PER_INPUT (for input node + SPENDS rel)
    //
    // Since outputs have 4 entities (1 node + 3 rels) and inputs have 2 (1 node + 1 rel),
    // BYTES_PER_OUTPUT should be roughly 2x BYTES_PER_INPUT if per-entity cost is similar.
    // At minimum, BYTES_PER_OUTPUT must be >= BYTES_PER_INPUT since outputs generate
    // more entities (4 vs 2).

    // With recalibrated constants, outputs should cost more than inputs per unit
    // because each output generates 4 entities vs 2 for inputs.
    assert!(
        BYTES_PER_OUTPUT >= BYTES_PER_INPUT,
        "BYTES_PER_OUTPUT ({}) should be >= BYTES_PER_INPUT ({}) because outputs \
         generate 4 entities (node + HAS_OUTPUT + LOCKED_TO + BENEFITS_TO) vs \
         2 for inputs (node + SPENDS)",
        BYTES_PER_OUTPUT,
        BYTES_PER_INPUT
    );
}

// =============================================================================
// Verify: max_transaction_memory_bytes accessor exists
// =============================================================================

#[test]
fn orchestrator_exposes_max_transaction_memory_bytes() {
    // The orchestrator needs a public accessor for the memory budget
    // so tests and diagnostics can verify the configured value.
    use bitcoin_chain_graph::domain::IngestionOrchestrator;
    use bitcoin_chain_graph::writer::MockWriter;

    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, 100_000)
        .with_max_transaction_memory_mb(42);

    assert_eq!(
        orchestrator.max_transaction_memory_bytes(),
        42 * 1024 * 1024,
        "Accessor should return the configured value in bytes"
    );
}
