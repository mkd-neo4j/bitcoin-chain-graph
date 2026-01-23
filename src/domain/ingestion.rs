//! Ingestion orchestrator - coordinates 6-phase blockchain ingestion
//!
//! The IngestionOrchestrator is the central domain layer component that orchestrates
//! the multi-phase ingestion process for Bitcoin blocks. It is generic over the
//! GraphWriter trait, allowing it to work with MockWriter (for testing) or Neo4jWriter
//! (for production).
//!
//! # 6-Phase Ingestion Process
//!
//! 1. **Phase 1: Block Nodes** - Create Block nodes with NEXT_BLOCK relationships
//! 2. **Phase 2: Transaction Nodes** - Create Transaction nodes with INCLUDED_IN relationships
//! 3. **Phase 3: Output Nodes** - Create Output nodes with LOCKED_TO relationships to addresses
//! 4. **Phase 4: Input Nodes** - Create Input nodes with SPENDS relationships to outputs
//! 5. **Phase 5: Calculate Amounts** - Update transaction amounts (totalInput, totalOutput, fee)
//! 6. **Phase 6: Simplified Layer** - Create PERFORMS and BENEFITS_TO relationships
//!
//! See [INGESTION_ARCHITECTURE.md](../../docs/INGESTION_ARCHITECTURE.md) for detailed design.

use bitcoin::{Block, Network};
use crate::domain::{BlockData, TransactionData, OutputData, InputData};
use crate::writer::{GraphWriter, Result};
use std::sync::Arc;

/// Orchestrates the 6-phase ingestion process
///
/// Generic over `W: GraphWriter` to support both testing (MockWriter) and
/// production (Neo4jWriter) implementations.
///
/// # Example
/// ```no_run
/// use bitcoin_chain_graph::domain::IngestionOrchestrator;
/// use bitcoin_chain_graph::writer::MockWriter;
/// use bitcoin::Network;
///
/// #[tokio::main]
/// async fn main() {
///     let writer = MockWriter::new();
///     let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin);
///
///     // Initialize schema
///     orchestrator.init_schema().await.unwrap();
///
///     // Ingest a block (all 6 phases)
///     // let block = ...; // Read from .blk file
///     // orchestrator.ingest_block(&block, 0).await.unwrap();
/// }
/// ```
pub struct IngestionOrchestrator<W: GraphWriter> {
    writer: Arc<W>,
    network: Network,
}

impl<W: GraphWriter> IngestionOrchestrator<W> {
    /// Create a new orchestrator with the given writer and network
    ///
    /// # Arguments
    /// * `writer` - Implementation of GraphWriter trait (MockWriter or Neo4jWriter)
    /// * `network` - Bitcoin network for address derivation (Bitcoin, Testnet, Regtest)
    pub fn new(writer: W, network: Network) -> Self {
        Self {
            writer: Arc::new(writer),
            network,
        }
    }

    /// Initialize database schema
    ///
    /// Creates all required constraints and indexes. Should be called once
    /// before starting ingestion.
    ///
    /// # Errors
    /// Returns error if schema initialization fails.
    pub async fn init_schema(&self) -> Result<()> {
        self.writer.init_schema().await
    }

    /// Ingest a single block through all 6 phases
    ///
    /// Processes one block completely before moving to the next. This ensures
    /// all dependencies are satisfied (outputs exist before inputs reference them).
    ///
    /// # Arguments
    /// * `block` - Bitcoin block from parser
    /// * `height` - Block height (0 = genesis)
    ///
    /// # Returns
    /// Ok(()) if all 6 phases complete successfully
    ///
    /// # Errors
    /// Returns error if any phase fails. The transaction should be rolled back
    /// to maintain consistency.
    pub async fn ingest_block(&self, block: &Block, height: u32) -> Result<()> {
        // Phase 1: Create Block node
        self.ingest_block_node(block, height).await?;

        // Phase 2: Create Transaction nodes
        self.ingest_transactions(block, height).await?;

        // Phase 3: Create Output nodes and address relationships
        self.ingest_outputs(block, height).await?;

        // Phase 4: Create Input nodes and SPENDS relationships
        self.ingest_inputs(block, height).await?;

        // Phase 5: Calculate transaction amounts
        self.calculate_transaction_amounts(block).await?;

        // Phase 6: Create simplified layer relationships
        self.write_simplified_layer(block).await?;

        Ok(())
    }

    /// Phase 1: Create Block node
    ///
    /// Creates a Block node with all properties and NEXT_BLOCK relationship
    /// to the previous block (if not genesis).
    async fn ingest_block_node(&self, block: &Block, height: u32) -> Result<()> {
        let block_data = BlockData::from_block(block, height);
        self.writer.write_blocks(&[block_data]).await
    }

    /// Phase 2: Create Transaction nodes
    ///
    /// Creates Transaction nodes with INCLUDED_IN relationships to the block.
    /// Note: totalInput, totalOutput, and fee are calculated later in Phase 5.
    async fn ingest_transactions(&self, block: &Block, height: u32) -> Result<()> {
        let block_hash = block.block_hash().to_string();
        let timestamp = block.header.time as i64;

        let transactions: Vec<TransactionData> = block
            .txdata
            .iter()
            .map(|tx| TransactionData::from_transaction(tx, height, &block_hash, timestamp))
            .collect();

        self.writer.write_transactions(&transactions).await
    }

    /// Phase 3: Create Output nodes and LOCKED_TO relationships
    ///
    /// Creates Output nodes with properties, HAS_OUTPUT relationships to transactions,
    /// and LOCKED_TO relationships to addresses (where derivable).
    async fn ingest_outputs(&self, block: &Block, _height: u32) -> Result<()> {
        let mut all_outputs = Vec::new();

        for tx in &block.txdata {
            let txid = tx.txid().to_string();

            for (output_index, output) in tx.output.iter().enumerate() {
                let output_data = OutputData::from_output(
                    output,
                    &txid,
                    output_index as u32,
                    self.network,
                );
                all_outputs.push(output_data);
            }
        }

        self.writer.write_outputs(&all_outputs).await
    }

    /// Phase 4: Create Input nodes and SPENDS relationships
    ///
    /// Creates Input nodes with properties, HAS_INPUT relationships to transactions,
    /// and SPENDS relationships to previous outputs. Also updates spent outputs
    /// with spent metadata.
    ///
    /// Coinbase inputs are created but have no SPENDS relationship (they don't
    /// spend any previous output).
    async fn ingest_inputs(&self, block: &Block, _height: u32) -> Result<()> {
        let mut all_inputs = Vec::new();

        for tx in &block.txdata {
            let txid = tx.txid().to_string();

            for (input_index, input) in tx.input.iter().enumerate() {
                let input_data = InputData::from_input(
                    input,
                    &txid,
                    input_index as u32,
                );
                all_inputs.push(input_data);
            }
        }

        self.writer.write_inputs(&all_inputs).await
    }

    /// Phase 5: Calculate transaction amounts
    ///
    /// Updates Transaction nodes with:
    /// - totalInput = sum of all spent output amounts
    /// - totalOutput = sum of all created output amounts
    /// - fee = totalInput - totalOutput (0 for coinbase)
    ///
    /// This phase requires SPENDS relationships to exist (Phase 4 complete).
    async fn calculate_transaction_amounts(&self, block: &Block) -> Result<()> {
        let transactions: Vec<TransactionData> = block
            .txdata
            .iter()
            .map(|tx| {
                let block_hash = block.block_hash().to_string();
                let timestamp = block.header.time as i64;
                TransactionData::from_transaction(
                    tx,
                    0, // height not needed for this phase
                    &block_hash,
                    timestamp,
                )
            })
            .collect();

        self.writer.calculate_amounts(&transactions).await
    }

    /// Phase 6: Create simplified layer relationships
    ///
    /// Creates direct relationships for easier querying:
    /// - PERFORMS: Address → Transaction (who performed the transaction)
    /// - BENEFITS_TO: Transaction → Address (who received funds)
    ///
    /// These are derived from the detailed graph structure but provide
    /// a simpler view for common queries.
    async fn write_simplified_layer(&self, block: &Block) -> Result<()> {
        let transactions: Vec<TransactionData> = block
            .txdata
            .iter()
            .map(|tx| {
                let block_hash = block.block_hash().to_string();
                let timestamp = block.header.time as i64;
                TransactionData::from_transaction(
                    tx,
                    0, // height not needed for this phase
                    &block_hash,
                    timestamp,
                )
            })
            .collect();

        self.writer.write_simplified_layer(&transactions).await
    }
}
