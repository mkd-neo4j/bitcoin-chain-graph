//! Domain models for Bitcoin blockchain data
//!
//! These models bridge the Parser layer (bitcoin crate types) and the Writer layer
//! (database operations). They use primitive types suitable for serialization and
//! database storage.
//!
//! Key principles:
//! - All models implement Clone, Debug
//! - Use primitive types (String, u32, u64, f64) for database compatibility
//! - No bitcoin crate types in models (conversion boundary)
//! - No neo4rs imports in domain layer

/// Block data extracted from blockchain
///
/// Maps directly to Block node properties in Neo4j.
/// See [DATA_MODEL.md](../../docs/DATA_MODEL.md) for schema.
#[derive(Clone, Debug)]
pub struct BlockData {
    /// Block number (0 = genesis)
    pub height: u32,
    /// Block hash (unique identifier)
    pub hash: String,
    /// Hash of previous block
    pub previous_hash: String,
    /// Merkle root of transactions
    pub merkle_root: String,
    /// Block timestamp (Unix epoch seconds)
    pub timestamp: i64,
    /// Difficulty target (compact format)
    pub bits: String,
    /// Mining difficulty
    pub difficulty: f64,
    /// Mining nonce
    pub nonce: u32,
    /// Block version
    pub version: i32,
    /// Number of transactions
    pub tx_count: u32,
    /// Block size in bytes
    pub size: u64,
    /// Block weight (SegWit)
    pub weight: u64,
}

/// Transaction data extracted from blockchain
///
/// Maps directly to Transaction node properties in Neo4j.
/// Amount fields (total_input, total_output, fee) are calculated in Rust during
/// Phase 5 using the UTXO cache, avoiding expensive Neo4j graph traversals.
#[derive(Clone, Debug)]
pub struct TransactionData {
    /// Transaction hash (unique identifier)
    pub txid: String,
    /// Block number containing this transaction
    pub block_height: u32,
    /// Hash of containing block
    pub block_hash: String,
    /// Block timestamp (Unix epoch seconds)
    pub timestamp: i64,
    /// Transaction version
    pub version: i32,
    /// Locktime value
    pub locktime: u32,
    /// Transaction size in bytes
    pub size: usize,
    /// Virtual size (SegWit)
    pub vsize: usize,
    /// Transaction weight units
    pub weight: usize,
    /// True if this is a coinbase (mining reward) transaction
    pub is_coinbase: bool,
    /// Total input amount in satoshis (calculated in Phase 5)
    pub total_input: Option<u64>,
    /// Total output amount in satoshis (calculated in Phase 5)
    pub total_output: Option<u64>,
    /// Transaction fee in satoshis (calculated in Phase 5)
    pub fee: Option<u64>,
}

/// Output data extracted from transaction
///
/// Maps directly to Output node properties in Neo4j.
/// Represents a "coin" that can be spent once (UTXO when unspent).
#[derive(Clone, Debug)]
pub struct OutputData {
    /// Unique identifier: "{txid}:{index}"
    pub output_id: String,
    /// Position in transaction outputs (0, 1, 2...)
    pub output_index: u32,
    /// Transaction ID this output belongs to
    pub txid: String,
    /// Amount in satoshis (1 BTC = 100,000,000 satoshis)
    pub amount: u64,
    /// Locking script (hex encoded)
    pub script_pubkey: String,
    /// Script type: P2PKH, P2SH, P2WPKH, P2WSH, P2TR, P2PK, NULL_DATA, UNKNOWN
    pub script_type: String,
    /// Bitcoin address (if derivable from script, None for NULL_DATA/UNKNOWN)
    pub address: Option<String>,
}

/// Input data extracted from transaction
///
/// Maps directly to Input node properties in Neo4j.
/// Spends a previous output (references via previous_txid:previous_output_index).
#[derive(Clone, Debug)]
pub struct InputData {
    /// Unique identifier: "{txid}:{index}"
    pub input_id: String,
    /// Position in transaction inputs (0, 1, 2...)
    pub input_index: u32,
    /// Transaction ID this input belongs to
    pub txid: String,
    /// Transaction ID containing the output being spent
    pub previous_txid: String,
    /// Index of the output being spent
    pub previous_output_index: u32,
    /// Unlocking script (hex encoded)
    pub script_sig: String,
    /// Sequence number
    pub sequence: u32,
    /// Witness data array (SegWit transactions, hex encoded items)
    pub witness: Vec<String>,
    /// Block height where this input appears (used for spentAtHeight on spent outputs)
    pub block_height: u32,
}

/// Checkpoint data for resumable ingestion
///
/// Maps directly to IngestionCheckpoint node properties in Neo4j.
/// Tracks progress to enable resume-on-failure.
#[derive(Clone, Debug)]
pub struct CheckpointData {
    /// Last successfully ingested block height (-1 for initial state)
    pub last_processed_height: i32,
    /// Hash of last processed block (for verification)
    pub last_processed_hash: String,
    /// Name of .blk file being processed (e.g., "blk00000.dat")
    pub last_processed_file: String,
    /// Byte offset within the file (optional optimization)
    pub last_processed_file_offset: Option<u64>,
    /// When checkpoint was last updated (Unix epoch seconds)
    pub timestamp: i64,
    /// Current status: "in_progress", "completed", "paused", "error"
    pub status: String,
}

/// PERFORMS relationship data (Address → Transaction)
///
/// Represents aggregated information about addresses performing transactions
/// (sending funds via inputs). Calculated in Rust during Phase 6 using UTXO cache,
/// avoiding expensive Neo4j graph traversals.
///
/// Maps to PERFORMS relationships with properties in Neo4j.
#[derive(Clone, Debug)]
pub struct PerformsData {
    /// Address performing the transaction (sending funds)
    pub from_address: String,
    /// Transaction ID being performed
    pub to_txid: String,
    /// Number of inputs from this address in this transaction
    pub input_count: u32,
    /// Total amount spent by this address in this transaction (satoshis)
    pub amount_spent: u64,
}

/// BENEFITS_TO relationship data (Transaction → Address)
///
/// Represents aggregated information about addresses receiving transaction outputs
/// (receiving funds via outputs). Calculated in Rust during Phase 6 using UTXO cache,
/// avoiding expensive Neo4j graph traversals.
///
/// Maps to BENEFITS_TO relationships with properties in Neo4j.
#[derive(Clone, Debug)]
pub struct BenefitsToData {
    /// Transaction ID providing the benefit
    pub from_txid: String,
    /// Address receiving the benefit
    pub to_address: String,
    /// Number of outputs to this address in this transaction
    pub output_count: u32,
    /// Total amount received by this address in this transaction (satoshis)
    pub amount_received: u64,
}
