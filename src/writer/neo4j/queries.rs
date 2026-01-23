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

/// Create/Update Transaction nodes with INCLUDED_IN relationships
///
/// Uses MERGE on txid (unique identifier) and SET for properties.
/// Idempotent for reprocessing scenarios.
///
/// Parameters:
/// - $transactions: List of transaction objects with properties
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
        t.isCoinbase = tx.isCoinbase
    WITH t, tx
    MATCH (b:Block {height: tx.blockHeight})
    MERGE (t)-[:INCLUDED_IN]->(b)
"#;

// =============================================================================
// PHASE 3: OUTPUT INGESTION
// =============================================================================

/// Create/Update Output nodes with HAS_OUTPUT relationships
///
/// Uses MERGE on outputId (unique identifier) and SET for properties.
/// Preserves isSpent status if already set.
/// Idempotent for reprocessing scenarios.
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
    WITH o, out
    MATCH (t:Transaction {txid: out.txid})
    MERGE (t)-[:HAS_OUTPUT]->(o)
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
// PHASE 5: CALCULATE TRANSACTION AMOUNTS
// =============================================================================

/// Update Transaction nodes with totalInput, totalOutput, and fee
///
/// Parameters:
/// - $txids: List of transaction IDs to calculate amounts for
pub const CALCULATE_AMOUNTS_QUERY: &str = r#"
    UNWIND $txids AS txid
    MATCH (t:Transaction {txid: txid})

    // Calculate total output
    OPTIONAL MATCH (t)-[:HAS_OUTPUT]->(out:Output)
    WITH t, sum(out.amount) AS totalOutput

    // Calculate total input (sum of spent outputs)
    OPTIONAL MATCH (t)-[:HAS_INPUT]->(inp:Input)-[:SPENDS]->(spentOut:Output)
    WITH t, totalOutput, sum(spentOut.amount) AS totalInput

    // Set amounts (fee = totalInput - totalOutput, or 0 for coinbase)
    SET t.totalInput = CASE WHEN totalInput IS NULL THEN 0 ELSE totalInput END,
        t.totalOutput = CASE WHEN totalOutput IS NULL THEN 0 ELSE totalOutput END,
        t.fee = CASE
            WHEN totalInput IS NULL THEN 0
            ELSE totalInput - totalOutput
        END
"#;

// =============================================================================
// PHASE 6: SIMPLIFIED LAYER (PERFORMS & BENEFITS_TO)
// =============================================================================

/// Create/Update PERFORMS relationships (Address -> Transaction)
///
/// Uses MERGE for idempotent relationship creation.
/// Connects addresses to transactions they performed (via inputs)
///
/// Parameters:
/// - $txids: List of transaction IDs to create relationships for
pub const CREATE_PERFORMS_QUERY: &str = r#"
    UNWIND $txids AS txid
    MATCH (t:Transaction {txid: txid})
    MATCH (t)-[:HAS_INPUT]->(inp:Input)-[:SPENDS]->(out:Output)-[:LOCKED_TO]->(addr:Address)
    WITH t, addr, count(DISTINCT inp) AS inputCount, sum(out.amount) AS totalSpent
    MERGE (addr)-[r:PERFORMS]->(t)
    SET r.inputCount = inputCount,
        r.amountSpent = totalSpent
"#;

/// Create/Update BENEFITS_TO relationships (Transaction -> Address)
///
/// Uses MERGE for idempotent relationship creation.
/// Connects transactions to addresses that received funds (via outputs)
///
/// Parameters:
/// - $txids: List of transaction IDs to create relationships for
pub const CREATE_BENEFITS_TO_QUERY: &str = r#"
    UNWIND $txids AS txid
    MATCH (t:Transaction {txid: txid})
    MATCH (t)-[:HAS_OUTPUT]->(out:Output)-[:LOCKED_TO]->(addr:Address)
    WITH t, addr, count(DISTINCT out) AS outputCount, sum(out.amount) AS totalReceived
    MERGE (t)-[r:BENEFITS_TO]->(addr)
    SET r.outputCount = outputCount,
        r.amountReceived = totalReceived
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

/// Create initial ingestion checkpoint
pub const CREATE_CHECKPOINT_QUERY: &str = r#"
    CREATE (c:IngestionCheckpoint {
        lastProcessedHeight: -1,
        lastProcessedHash: '0000000000000000000000000000000000000000000000000000000000000000',
        lastProcessedFile: 'blk00000.dat',
        lastProcessedFileOffset: 0,
        timestamp: datetime(),
        status: 'in_progress'
    })
"#;

/// Update checkpoint after successful block ingestion
///
/// Parameters:
/// - $height: Last successfully processed block height
/// - $hash: Last successfully processed block hash
/// - $file: .blk file name
/// - $offset: File offset
/// - $status: Checkpoint status
pub const UPDATE_CHECKPOINT_QUERY: &str = r#"
    MATCH (c:IngestionCheckpoint)
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
