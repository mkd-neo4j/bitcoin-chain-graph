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

use super::error::Result;
use crate::domain::{
    BenefitsToData, BlockData, CheckpointData, InputData, OutputData, PerformsData, TransactionData,
};
use async_trait::async_trait;

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

    // Phase 3.5: HAS_OUTPUT Relationships

    /// Write HAS_OUTPUT relationships in bulk (Transaction -> Output)
    ///
    /// Creates HAS_OUTPUT relationships linking Transaction nodes to their
    /// Output nodes. Called in Phase 3.5 AFTER both outputs (Phase 2) and
    /// transactions (Phase 3) have been written.
    ///
    /// Separated from `write_outputs()` because outputs are created before
    /// transactions to support same-block UTXO references.
    ///
    /// # Arguments
    /// * `outputs` - Slice of OutputData containing txid and outputId
    ///
    /// # Errors
    /// Returns error if relationship creation fails.
    async fn write_has_output_relationships(&self, outputs: &[OutputData]) -> Result<()>;

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

    // Block Lookup (for reorg detection)

    /// Look up a block's hash by height
    ///
    /// Queries the database for the hash of the block at the given height.
    /// Used for parent hash validation before ingesting a new block to
    /// detect chain reorganizations.
    ///
    /// # Arguments
    /// * `height` - Block height to look up
    ///
    /// # Returns
    /// Some(hash) if a block exists at that height, None otherwise
    ///
    /// # Errors
    /// Returns error if query fails.
    async fn lookup_block_hash(&self, height: u32) -> Result<Option<String>>;

    // Rollback (for chain reorganization handling)

    /// Roll back all data associated with a block at the given height
    ///
    /// Deletes the block node, all its transactions, inputs, outputs,
    /// and relationships. Also reverts the spent status of outputs from
    /// earlier blocks that were marked as spent by this block's transactions.
    ///
    /// **IMPORTANT**: Must only be called on the current chain tip.
    /// Rolling back from the middle would leave orphaned data.
    /// Always roll back from tip to fork point in reverse height order.
    ///
    /// # Arguments
    /// * `height` - Block height to roll back
    ///
    /// # Errors
    /// Returns error if any rollback step fails.
    async fn rollback_block(&self, height: u32) -> Result<()>;

    // Crash Recovery

    /// Get the maximum block height currently in the database
    ///
    /// Used for crash recovery to detect blocks written but not checkpointed.
    /// If the database has blocks beyond the checkpoint, this indicates a
    /// crash occurred mid-batch after writing some blocks but before updating
    /// the checkpoint.
    ///
    /// # Returns
    /// Some(height) if blocks exist, None if database is empty
    ///
    /// # Errors
    /// Returns error if query fails.
    async fn get_max_block_height(&self) -> Result<Option<u32>>;

    /// Check if a block at given height is complete
    ///
    /// Compares the block's expected transaction count (from block.txCount)
    /// with the actual number of Transaction nodes linked to it. A mismatch
    /// indicates the block was partially written during a crashed batch.
    ///
    /// # Arguments
    /// * `height` - Block height to check
    ///
    /// # Returns
    /// (expected_tx_count, actual_tx_count) tuple
    ///
    /// # Errors
    /// Returns error if block doesn't exist or query fails.
    async fn check_block_complete(&self, height: u32) -> Result<(u32, u32)>;

    // =========================================================================
    // Fast Write Methods (CREATE instead of MERGE)
    // =========================================================================
    //
    // These methods use CREATE queries for better performance during forward
    // ingestion when we know nodes don't exist yet. They will fail with
    // constraint violations if duplicates exist.
    //
    // Default implementations delegate to the regular methods, allowing
    // implementations that don't need optimization (like MockWriter) to
    // work without changes.

    /// Fast write blocks using CREATE (no existence check)
    ///
    /// Use only during forward ingestion when blocks are guaranteed new.
    /// Will fail with constraint violation if block already exists.
    ///
    /// Default implementation delegates to `write_blocks()`.
    async fn write_blocks_fast(&self, blocks: &[BlockData]) -> Result<()> {
        self.write_blocks(blocks).await
    }

    /// Fast write transactions using CREATE (no existence check)
    ///
    /// Use only during forward ingestion when transactions are guaranteed new.
    /// Will fail with constraint violation if transaction already exists.
    ///
    /// Default implementation delegates to `write_transactions()`.
    async fn write_transactions_fast(&self, transactions: &[TransactionData]) -> Result<()> {
        self.write_transactions(transactions).await
    }

    /// Fast write outputs using CREATE (no existence check)
    ///
    /// Use only during forward ingestion when outputs are guaranteed new.
    /// Will fail with constraint violation if output already exists.
    ///
    /// Default implementation delegates to `write_outputs()`.
    async fn write_outputs_fast(&self, outputs: &[OutputData]) -> Result<()> {
        self.write_outputs(outputs).await
    }

    /// Fast write HAS_OUTPUT relationships using CREATE (no existence check)
    ///
    /// Default implementation delegates to `write_has_output_relationships()`.
    async fn write_has_output_relationships_fast(&self, outputs: &[OutputData]) -> Result<()> {
        self.write_has_output_relationships(outputs).await
    }

    /// Fast write inputs using CREATE (no existence check)
    ///
    /// Use only during forward ingestion when inputs are guaranteed new.
    /// Will fail with constraint violation if input already exists.
    ///
    /// Default implementation delegates to `write_inputs()`.
    async fn write_inputs_fast(&self, inputs: &[InputData]) -> Result<()> {
        self.write_inputs(inputs).await
    }

    // =========================================================================
    // Transaction Management
    // =========================================================================
    //
    // Explicit transaction support for atomic batch operations.
    // All phase writes within a batch chunk execute inside a single transaction.
    // If any phase fails, the entire transaction is rolled back.

    /// Begin an explicit database transaction
    ///
    /// All subsequent write operations will be part of this transaction
    /// until `commit_transaction()` or `rollback_transaction()` is called.
    ///
    /// # Errors
    /// Returns error if transaction creation fails.
    async fn begin_transaction(&self) -> Result<()>;

    /// Commit the current explicit transaction
    ///
    /// Atomically persists all writes since `begin_transaction()`.
    ///
    /// # Errors
    /// Returns error if commit fails (e.g., timeout, memory pressure).
    async fn commit_transaction(&self) -> Result<()>;

    /// Roll back the current explicit transaction
    ///
    /// Discards all writes since `begin_transaction()`.
    ///
    /// # Errors
    /// Returns error if rollback fails.
    async fn rollback_transaction(&self) -> Result<()>;
}
