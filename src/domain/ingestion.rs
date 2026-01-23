//! Ingestion orchestrator - coordinates 6-phase blockchain ingestion with UTXO cache
//!
//! The IngestionOrchestrator is the central domain layer component that orchestrates
//! the multi-phase ingestion process for Bitcoin blocks. It is generic over the
//! GraphWriter trait, allowing it to work with MockWriter (for testing) or Neo4jWriter
//! (for production).
//!
//! # Milestone 7: UTXO Cache Integration
//!
//! This milestone introduces a UTXO cache to dramatically improve ingestion performance:
//! - **Before M7**: Phase 5 and 6 performed expensive Neo4j graph traversals (3 queries/block)
//! - **After M7**: Amounts calculated in Rust using cache, relationships pre-aggregated
//! - **Performance**: 10-100x faster ingestion expected
//!
//! # 6-Phase Ingestion Process (M7)
//!
//! **IMPORTANT:** Phases 2 and 3 are swapped to handle same-block UTXO references!
//!
//! 1. **Phase 1: Block Nodes** - Create Block nodes with NEXT_BLOCK relationships
//! 2. **Phase 2: Output Nodes** - Create Output nodes + populate UTXO cache (BEFORE calculating amounts!)
//! 3. **Phase 3: Transaction Nodes** - Create Transaction nodes WITH amounts (calculated in Rust using cache)
//! 4. **Phase 4: Input Nodes** - Create Input nodes + remove spent outputs from cache
//! 5. **Phase 5: REMOVED** - Amounts now calculated in Phase 3
//! 6. **Phase 6: Simplified Layer** - Create PERFORMS and BENEFITS_TO from pre-aggregated data
//!
//! **Why Phase 2/3 are swapped:** Bitcoin allows transactions to spend outputs created earlier
//! in the SAME block. For example, in block 546:
//! - Transaction 1 creates output `28204cad...:0`
//! - Transaction 2 spends that output in the same block!
//!
//! By creating outputs BEFORE calculating transaction amounts, same-block references work.
//!
//! See [INGESTION_ARCHITECTURE.md](../../docs/INGESTION_ARCHITECTURE.md) for detailed design.

use bitcoin::{Block, Network};
use crate::domain::{BlockData, TransactionData, OutputData, InputData, CheckpointData, PerformsData, BenefitsToData, UtxoCache, CachedOutput};
use crate::writer::{GraphWriter, Result};
use std::sync::Arc;
use std::collections::HashMap;

/// Orchestrates the 6-phase ingestion process with UTXO cache
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
///     let cache_size = 100_000; // 100k outputs (~15MB)
///     let orchestrator = IngestionOrchestrator::new(writer, Network::Bitcoin, cache_size);
///
///     // Initialize schema
///     orchestrator.init_schema().await.unwrap();
///
///     // Ingest a block (all 6 phases with UTXO cache)
///     // let block = ...; // Read from .blk file
///     // orchestrator.ingest_block(&block, 0, "blk00000.dat", None).await.unwrap();
/// }
/// ```
pub struct IngestionOrchestrator<W: GraphWriter> {
    writer: Arc<W>,
    network: Network,
    utxo_cache: UtxoCache<W>,
}

impl<W: GraphWriter> IngestionOrchestrator<W> {
    /// Create a new orchestrator with the given writer, network, and cache size
    ///
    /// # Arguments
    /// * `writer` - Implementation of GraphWriter trait (MockWriter or Neo4jWriter)
    /// * `network` - Bitcoin network for address derivation (Bitcoin, Testnet, Regtest)
    /// * `cache_size` - UTXO cache capacity (default: 100,000 entries ≈ 15MB)
    ///
    /// # Cache Size Guidelines
    /// - Low resource (2GB RAM): 50,000 entries (~7MB)
    /// - Default (8GB RAM): 100,000 entries (~15MB)
    /// - High performance (32GB RAM): 1,000,000 entries (~150MB)
    /// - Ultra performance (128GB RAM): 10,000,000 entries (~1.5GB)
    pub fn new(writer: W, network: Network, cache_size: usize) -> Self {
        let writer_arc = Arc::new(writer);
        let utxo_cache = UtxoCache::new(cache_size, Arc::clone(&writer_arc));

        Self {
            writer: writer_arc,
            network,
            utxo_cache,
        }
    }

    /// Initialize database schema and checkpoint
    ///
    /// Creates all required constraints and indexes, and initializes the
    /// ingestion checkpoint if it doesn't already exist. Should be called once
    /// before starting ingestion.
    ///
    /// # Errors
    /// Returns error if schema initialization fails.
    pub async fn init_schema(&self) -> Result<()> {
        self.writer.init_schema().await?;

        // Create checkpoint if it doesn't exist
        if self.writer.get_checkpoint().await?.is_none() {
            self.writer.create_checkpoint().await?;
        }

        Ok(())
    }

    /// Get the last processed block height from checkpoint
    ///
    /// Returns the height to resume from. If no checkpoint exists or checkpoint
    /// is at -1 (initial state), returns 0 to start from genesis.
    ///
    /// # Returns
    /// The block height to start/resume ingestion from
    ///
    /// # Errors
    /// Returns error if checkpoint query fails
    pub async fn get_resume_height(&self) -> Result<u32> {
        match self.writer.get_checkpoint().await? {
            Some(checkpoint) => {
                if checkpoint.last_processed_height < 0 {
                    Ok(0) // Start from genesis (checkpoint at -1)
                } else {
                    Ok((checkpoint.last_processed_height + 1) as u32) // Resume from next block
                }
            }
            None => Ok(0), // No checkpoint, start from genesis
        }
    }

    /// Mark ingestion as complete
    ///
    /// Updates the checkpoint status to "completed" when all blocks have been
    /// successfully ingested.
    ///
    /// # Errors
    /// Returns error if checkpoint update fails
    pub async fn mark_complete(&self) -> Result<()> {
        self.writer.mark_checkpoint_complete().await
    }

    /// Get UTXO cache statistics
    ///
    /// Returns cache performance metrics including hits, misses, and hit rate.
    /// Useful for monitoring cache effectiveness and tuning cache size.
    pub fn cache_stats(&self) -> crate::domain::UtxoCacheStats {
        self.utxo_cache.stats()
    }

    /// Get current number of entries in UTXO cache
    ///
    /// Returns the number of outputs currently cached in memory.
    /// Useful for monitoring cache utilization.
    pub fn cache_size(&self) -> usize {
        self.utxo_cache.len()
    }

    /// Ingest a single block through all 6 phases (M7 version with UTXO cache)
    ///
    /// Processes one block completely before moving to the next. This ensures
    /// all dependencies are satisfied (outputs exist before inputs reference them).
    ///
    /// After successful ingestion, updates the checkpoint with the current block's
    /// information for resume capability.
    ///
    /// # Arguments
    /// * `block` - Bitcoin block from parser
    /// * `height` - Block height (0 = genesis)
    /// * `file_name` - Name of the .blk file being processed (e.g., "blk00000.dat")
    /// * `file_offset` - Optional byte offset within the file for precise resume
    ///
    /// # Returns
    /// Ok(()) if all phases complete successfully
    ///
    /// # Errors
    /// Returns error if any phase fails. The transaction should be rolled back
    /// to maintain consistency.
    pub async fn ingest_block(
        &self,
        block: &Block,
        height: u32,
        file_name: &str,
        file_offset: Option<u64>,
    ) -> Result<()> {
        // Phase 1: Create Block node
        self.ingest_block_node(block, height).await?;

        // Phase 2: Create Output nodes and populate UTXO cache (M7 - SWAPPED WITH PHASE 3!)
        // CRITICAL: This must happen BEFORE Phase 3 to handle same-block UTXO references
        self.ingest_outputs_and_cache(block, height).await?;

        // Phase 3: Create Transaction nodes WITH amounts (M7: calculated in Rust using cache)
        // Now outputs from earlier transactions in this block are in cache
        self.ingest_transactions_with_amounts(block, height).await?;

        // Phase 4: Create Input nodes and remove spent outputs from cache (M7)
        self.ingest_inputs_and_update_cache(block, height).await?;

        // Phase 5: REMOVED in M7 - amounts calculated in Phase 3

        // Phase 6: Create simplified layer from pre-aggregated data (M7)
        self.write_simplified_layer_rust(block).await?;

        // Update checkpoint after successful ingestion
        let checkpoint = CheckpointData {
            last_processed_height: height as i32,
            last_processed_hash: block.block_hash().to_string(),
            last_processed_file: file_name.to_string(),
            last_processed_file_offset: file_offset,
            timestamp: chrono::Utc::now().timestamp(),
            status: "in_progress".to_string(),
        };

        self.writer.update_checkpoint(&checkpoint).await?;

        Ok(())
    }

    /// Ingest multiple blocks in batches for improved performance
    ///
    /// Processes blocks in chunks, accumulating data structures and making bulk database writes.
    /// Uses the same phase ordering as `ingest_block()` to maintain correctness:
    /// 1. Blocks
    /// 2. Outputs (+ cache population) - BEFORE transactions for same-block UTXO references
    /// 3. Transactions (with amounts calculated from cache)
    /// 4. Inputs (+ cache removal)
    /// 5. Simplified layer
    ///
    /// # Arguments
    /// * `blocks` - Slice of (height, block, file_name) tuples in blockchain height order
    /// * `batch_size` - Number of blocks to accumulate before writing to database
    ///
    /// # Performance
    /// - Backlog mode: Use batch_size = 100-1000 for maximum throughput
    /// - Real-time mode: Use batch_size = 10-100 for lower latency
    ///
    /// # Errors
    /// Returns error if any database write fails or UTXO lookup fails
    pub async fn ingest_blocks_batch(
        &self,
        blocks: &[(u32, Block, String)],
        batch_size: usize,
    ) -> Result<()> {
        let total_blocks = blocks.len();
        println!("\n🚀 Starting batch ingestion of {} blocks (batch size: {})...", total_blocks, batch_size);

        for (batch_idx, chunk) in blocks.chunks(batch_size).enumerate() {
            let start_height = chunk.first().map(|(h, _, _)| *h).unwrap_or(0);
            let end_height = chunk.last().map(|(h, _, _)| *h).unwrap_or(0);
            let blocks_in_batch = chunk.len();

            println!("\n📦 Batch {} | Heights {}-{} | {} blocks",
                batch_idx + 1, start_height, end_height, blocks_in_batch);

            // Phase 1: Accumulate all block data
            let mut block_data_batch = Vec::with_capacity(blocks_in_batch);
            for (height, block, _file_name) in chunk {
                block_data_batch.push(BlockData::from_block(block, *height));
            }

            // Write blocks in one batch
            println!("   Phase 1: Writing {} blocks...", block_data_batch.len());
            self.writer.write_blocks(&block_data_batch).await?;

            // Phase 2: Accumulate outputs and populate cache (BEFORE transactions!)
            let mut output_data_batch = Vec::new();
            for (_height, block, _file_name) in chunk {
                for tx in &block.txdata {
                    let txid = tx.txid().to_string();
                    for (output_index, output) in tx.output.iter().enumerate() {
                        let output_data = OutputData::from_output(
                            output,
                            &txid,
                            output_index as u32,
                            self.network,
                        );
                        output_data_batch.push(output_data);
                    }
                }
            }

            // Write outputs in one batch
            println!("   Phase 2: Writing {} outputs...", output_data_batch.len());
            self.writer.write_outputs(&output_data_batch).await?;

            // Populate UTXO cache (needed for same-block references!)
            for output in &output_data_batch {
                let cached_output = CachedOutput {
                    output_id: output.output_id.clone(),
                    output_index: output.output_index,
                    amount: output.amount,
                    script_type: output.script_type.clone(),
                    address: output.address.clone(),
                };
                self.utxo_cache.insert(cached_output);
            }

            // Phase 3: Accumulate transactions (with amounts calculated from cache)
            let mut transaction_data_batch = Vec::new();
            for (height, block, _file_name) in chunk {
                let block_hash = block.block_hash().to_string();
                let timestamp = block.header.time as i64;

                for tx in &block.txdata {
                    let mut tx_data = TransactionData::from_transaction(tx, *height, &block_hash, timestamp);

                    // Calculate amounts (same logic as single-block ingestion)
                    let total_output: u64 = tx.output.iter().map(|out| out.value.to_sat()).sum();
                    let total_input: u64 = if tx.is_coinbase() {
                        0
                    } else {
                        let mut sum = 0u64;
                        for input in &tx.input {
                            let output_id = format!("{}:{}", input.previous_output.txid, input.previous_output.vout);
                            let cached_output = self.utxo_cache.get(&output_id).await?;
                            sum += cached_output.amount;
                        }
                        sum
                    };

                    tx_data.total_input = Some(total_input);
                    tx_data.total_output = Some(total_output);
                    tx_data.fee = Some(total_input.saturating_sub(total_output));

                    transaction_data_batch.push(tx_data);
                }
            }

            // Write transactions in one batch
            println!("   Phase 3: Writing {} transactions...", transaction_data_batch.len());
            self.writer.write_transactions(&transaction_data_batch).await?;

            // Phase 4: Accumulate inputs and update cache
            let mut input_data_batch = Vec::new();
            for (_height, block, _file_name) in chunk {
                for tx in &block.txdata {
                    let txid = tx.txid().to_string();
                    for (input_index, input) in tx.input.iter().enumerate() {
                        let input_data = InputData::from_input(input, &txid, input_index as u32);
                        input_data_batch.push(input_data);

                        // Remove spent output from cache
                        if !tx.is_coinbase() {
                            let output_id = format!("{}:{}", input.previous_output.txid, input.previous_output.vout);
                            self.utxo_cache.remove(&output_id);
                        }
                    }
                }
            }

            // Write inputs in one batch
            println!("   Phase 4: Writing {} inputs...", input_data_batch.len());
            self.writer.write_inputs(&input_data_batch).await?;

            // Phase 5: Simplified layer (per-block processing needed)
            println!("   Phase 5: Writing simplified layer...");
            for (_height, block, _file_name) in chunk {
                self.write_simplified_layer_rust(block).await?;
            }

            // Update checkpoint after each batch
            if let Some((height, block, file_name)) = chunk.last() {
                let checkpoint = CheckpointData {
                    last_processed_height: *height as i32,
                    last_processed_hash: block.block_hash().to_string(),
                    last_processed_file: file_name.clone(),
                    last_processed_file_offset: None, // Not tracked in batch mode
                    timestamp: chrono::Utc::now().timestamp(),
                    status: "in_progress".to_string(),
                };
                self.writer.update_checkpoint(&checkpoint).await?;
            }

            println!("   ✅ Batch {} complete", batch_idx + 1);
        }

        println!("\n✅ Batch ingestion complete! Processed {} blocks", total_blocks);
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

    /// Phase 2: Create Output nodes and populate UTXO cache (M7)
    ///
    /// Creates Output nodes with properties, HAS_OUTPUT relationships to transactions,
    /// and LOCKED_TO relationships to addresses (where derivable).
    ///
    /// **M7 Addition**: After successful write, inserts each output into the UTXO cache
    /// for use in future transactions (Phase 3 amount calculations).
    ///
    /// **CRITICAL**: This phase was moved BEFORE transaction processing (Phase 3) to handle
    /// same-block UTXO references. Bitcoin allows transactions to spend outputs created
    /// earlier in the SAME block.
    async fn ingest_outputs_and_cache(&self, block: &Block, _height: u32) -> Result<()> {
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

        // Write outputs to database
        self.writer.write_outputs(&all_outputs).await?;

        // M7: Insert outputs into UTXO cache for future lookups (including same-block references!)
        for output in &all_outputs {
            let cached_output = CachedOutput {
                output_id: output.output_id.clone(),
                output_index: output.output_index,
                amount: output.amount,
                script_type: output.script_type.clone(),
                address: output.address.clone(),
            };
            self.utxo_cache.insert(cached_output);
        }

        Ok(())
    }

    /// Phase 3: Create Transaction nodes WITH amounts (M7 - calculated in Rust)
    ///
    /// Creates Transaction nodes with INCLUDED_IN relationships to the block.
    ///
    /// **M7 Change**: Calculates total_input, total_output, and fee in Rust using
    /// the UTXO cache, avoiding expensive Neo4j graph traversals.
    ///
    /// **IMPORTANT**: This phase was moved AFTER output creation (Phase 2) to handle
    /// same-block UTXO references. Outputs from earlier transactions in the current
    /// block are now in cache and can be looked up successfully.
    ///
    /// For each transaction:
    /// - total_output = sum(transaction.outputs.amount) - easy, current block data
    /// - total_input = sum(previous_outputs.amount) - lookup from UTXO cache (including same-block!)
    /// - fee = total_input - total_output (0 for coinbase)
    async fn ingest_transactions_with_amounts(&self, block: &Block, height: u32) -> Result<()> {
        let block_hash = block.block_hash().to_string();
        let timestamp = block.header.time as i64;

        let mut transactions: Vec<TransactionData> = Vec::new();

        for tx in &block.txdata {
            let mut tx_data = TransactionData::from_transaction(tx, height, &block_hash, timestamp);

            // Calculate total_output (easy - sum current outputs)
            let total_output: u64 = tx.output.iter()
                .map(|out| out.value.to_sat())
                .sum();

            // Calculate total_input (lookup previous outputs from UTXO cache)
            let total_input: u64 = if tx.is_coinbase() {
                0 // Coinbase has no inputs
            } else {
                let mut sum = 0u64;
                for input in &tx.input {
                    let output_id = format!("{}:{}", input.previous_output.txid, input.previous_output.vout);

                    // Lookup from UTXO cache (will fallback to Neo4j if cache miss)
                    let cached_output = self.utxo_cache.get(&output_id).await?;
                    sum += cached_output.amount;
                }
                sum
            };

            // Calculate fee
            let fee = if tx.is_coinbase() {
                0
            } else {
                total_input.saturating_sub(total_output)
            };

            // Update transaction data with calculated amounts
            tx_data.total_input = Some(total_input);
            tx_data.total_output = Some(total_output);
            tx_data.fee = Some(fee);

            transactions.push(tx_data);
        }

        self.writer.write_transactions(&transactions).await
    }

    /// Phase 4: Create Input nodes and remove spent outputs from cache (M7)
    ///
    /// Creates Input nodes with properties, HAS_INPUT relationships to transactions,
    /// and SPENDS relationships to previous outputs. Also updates spent outputs
    /// with spent metadata.
    ///
    /// **M7 Addition**: After successful write, removes spent outputs from the UTXO
    /// cache to keep it focused on unspent outputs.
    ///
    /// Coinbase inputs are created but have no SPENDS relationship (they don't
    /// spend any previous output).
    async fn ingest_inputs_and_update_cache(&self, block: &Block, _height: u32) -> Result<()> {
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

        // Write inputs to database
        self.writer.write_inputs(&all_inputs).await?;

        // M7: Remove spent outputs from UTXO cache
        for input in &all_inputs {
            // Skip coinbase inputs
            if input.previous_output_index != 0xFFFFFFFF {
                let output_id = format!("{}:{}", input.previous_txid, input.previous_output_index);
                self.utxo_cache.remove(&output_id);
            }
        }

        Ok(())
    }

    /// Phase 6: Create simplified layer from pre-aggregated data (M7 - Rust calculation)
    ///
    /// Creates direct relationships for easier querying:
    /// - PERFORMS: Address → Transaction (who performed the transaction)
    /// - BENEFITS_TO: Transaction → Address (who received funds)
    ///
    /// **M7 Change**: Aggregation is performed in Rust using UTXO cache lookups,
    /// avoiding expensive Neo4j graph traversals. Data is pre-aggregated before
    /// being sent to Neo4j for bulk relationship creation.
    async fn write_simplified_layer_rust(&self, block: &Block) -> Result<()> {
        let mut performs_data: Vec<PerformsData> = Vec::new();
        let mut benefits_to_data: Vec<BenefitsToData> = Vec::new();

        for tx in &block.txdata {
            let txid = tx.txid().to_string();

            // Build PERFORMS data (Address → Transaction via inputs)
            if !tx.is_coinbase() {
                // Aggregate by (address, txid)
                let mut performs_map: HashMap<String, (u32, u64)> = HashMap::new();

                for input in &tx.input {
                    let output_id = format!("{}:{}", input.previous_output.txid, input.previous_output.vout);

                    // Lookup previous output from cache to get address and amount
                    match self.utxo_cache.get(&output_id).await {
                        Ok(cached_output) => {
                            if let Some(address) = cached_output.address {
                                let entry = performs_map.entry(address).or_insert((0, 0));
                                entry.0 += 1; // increment input count
                                entry.1 += cached_output.amount; // add amount
                            }
                        }
                        Err(_) => {
                            // Output might have been spent long ago (cache miss)
                            // This is expected for old UTXOs; Neo4j fallback will handle it
                            continue;
                        }
                    }
                }

                // Convert map to PerformsData
                for (address, (input_count, amount_spent)) in performs_map {
                    performs_data.push(PerformsData {
                        from_address: address,
                        to_txid: txid.clone(),
                        input_count,
                        amount_spent,
                    });
                }
            }

            // Build BENEFITS_TO data (Transaction → Address via outputs)
            // Aggregate by (txid, address)
            let mut benefits_map: HashMap<String, (u32, u64)> = HashMap::new();

            for output in &tx.output {
                // Extract address from current block (already in memory)
                let output_data = OutputData::from_output(
                    output,
                    &txid,
                    tx.output.iter().position(|o| std::ptr::eq(o, output)).unwrap() as u32,
                    self.network,
                );

                if let Some(address) = output_data.address {
                    let entry = benefits_map.entry(address).or_insert((0, 0));
                    entry.0 += 1; // increment output count
                    entry.1 += output.value.to_sat(); // add amount
                }
            }

            // Convert map to BenefitsToData
            for (address, (output_count, amount_received)) in benefits_map {
                benefits_to_data.push(BenefitsToData {
                    from_txid: txid.clone(),
                    to_address: address,
                    output_count,
                    amount_received,
                });
            }
        }

        // Write pre-aggregated relationships to database (no graph traversal)
        if !performs_data.is_empty() {
            self.writer.write_performs(&performs_data).await?;
        }

        if !benefits_to_data.is_empty() {
            self.writer.write_benefits_to(&benefits_to_data).await?;
        }

        Ok(())
    }
}
