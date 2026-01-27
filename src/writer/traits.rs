//! GraphWriter trait - Database abstraction for blockchain ingestion
//!
//! Defines the interface for writing blockchain data to a graph database.
//! This trait abstracts database operations, enabling:
//! - Testing with MockWriter (no database required)
//! - Future database implementations beyond Neo4j
//! - Clean separation between domain logic and database layer
//!
//! The trait supports the 6-phase ingestion process described in
//! [INGESTION_ARCHITECTURE.md](../../docs/INGESTION_ARCHITECTURE.md)

use async_trait::async_trait;
use crate::domain::{BlockData, TransactionData, OutputData, InputData, CheckpointData, PerformsData, BenefitsToData};
use super::error::Result;

/// Graph database writer interface
///
/// Implementors handle persistence of blockchain data to a graph database.
/// All methods are async to support database I/O operations.
///
/// # Implementations
/// - `Neo4jWriter` - Production implementation for Neo4j (Milestone 6)
/// - `MockWriter` - In-memory implementation for testing (Milestone 4)
///
/// # Example Usage
/// ```no_run
/// use bitcoin_chain_graph::writer::{GraphWriter, MockWriter};
/// use bitcoin_chain_graph::domain::BlockData;
///
/// async fn example() {
///     let writer = MockWriter::new();
///
///     // Initialize schema
///     writer.init_schema().await.unwrap();
///
///     // Write blocks
///     let blocks = vec![/* BlockData instances */];
///     writer.write_blocks(&blocks).await.unwrap();
/// }
/// ```
#[async_trait]
pub trait GraphWriter: Send + Sync {
    // Schema Management

    /// Initialize database schema (constraints, indexes)
    ///
    /// Creates all required constraints for unique node properties and indexes
    /// for query performance. Should be idempotent - safe to call multiple times.
    ///
    /// # Errors
    /// Returns error if schema creation fails.
    async fn init_schema(&self) -> Result<()>;

    // Phase 1: Blocks

    /// Write block nodes in bulk
    ///
    /// Creates Block nodes with all properties and NEXT_BLOCK relationships
    /// to previous blocks. This is Phase 1 of the 6-phase ingestion process.
    ///
    /// # Arguments
    /// * `blocks` - Slice of BlockData to persist
    ///
    /// # Errors
    /// Returns error if block writing fails.
    async fn write_blocks(&self, blocks: &[BlockData]) -> Result<()>;

    // Phase 2: Transactions

    /// Write transaction nodes in bulk
    ///
    /// Creates Transaction nodes with properties and INCLUDED_IN relationships
    /// to containing blocks. This is Phase 2 of ingestion.
    ///
    /// In Milestone 7+, TransactionData includes pre-calculated total_input,
    /// total_output, and fee fields (calculated in Rust using UTXO cache).
    ///
    /// # Arguments
    /// * `transactions` - Slice of TransactionData to persist (with amounts)
    ///
    /// # Errors
    /// Returns error if transaction writing fails.
    async fn write_transactions(&self, transactions: &[TransactionData]) -> Result<()>;

    // Phase 3: Outputs

    /// Write output nodes and LOCKED_TO relationships in bulk
    ///
    /// Creates Output nodes with properties, HAS_OUTPUT relationships to
    /// transactions, and LOCKED_TO relationships to addresses (where derivable).
    /// This is Phase 3 of ingestion.
    ///
    /// Outputs with NULL_DATA or UNKNOWN script types have no LOCKED_TO relationship.
    ///
    /// # Arguments
    /// * `outputs` - Slice of OutputData to persist
    ///
    /// # Errors
    /// Returns error if output writing fails.
    async fn write_outputs(&self, outputs: &[OutputData]) -> Result<()>;

    // Phase 4: Inputs

    /// Write input nodes and SPENDS relationships in bulk
    ///
    /// Creates Input nodes with properties, HAS_INPUT relationships to
    /// transactions, and SPENDS relationships to previous outputs.
    /// This is Phase 4 of ingestion.
    ///
    /// Also updates spent outputs with:
    /// - isSpent = true
    /// - spentInTxid = current transaction
    /// - spentAtHeight = current block height
    ///
    /// Coinbase inputs have special handling (no SPENDS relationship created).
    ///
    /// # Arguments
    /// * `inputs` - Slice of InputData to persist
    ///
    /// # Errors
    /// Returns error if input writing fails or if referenced outputs don't exist.
    async fn write_inputs(&self, inputs: &[InputData]) -> Result<()>;

    // Phase 5: (REMOVED in M7) Amounts now calculated in Rust using UTXO cache
    // Amounts are passed to write_transactions() directly

    // Phase 6: Simplified Layer

    /// Write PERFORMS relationships in bulk (Address → Transaction)
    ///
    /// Creates PERFORMS relationships with pre-aggregated properties.
    /// Called during Phase 6 of ingestion.
    ///
    /// Each relationship represents an address performing a transaction
    /// (sending funds via inputs), with aggregated input count and amount.
    ///
    /// # Arguments
    /// * `performs` - Slice of PerformsData with pre-calculated aggregations
    ///
    /// # Errors
    /// Returns error if relationship creation fails.
    async fn write_performs(&self, performs: &[PerformsData]) -> Result<()>;

    /// Write BENEFITS_TO relationships in bulk (Transaction → Address)
    ///
    /// Creates BENEFITS_TO relationships with pre-aggregated properties.
    /// Called during Phase 6 of ingestion.
    ///
    /// Each relationship represents a transaction benefiting an address
    /// (receiving funds via outputs), with aggregated output count and amount.
    ///
    /// # Arguments
    /// * `benefits_to` - Slice of BenefitsToData with pre-calculated aggregations
    ///
    /// # Errors
    /// Returns error if relationship creation fails.
    async fn write_benefits_to(&self, benefits_to: &[BenefitsToData]) -> Result<()>;

    // UTXO Operations

    /// Lookup output by ID (for UTXO cache misses)
    ///
    /// Queries the database for an output node by its ID (txid:index).
    /// Used by the UTXO cache when a cache miss occurs.
    ///
    /// # Arguments
    /// * `output_id` - Output identifier in format "txid:index"
    ///
    /// # Returns
    /// OutputData for the requested output
    ///
    /// # Errors
    /// Returns `WriterError::OutputNotFound` if output doesn't exist.
    async fn lookup_output(&self, output_id: &str) -> Result<OutputData>;

    /// Batch lookup multiple outputs by ID (for UTXO cache misses)
    ///
    /// Queries the database for multiple output nodes in a single UNWIND query,
    /// reducing N round-trips to 1. Used when `get_many()` returns multiple
    /// cache misses that need Neo4j fallback.
    ///
    /// # Arguments
    /// * `output_ids` - Slice of output identifiers in format "txid:index"
    ///
    /// # Returns
    /// Vec of OutputData for found outputs. Outputs not found are silently skipped.
    ///
    /// # Errors
    /// Returns error if query execution fails.
    async fn lookup_outputs_batch(&self, output_ids: &[String]) -> Result<Vec<OutputData>>;

    /// Mark output as spent
    ///
    /// Updates an output node with spent metadata:
    /// - isSpent = true
    /// - spentInTxid = transaction that spent it
    /// - spentAtHeight = block height where spent
    ///
    /// # Arguments
    /// * `output_id` - Output identifier in format "txid:index"
    /// * `spent_in_txid` - Transaction ID that spent this output
    /// * `spent_at_height` - Block height where spent
    ///
    /// # Errors
    /// Returns error if output doesn't exist or update fails.
    async fn mark_output_spent(
        &self,
        output_id: &str,
        spent_in_txid: &str,
        spent_at_height: u32,
    ) -> Result<()>;

    // Checkpoint Management

    /// Create initial checkpoint
    ///
    /// Creates an IngestionCheckpoint node with initial state before
    /// starting ingestion. Should be called once at ingestion start.
    ///
    /// # Errors
    /// Returns error if checkpoint creation fails.
    async fn create_checkpoint(&self) -> Result<()>;

    /// Update checkpoint after successfully processing a block
    ///
    /// Updates the checkpoint with progress information. Should be called
    /// after each block is fully ingested (all 6 phases complete).
    ///
    /// # Arguments
    /// * `checkpoint` - Updated checkpoint data with latest progress
    ///
    /// # Errors
    /// Returns error if checkpoint update fails.
    async fn update_checkpoint(&self, checkpoint: &CheckpointData) -> Result<()>;

    /// Get current checkpoint state
    ///
    /// Retrieves the current checkpoint for resume logic. Returns None if
    /// no checkpoint exists (first run).
    ///
    /// # Returns
    /// Some(CheckpointData) if checkpoint exists, None otherwise
    ///
    /// # Errors
    /// Returns error if query fails.
    async fn get_checkpoint(&self) -> Result<Option<CheckpointData>>;

    /// Mark ingestion as complete
    ///
    /// Updates checkpoint status to "completed" when all blocks have been
    /// successfully ingested.
    ///
    /// # Errors
    /// Returns error if status update fails.
    async fn mark_checkpoint_complete(&self) -> Result<()>;

    /// Set checkpoint status
    ///
    /// Updates the checkpoint status field. Useful for error recovery and
    /// manual status management.
    ///
    /// # Arguments
    /// * `status` - New status: "in_progress", "completed", "paused", "error"
    ///
    /// # Errors
    /// Returns error if status update fails.
    async fn set_checkpoint_status(&self, status: &str) -> Result<()>;
}
