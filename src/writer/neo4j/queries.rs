//! Centralized Cypher queries for Neo4j Writer
//!
//! ALL Cypher queries for blockchain ingestion are defined here as constants.
//! This centralization enables easy review, optimization, and maintenance.
//!
//! Query Design Principles:
//! - Use MERGE on unique IDs for idempotent operations (reprocessing-safe)
//! - Use UNWIND for bulk operations
//! - Parameterized (no string interpolation)
//! - Include error handling (WHERE clauses)
//! - Optimize for batch writes

// =============================================================================
// PHASE 1: BLOCK INGESTION
// =============================================================================

/// Create/Update Block nodes with NEXT_BLOCK relationships
///
/// Uses MERGE on hash (unique identifier) and SET for properties.
/// Idempotent for reprocessing scenarios.
///
/// Parameters:
/// - $blocks: List of block objects with properties
pub const CREATE_BLOCKS_QUERY: &str = r#"
    UNWIND $blocks AS block
    MERGE (b:Block {hash: block.hash})
    SET b.height = block.height,
        b.previousHash = block.previousHash,
        b.merkleRoot = block.merkleRoot,
        b.timestamp = datetime({epochSeconds: block.timestamp}),
        b.bits = block.bits,
        b.difficulty = block.difficulty,
        b.nonce = block.nonce,
        b.version = block.version,
        b.txCount = block.txCount,
        b.size = block.size,
        b.weight = block.weight
    WITH b, block
    WHERE block.height > 0
    OPTIONAL MATCH (prev:Block {height: block.height - 1})
    FOREACH (ignoreMe IN CASE WHEN prev IS NOT NULL THEN [1] ELSE [] END |
        MERGE (prev)-[:NEXT_BLOCK]->(b)
    )
"#;

// =============================================================================
// PHASE 2: TRANSACTION INGESTION
// =============================================================================

/// Create/Update Transaction nodes with INCLUDED_IN relationships (M7 - with amounts)
///
/// Uses MERGE on txid (unique identifier) and SET for properties.
/// Idempotent for reprocessing scenarios.
///
/// **M7 Update**: Now includes totalInput, totalOutput, and fee fields that are
/// calculated in Rust using the UTXO cache, avoiding expensive graph traversals.
///
/// Parameters:
/// - $transactions: List of transaction objects with ALL properties including amounts
pub const CREATE_TRANSACTIONS_QUERY: &str = r#"
    UNWIND $transactions AS tx
    MERGE (t:Transaction {txid: tx.txid})
    SET t.blockHeight = tx.blockHeight,
        t.blockHash = tx.blockHash,
        t.timestamp = datetime({epochSeconds: tx.timestamp}),
        t.version = tx.version,
        t.locktime = tx.locktime,
        t.size = tx.size,
        t.vsize = tx.vsize,
        t.weight = tx.weight,
        t.isCoinbase = tx.isCoinbase,
        t.totalInput = tx.totalInput,
        t.totalOutput = tx.totalOutput,
        t.fee = tx.fee
    WITH t, tx
    MATCH (b:Block {height: tx.blockHeight})
    MERGE (t)-[:INCLUDED_IN]->(b)
"#;

// =============================================================================
// PHASE 3: OUTPUT INGESTION
// =============================================================================

/// Create/Update Output nodes
///
/// Uses MERGE on outputId (unique identifier) and SET for properties.
/// Idempotent for reprocessing scenarios.
///
/// NOTE: HAS_OUTPUT relationships are created separately by CREATE_HAS_OUTPUT_QUERY
/// after Transaction nodes exist (Phase 3.5). Outputs are created in Phase 2
/// (before Transactions in Phase 3) to support same-block UTXO references.
///
/// Parameters:
/// - $outputs: List of output objects with properties
pub const CREATE_OUTPUTS_QUERY: &str = r#"
    UNWIND $outputs AS out
    MERGE (o:Output {outputId: out.outputId})
    ON CREATE SET
        o.outputIndex = out.outputIndex,
        o.amount = out.amount,
        o.scriptPubKey = out.scriptPubKey,
        o.scriptType = out.scriptType
    ON MATCH SET
        o.outputIndex = out.outputIndex,
        o.amount = out.amount,
        o.scriptPubKey = out.scriptPubKey,
        o.scriptType = out.scriptType
"#;

/// Create LOCKED_TO relationships for outputs with addresses
///
/// Uses MERGE for both Address nodes and relationships.
/// Idempotent for reprocessing scenarios.
///
/// Parameters:
/// - $outputs: List of output objects with address field
pub const CREATE_LOCKED_TO_QUERY: &str = r#"
    UNWIND $outputs AS out
    MATCH (o:Output {outputId: out.outputId})
    MERGE (a:Address {address: out.address})
    MERGE (o)-[:LOCKED_TO]->(a)
"#;

/// Create HAS_OUTPUT relationships (Transaction -> Output)
///
/// Runs in Phase 3.5, AFTER both Output nodes (Phase 2) and Transaction nodes
/// (Phase 3) exist. Separated from CREATE_OUTPUTS_QUERY because outputs must be
/// created before transactions to support same-block UTXO references.
///
/// Parameters:
/// - $outputs: List of output objects with txid and outputId fields
pub const CREATE_HAS_OUTPUT_QUERY: &str = r#"
    UNWIND $outputs AS out
    MATCH (t:Transaction {txid: out.txid})
    MATCH (o:Output {outputId: out.outputId})
    MERGE (t)-[:HAS_OUTPUT]->(o)
"#;

// =============================================================================
// PHASE 4: INPUT INGESTION
// =============================================================================

/// Create/Update Input nodes with HAS_INPUT and SPENDS relationships
///
/// Uses MERGE on inputId (unique identifier) and SET for properties.
/// Idempotent for reprocessing scenarios.
///
/// Parameters:
/// - $inputs: List of input objects with properties
///
/// Note: Coinbase inputs (previousOutputIndex = 0xFFFFFFFF) skip SPENDS creation
pub const CREATE_INPUTS_QUERY: &str = r#"
    UNWIND $inputs AS inp
    MERGE (i:Input {inputId: inp.inputId})
    SET i.inputIndex = inp.inputIndex,
        i.scriptSig = inp.scriptSig,
        i.sequence = inp.sequence,
        i.witness = inp.witness
    WITH i, inp
    MATCH (t:Transaction {txid: inp.txid})
    MERGE (t)-[:HAS_INPUT]->(i)
    WITH i, inp
    WHERE inp.previousOutputIndex <> 4294967295
    MATCH (o:Output {outputId: inp.previousOutputId})
    MERGE (i)-[:SPENDS]->(o)
"#;

// =============================================================================
// PHASE 5: (REMOVED IN M7) - Amounts now calculated in Rust using UTXO cache
// =============================================================================
// The CALCULATE_AMOUNTS_QUERY has been removed. Transaction amounts (totalInput,
// totalOutput, fee) are now calculated in Rust during Phase 2 using the UTXO cache,
// avoiding expensive Neo4j graph traversals. This provides 10-100x performance improvement.

// =============================================================================
// PHASE 6: SIMPLIFIED LAYER (M7 - Bulk creation with pre-aggregated data)
// =============================================================================

/// Create PERFORMS relationships in bulk with pre-aggregated data (M7)
///
/// **M7 Change**: Replaces graph traversal-based query with bulk creation using
/// pre-aggregated data from Rust. This avoids expensive 3-4 hop traversals.
///
/// Parameters:
/// - $performs: List of {fromAddress, toTxid, inputCount, amountSpent}
pub const CREATE_PERFORMS_BULK_QUERY: &str = r#"
    UNWIND $performs AS p
    MERGE (addr:Address {address: p.fromAddress})
    WITH addr, p
    MATCH (t:Transaction {txid: p.toTxid})
    MERGE (addr)-[r:PERFORMS]->(t)
    SET r.inputCount = p.inputCount,
        r.amountSpent = p.amountSpent
"#;

/// Create BENEFITS_TO relationships in bulk with pre-aggregated data (M7)
///
/// **M7 Change**: Replaces graph traversal-based query with bulk creation using
/// pre-aggregated data from Rust. This avoids expensive 3-4 hop traversals.
///
/// Parameters:
/// - $benefitsTo: List of {fromTxid, toAddress, outputCount, amountReceived}
pub const CREATE_BENEFITS_TO_BULK_QUERY: &str = r#"
    UNWIND $benefitsTo AS b
    MATCH (t:Transaction {txid: b.fromTxid})
    WITH t, b
    MERGE (addr:Address {address: b.toAddress})
    MERGE (t)-[r:BENEFITS_TO]->(addr)
    SET r.outputCount = b.outputCount,
        r.amountReceived = b.amountReceived
"#;

// =============================================================================
// UTXO OPERATIONS
// =============================================================================

/// Mark output as spent
///
/// Parameters:
/// - $outputId: Output identifier
/// - $spentInTxid: Transaction that spent this output
/// - $spentAtHeight: Block height where spent
pub const MARK_OUTPUT_SPENT_QUERY: &str = r#"
    MATCH (o:Output {outputId: $outputId})
    SET o.isSpent = true,
        o.spentInTxid = $spentInTxid,
        o.spentAtHeight = $spentAtHeight
"#;

// =============================================================================
// UTXO CACHE FALLBACK
// =============================================================================

/// Look up Output nodes by their IDs for UTXO cache fallback.
///
/// Used when the UTXO cache has misses and needs to resolve outputs
/// from Neo4j. Returns output properties needed to reconstruct
/// CachedOutput entries.
///
/// Parameters:
/// - $outputIds: List of output identifier strings ("txid:index")
pub const LOOKUP_OUTPUTS_BATCH_QUERY: &str = r#"
    UNWIND $outputIds AS oid
    MATCH (o:Output {outputId: oid})
    OPTIONAL MATCH (o)-[:LOCKED_TO]->(a:Address)
    RETURN o.outputId AS outputId,
           o.outputIndex AS outputIndex,
           o.amount AS amount,
           o.scriptType AS scriptType,
           a.address AS address
"#;

// =============================================================================
// CHECKPOINT MANAGEMENT
// =============================================================================

/// Delete all existing checkpoints (used before creating a fresh one)
pub const DELETE_CHECKPOINT_QUERY: &str = r#"
    MATCH (c:IngestionCheckpoint) DELETE c
"#;

/// Create initial ingestion checkpoint
///
/// Uses sentinel height -999 to represent "not yet started" state.
/// This avoids a neo4rs driver bug that misreads -1 as 255.
pub const CREATE_CHECKPOINT_QUERY: &str = r#"
    CREATE (c:IngestionCheckpoint {
        lastProcessedHeight: -999,
        lastProcessedHash: '0000000000000000000000000000000000000000000000000000000000000000',
        lastProcessedFile: 'blk00000.dat',
        lastProcessedFileOffset: 0,
        timestamp: datetime(),
        status: 'in_progress'
    })
"#;

/// Set checkpoint status
///
/// Parameters:
/// - $status: New status ("in_progress", "completed", "paused", "error")
pub const SET_CHECKPOINT_STATUS_QUERY: &str = r#"
    MATCH (c:IngestionCheckpoint)
    SET c.status = $status,
        c.timestamp = datetime()
"#;

/// Update checkpoint after successful block ingestion
///
/// Uses MERGE to guarantee the checkpoint node exists and is updated.
/// If the node was deleted or never created, MERGE will create it and
/// then SET applies the properties — preventing silent no-ops.
///
/// Parameters:
/// - $height: Last successfully processed block height
/// - $hash: Last successfully processed block hash
/// - $file: .blk file name or source identifier (e.g., "rpc")
/// - $offset: File offset
/// - $status: Checkpoint status
pub const UPDATE_CHECKPOINT_QUERY: &str = r#"
    MERGE (c:IngestionCheckpoint)
    SET c.lastProcessedHeight = $height,
        c.lastProcessedHash = $hash,
        c.lastProcessedFile = $file,
        c.lastProcessedFileOffset = $offset,
        c.timestamp = datetime(),
        c.status = $status
"#;

/// Get current checkpoint state
pub const GET_CHECKPOINT_QUERY: &str = r#"
    MATCH (c:IngestionCheckpoint)
    RETURN c.lastProcessedHeight AS lastProcessedHeight,
           c.lastProcessedHash AS lastProcessedHash,
           c.lastProcessedFile AS lastProcessedFile,
           c.lastProcessedFileOffset AS lastProcessedFileOffset,
           c.timestamp AS timestamp,
           c.status AS status
"#;

/// Mark ingestion as complete
pub const MARK_CHECKPOINT_COMPLETE_QUERY: &str = r#"
    MATCH (c:IngestionCheckpoint)
    SET c.status = 'completed',
        c.timestamp = datetime()
"#;

// =============================================================================
// BLOCK LOOKUP (for reorg detection)
// =============================================================================

/// Look up a block hash by height
///
/// Used for parent hash validation before ingesting a new block.
/// Returns the hash of the block at the given height, or no rows if
/// no block exists at that height.
///
/// Parameters:
/// - $height: Block height to look up
pub const LOOKUP_BLOCK_HASH_QUERY: &str = r#"
    MATCH (b:Block {height: $height})
    RETURN b.hash AS hash
"#;

// =============================================================================
// ROLLBACK (for chain reorganization handling)
// =============================================================================
//
// Rollback deletes all data associated with a block at a given height.
// Must be called in order: delete inputs → delete outputs →
// delete transactions → delete block.
//
// IMPORTANT: Only call on the current chain tip. Rolling back from the middle
// would leave orphaned data from later blocks.

/// Step 1: Delete all Input nodes created by this block's transactions
///
/// DETACH DELETE removes the Input node and all its relationships
/// (HAS_INPUT from Transaction, SPENDS to Output).
///
/// Parameters:
/// - $height: Block height to roll back
pub const ROLLBACK_DELETE_INPUTS_QUERY: &str = r#"
    MATCH (:Block {height: $height})<-[:INCLUDED_IN]-(:Transaction)-[:HAS_INPUT]->(i:Input)
    DETACH DELETE i
"#;

/// Step 2: Delete all Output nodes created by this block's transactions
///
/// DETACH DELETE removes the Output node and all its relationships
/// (HAS_OUTPUT from Transaction, LOCKED_TO to Address).
/// Address nodes are intentionally preserved (they may be referenced
/// by outputs in other blocks).
///
/// Parameters:
/// - $height: Block height to roll back
pub const ROLLBACK_DELETE_OUTPUTS_QUERY: &str = r#"
    MATCH (:Block {height: $height})<-[:INCLUDED_IN]-(:Transaction)-[:HAS_OUTPUT]->(o:Output)
    DETACH DELETE o
"#;

/// Step 3: Delete all Transaction nodes in this block
///
/// DETACH DELETE removes Transaction nodes and all remaining relationships
/// (INCLUDED_IN to Block, PERFORMS from Address, BENEFITS_TO to Address).
///
/// Parameters:
/// - $height: Block height to roll back
pub const ROLLBACK_DELETE_TRANSACTIONS_QUERY: &str = r#"
    MATCH (:Block {height: $height})<-[:INCLUDED_IN]-(t:Transaction)
    DETACH DELETE t
"#;

/// Step 4: Delete the Block node itself
///
/// DETACH DELETE removes the Block node and its NEXT_BLOCK relationships
/// (both incoming from the previous block and outgoing to the next block).
///
/// Parameters:
/// - $height: Block height to roll back
pub const ROLLBACK_DELETE_BLOCK_QUERY: &str = r#"
    MATCH (b:Block {height: $height})
    DETACH DELETE b
"#;

// =============================================================================
// FAST CREATE QUERIES (for forward ingestion - no existence check)
// =============================================================================
//
// These queries use CREATE instead of MERGE for better performance during
// forward ingestion when we know nodes don't exist yet. They will fail with
// a constraint violation if duplicates exist, which is safer than MERGE
// silently updating.
//
// Use these only during normal catchup/ingestion. Use the MERGE-based queries
// above for reprocessing or recovery scenarios.

/// Fast CREATE for Block nodes (no existence check)
///
/// Use only during forward ingestion when blocks are guaranteed new.
/// Will fail with constraint violation if block already exists.
///
/// Parameters:
/// - $blocks: List of block objects with properties
pub const CREATE_BLOCKS_FAST_QUERY: &str = r#"
    UNWIND $blocks AS block
    CREATE (b:Block {hash: block.hash})
    SET b.height = block.height,
        b.previousHash = block.previousHash,
        b.merkleRoot = block.merkleRoot,
        b.timestamp = datetime({epochSeconds: block.timestamp}),
        b.bits = block.bits,
        b.difficulty = block.difficulty,
        b.nonce = block.nonce,
        b.version = block.version,
        b.txCount = block.txCount,
        b.size = block.size,
        b.weight = block.weight
    WITH b, block
    WHERE block.height > 0
    MATCH (prev:Block {height: block.height - 1})
    CREATE (prev)-[:NEXT_BLOCK]->(b)
"#;

/// Fast CREATE for Transaction nodes (no existence check)
///
/// Use only during forward ingestion when transactions are guaranteed new.
/// Will fail with constraint violation if transaction already exists.
///
/// Parameters:
/// - $transactions: List of transaction objects with ALL properties including amounts
pub const CREATE_TRANSACTIONS_FAST_QUERY: &str = r#"
    UNWIND $transactions AS tx
    CREATE (t:Transaction {txid: tx.txid})
    SET t.blockHeight = tx.blockHeight,
        t.blockHash = tx.blockHash,
        t.timestamp = datetime({epochSeconds: tx.timestamp}),
        t.version = tx.version,
        t.locktime = tx.locktime,
        t.size = tx.size,
        t.vsize = tx.vsize,
        t.weight = tx.weight,
        t.isCoinbase = tx.isCoinbase,
        t.totalInput = tx.totalInput,
        t.totalOutput = tx.totalOutput,
        t.fee = tx.fee
    WITH t, tx
    MATCH (b:Block {height: tx.blockHeight})
    CREATE (t)-[:INCLUDED_IN]->(b)
"#;

/// Fast CREATE for Output nodes WITH LOCKED_TO relationships in single query
///
/// Combines output node creation with LOCKED_TO relationship creation to save
/// one network round-trip. Uses CREATE for outputs (forward ingestion) and
/// MERGE for Address nodes (may already exist from previous outputs).
///
/// Performance optimization: Instead of two separate queries:
/// 1. CREATE outputs
/// 2. MATCH outputs, MERGE addresses, CREATE LOCKED_TO
///
/// This query does both in one pass, avoiding the MATCH lookup overhead.
///
/// Parameters:
/// - $outputs: List of output objects with properties (including optional address)
pub const CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY: &str = r#"
    UNWIND $outputs AS out
    CREATE (o:Output {
        outputId: out.outputId,
        outputIndex: out.outputIndex,
        amount: out.amount,
        scriptPubKey: out.scriptPubKey,
        scriptType: out.scriptType
    })
    WITH o, out
    WHERE out.address IS NOT NULL
    MERGE (a:Address {address: out.address})
    CREATE (o)-[:LOCKED_TO]->(a)
"#;

/// Fast CREATE for Input nodes (no existence check)
///
/// Use only during forward ingestion when inputs are guaranteed new.
/// Will fail with constraint violation if input already exists.
///
/// Parameters:
/// - $inputs: List of input objects with properties
pub const CREATE_INPUTS_FAST_QUERY: &str = r#"
    UNWIND $inputs AS inp
    CREATE (i:Input {inputId: inp.inputId})
    SET i.inputIndex = inp.inputIndex,
        i.scriptSig = inp.scriptSig,
        i.sequence = inp.sequence,
        i.witness = inp.witness
    WITH i, inp
    MATCH (t:Transaction {txid: inp.txid})
    CREATE (t)-[:HAS_INPUT]->(i)
    WITH i, inp
    WHERE inp.previousOutputIndex <> 4294967295
    MATCH (o:Output {outputId: inp.previousOutputId})
    CREATE (i)-[:SPENDS]->(o)
"#;

/// Fast CREATE for HAS_OUTPUT relationships (no existence check)
///
/// Use only during forward ingestion when relationships are guaranteed new.
///
/// Parameters:
/// - $outputs: List of output objects with txid and outputId fields
pub const CREATE_HAS_OUTPUT_FAST_QUERY: &str = r#"
    UNWIND $outputs AS out
    MATCH (t:Transaction {txid: out.txid})
    MATCH (o:Output {outputId: out.outputId})
    CREATE (t)-[:HAS_OUTPUT]->(o)
"#;

// =============================================================================
// CRASH RECOVERY QUERIES
// =============================================================================

/// Get the maximum block height in the database
///
/// Used for crash recovery to detect blocks written but not checkpointed.
/// Returns null if no blocks exist.
pub const GET_MAX_BLOCK_HEIGHT_QUERY: &str = r#"
    MATCH (b:Block)
    RETURN max(b.height) AS maxHeight
"#;

/// Check if a block is complete (has expected transaction count)
///
/// Compares the block's stored txCount with the actual number of Transaction
/// nodes linked via INCLUDED_IN. A mismatch indicates the block was partially
/// written (e.g., crash after Phase 1 but before Phase 3).
///
/// Parameters:
/// - $height: Block height to check
///
/// Returns:
/// - expectedTxCount: The block's txCount property
/// - actualTxCount: Count of Transaction nodes linked to this block
pub const CHECK_BLOCK_COMPLETE_QUERY: &str = r#"
    MATCH (b:Block {height: $height})
    OPTIONAL MATCH (t:Transaction)-[:INCLUDED_IN]->(b)
    RETURN b.txCount AS expectedTxCount, count(t) AS actualTxCount
"#;

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // AC 1: CREATE_INPUTS_FAST_QUERY — single UNWIND, no SET on Output,
    //        uses inp.previousOutputId instead of Cypher string concatenation
    // =========================================================================

    #[test]
    fn ac1_fast_input_query_uses_previous_output_id_param() {
        // After the change, the query should reference inp.previousOutputId
        // (pre-computed in Rust) instead of Cypher concatenation
        assert!(
            CREATE_INPUTS_FAST_QUERY.contains("inp.previousOutputId"),
            "CREATE_INPUTS_FAST_QUERY should use inp.previousOutputId instead of Cypher concatenation"
        );
    }

    #[test]
    fn ac1_fast_input_query_no_cypher_concatenation() {
        // The query should NOT contain string concatenation for outputId
        assert!(
            !CREATE_INPUTS_FAST_QUERY
                .contains("inp.previousTxid + ':' + toString(inp.previousOutputIndex)"),
            "CREATE_INPUTS_FAST_QUERY should not use Cypher string concatenation for outputId"
        );
    }

    #[test]
    fn ac1_fast_input_query_no_set_on_output() {
        // The query should NOT SET any properties on Output nodes (o.isSpent, etc.)
        assert!(
            !CREATE_INPUTS_FAST_QUERY.contains("o.isSpent"),
            "CREATE_INPUTS_FAST_QUERY should not SET isSpent on Output nodes"
        );
        assert!(
            !CREATE_INPUTS_FAST_QUERY.contains("o.spentInTxid"),
            "CREATE_INPUTS_FAST_QUERY should not SET spentInTxid on Output nodes"
        );
        assert!(
            !CREATE_INPUTS_FAST_QUERY.contains("o.spentAtHeight"),
            "CREATE_INPUTS_FAST_QUERY should not SET spentAtHeight on Output nodes"
        );
    }

    #[test]
    fn ac1_fast_input_query_single_unwind() {
        // The query should have exactly one UNWIND (single query, not split)
        let unwind_count = CREATE_INPUTS_FAST_QUERY.matches("UNWIND").count();
        assert_eq!(
            unwind_count, 1,
            "CREATE_INPUTS_FAST_QUERY should have exactly one UNWIND (single query)"
        );
    }

    #[test]
    fn ac1_fast_input_query_creates_input_has_input_and_spends() {
        // The query should still create Input nodes, HAS_INPUT, and SPENDS
        assert!(
            CREATE_INPUTS_FAST_QUERY.contains("CREATE (i:Input"),
            "CREATE_INPUTS_FAST_QUERY should create Input nodes"
        );
        assert!(
            CREATE_INPUTS_FAST_QUERY.contains("HAS_INPUT"),
            "CREATE_INPUTS_FAST_QUERY should create HAS_INPUT relationships"
        );
        assert!(
            CREATE_INPUTS_FAST_QUERY.contains("SPENDS"),
            "CREATE_INPUTS_FAST_QUERY should create SPENDS relationships"
        );
    }

    // =========================================================================
    // AC 2: CREATE_INPUTS_QUERY (MERGE recovery variant) — same pattern as fast
    // =========================================================================

    #[test]
    fn ac2_merge_input_query_uses_previous_output_id_param() {
        assert!(
            CREATE_INPUTS_QUERY.contains("inp.previousOutputId"),
            "CREATE_INPUTS_QUERY should use inp.previousOutputId instead of Cypher concatenation"
        );
    }

    #[test]
    fn ac2_merge_input_query_no_cypher_concatenation() {
        assert!(
            !CREATE_INPUTS_QUERY
                .contains("inp.previousTxid + ':' + toString(inp.previousOutputIndex)"),
            "CREATE_INPUTS_QUERY should not use Cypher string concatenation for outputId"
        );
    }

    #[test]
    fn ac2_merge_input_query_no_set_on_output() {
        assert!(
            !CREATE_INPUTS_QUERY.contains("o.isSpent"),
            "CREATE_INPUTS_QUERY should not SET isSpent on Output nodes"
        );
        assert!(
            !CREATE_INPUTS_QUERY.contains("o.spentInTxid"),
            "CREATE_INPUTS_QUERY should not SET spentInTxid on Output nodes"
        );
        assert!(
            !CREATE_INPUTS_QUERY.contains("o.spentAtHeight"),
            "CREATE_INPUTS_QUERY should not SET spentAtHeight on Output nodes"
        );
    }

    #[test]
    fn ac2_merge_input_query_single_unwind() {
        let unwind_count = CREATE_INPUTS_QUERY.matches("UNWIND").count();
        assert_eq!(
            unwind_count, 1,
            "CREATE_INPUTS_QUERY should have exactly one UNWIND (single query)"
        );
    }

    // =========================================================================
    // AC 3: CREATE_OUTPUTS_QUERY — ON CREATE SET does NOT include isSpent fields
    // =========================================================================

    #[test]
    fn ac3_outputs_query_no_is_spent_in_on_create_set() {
        assert!(
            !CREATE_OUTPUTS_QUERY.contains("isSpent"),
            "CREATE_OUTPUTS_QUERY should not include isSpent in ON CREATE SET"
        );
    }

    #[test]
    fn ac3_outputs_query_no_spent_in_txid_in_on_create_set() {
        assert!(
            !CREATE_OUTPUTS_QUERY.contains("spentInTxid"),
            "CREATE_OUTPUTS_QUERY should not include spentInTxid in ON CREATE SET"
        );
    }

    #[test]
    fn ac3_outputs_query_no_spent_at_height_in_on_create_set() {
        assert!(
            !CREATE_OUTPUTS_QUERY.contains("spentAtHeight"),
            "CREATE_OUTPUTS_QUERY should not include spentAtHeight in ON CREATE SET"
        );
    }

    // =========================================================================
    // AC 4: CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY — no isSpent fields
    // =========================================================================

    #[test]
    fn ac4_fast_outputs_query_no_is_spent() {
        assert!(
            !CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY.contains("isSpent"),
            "CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY should not include isSpent"
        );
    }

    #[test]
    fn ac4_fast_outputs_query_no_spent_in_txid() {
        assert!(
            !CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY.contains("spentInTxid"),
            "CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY should not include spentInTxid"
        );
    }

    #[test]
    fn ac4_fast_outputs_query_no_spent_at_height() {
        assert!(
            !CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY.contains("spentAtHeight"),
            "CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY should not include spentAtHeight"
        );
    }

    // =========================================================================
    // AC 9: Coinbase filtering — WHERE clause filters coinbase inputs
    // =========================================================================

    #[test]
    fn ac9_fast_input_query_filters_coinbase_via_where_clause() {
        // The query should have a WHERE clause that filters out coinbase inputs
        // (previousOutputIndex = 4294967295 = 0xFFFFFFFF) before creating SPENDS
        assert!(
            CREATE_INPUTS_FAST_QUERY.contains("4294967295"),
            "CREATE_INPUTS_FAST_QUERY should filter coinbase inputs by previousOutputIndex 4294967295"
        );
        assert!(
            CREATE_INPUTS_FAST_QUERY.contains("WHERE"),
            "CREATE_INPUTS_FAST_QUERY should have a WHERE clause for coinbase filtering"
        );
    }

    #[test]
    fn ac9_merge_input_query_filters_coinbase_via_where_clause() {
        assert!(
            CREATE_INPUTS_QUERY.contains("4294967295"),
            "CREATE_INPUTS_QUERY should filter coinbase inputs by previousOutputIndex 4294967295"
        );
        assert!(
            CREATE_INPUTS_QUERY.contains("WHERE"),
            "CREATE_INPUTS_QUERY should have a WHERE clause for coinbase filtering"
        );
    }
}
