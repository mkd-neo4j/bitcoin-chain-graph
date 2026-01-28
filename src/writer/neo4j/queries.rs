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
/// Preserves isSpent status if already set.
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
        o.scriptType = out.scriptType,
        o.isSpent = false,
        o.spentInTxid = null,
        o.spentAtHeight = null
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
    MATCH (o:Output {outputId: inp.previousTxid + ':' + toString(inp.previousOutputIndex)})
    MERGE (i)-[:SPENDS]->(o)
    SET o.isSpent = true,
        o.spentInTxid = inp.txid,
        o.spentAtHeight = inp.blockHeight
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

/// Lookup output by ID (for UTXO cache misses)
///
/// Parameters:
/// - $outputId: Output identifier in format "txid:index"
pub const LOOKUP_OUTPUT_QUERY: &str = r#"
    MATCH (o:Output {outputId: $outputId})
    OPTIONAL MATCH (o)-[:LOCKED_TO]->(a:Address)
    RETURN o.outputId AS outputId,
           o.outputIndex AS outputIndex,
           o.amount AS amount,
           o.scriptPubKey AS scriptPubKey,
           o.scriptType AS scriptType,
           a.address AS address
"#;

/// Batch lookup multiple outputs by ID (for UTXO cache batch misses)
///
/// Uses UNWIND to look up N outputs in a single query instead of N round-trips.
/// Outputs that don't exist are silently skipped (MATCH filters them out).
///
/// Parameters:
/// - $outputIds: List of output identifiers in format "txid:index"
pub const LOOKUP_OUTPUTS_BATCH_QUERY: &str = r#"
    UNWIND $outputIds AS oid
    MATCH (o:Output {outputId: oid})
    OPTIONAL MATCH (o)-[:LOCKED_TO]->(a:Address)
    RETURN o.outputId AS outputId,
           o.outputIndex AS outputIndex,
           o.amount AS amount,
           o.scriptPubKey AS scriptPubKey,
           o.scriptType AS scriptType,
           a.address AS address
"#;

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
