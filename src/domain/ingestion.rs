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
use crate::writer::{GraphWriter, Result, WriterError};
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

impl<W: GraphWriter + 'static> IngestionOrchestrator<W> {
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

    /// Get reference to the UTXO cache
    ///
    /// Returns a reference to the internal UTXO cache for direct access.
    /// This is useful for cache pre-warming during resume operations.
    ///
    /// # Example
    /// ```no_run
    /// // Pre-warm cache before resuming ingestion
    /// cache.enable_prewarm_mode();
    /// loader.prewarm_cache(orchestrator.get_cache(), start_height, 50).await?;
    /// cache.disable_prewarm_mode();
    /// ```
    pub fn get_cache(&self) -> &UtxoCache<W> {
        &self.utxo_cache
    }

    /// Get current checkpoint
    ///
    /// Returns the current checkpoint state for status reporting.
    ///
    /// # Returns
    /// Some(CheckpointData) if checkpoint exists, None otherwise
    ///
    /// # Errors
    /// Returns error if checkpoint query fails
    pub async fn get_checkpoint(&self) -> Result<Option<CheckpointData>> {
        self.writer.get_checkpoint().await
    }

    /// Set checkpoint status
    ///
    /// Updates the checkpoint status field. Useful for error recovery
    /// and manual status management.
    ///
    /// # Arguments
    /// * `status` - New status: "in_progress", "completed", "paused", "error"
    ///
    /// # Errors
    /// Returns error if status update fails
    pub async fn set_checkpoint_status(&self, status: &str) -> Result<()> {
        self.writer.set_checkpoint_status(status).await
    }

    /// Update checkpoint with new data
    ///
    /// Updates the checkpoint with progress information. Useful for testing
    /// and manual checkpoint management.
    ///
    /// # Arguments
    /// * `checkpoint` - Updated checkpoint data with latest progress
    ///
    /// # Errors
    /// Returns error if checkpoint update fails
    pub async fn update_checkpoint(&self, checkpoint: &CheckpointData) -> Result<()> {
        self.writer.update_checkpoint(checkpoint).await
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

        // Phase 4: Create Input nodes (cache removal deferred to Phase 7)
        self.ingest_inputs(block, height).await?;

        // Phase 5: REMOVED in M7 - amounts calculated in Phase 3

        // Phase 6: Create simplified layer from pre-aggregated data (M7)
        self.write_simplified_layer_rust(block).await?;

        // Phase 7: Remove spent outputs from cache (must be AFTER Phase 6!)
        self.remove_spent_outputs_from_cache(block).await?;

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
            let phase1_start = std::time::Instant::now();
            let mut block_data_batch = Vec::with_capacity(blocks_in_batch);
            for (height, block, _file_name) in chunk {
                block_data_batch.push(BlockData::from_block(block, *height));
            }

            // Write blocks in one batch
            let write_start = std::time::Instant::now();
            self.writer.write_blocks(&block_data_batch).await?;
            tracing::debug!(
                phase = "1_blocks",
                count = block_data_batch.len(),
                accumulate_secs = format!("{:.2}", phase1_start.elapsed().as_secs_f64()),
                write_secs = format!("{:.2}", write_start.elapsed().as_secs_f64()),
                "Phase complete"
            );

            // Phase 2: Accumulate outputs and populate cache (BEFORE transactions!)
            let phase2_start = std::time::Instant::now();
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
            let write_start = std::time::Instant::now();
            self.writer.write_outputs(&output_data_batch).await?;
            tracing::debug!(
                phase = "2_outputs",
                count = output_data_batch.len(),
                accumulate_secs = format!("{:.2}", phase2_start.elapsed().as_secs_f64()),
                write_secs = format!("{:.2}", write_start.elapsed().as_secs_f64()),
                "Phase complete"
            );

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
            let phase3_start = std::time::Instant::now();
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
            let write_start = std::time::Instant::now();
            self.writer.write_transactions(&transaction_data_batch).await?;
            tracing::debug!(
                phase = "3_transactions",
                count = transaction_data_batch.len(),
                accumulate_secs = format!("{:.2}", phase3_start.elapsed().as_secs_f64()),
                write_secs = format!("{:.2}", write_start.elapsed().as_secs_f64()),
                "Phase complete"
            );

            // Phase 4: Accumulate inputs (cache removal deferred to Phase 6)
            let phase4_start = std::time::Instant::now();
            let mut input_data_batch = Vec::new();
            for (_height, block, _file_name) in chunk {
                for tx in &block.txdata {
                    let txid = tx.txid().to_string();
                    for (input_index, input) in tx.input.iter().enumerate() {
                        let input_data = InputData::from_input(input, &txid, input_index as u32);
                        input_data_batch.push(input_data);
                    }
                }
            }

            // Write inputs in one batch
            let write_start = std::time::Instant::now();
            self.writer.write_inputs(&input_data_batch).await?;
            tracing::debug!(
                phase = "4_inputs",
                count = input_data_batch.len(),
                accumulate_secs = format!("{:.2}", phase4_start.elapsed().as_secs_f64()),
                write_secs = format!("{:.2}", write_start.elapsed().as_secs_f64()),
                "Phase complete"
            );

            // Phase 5: Simplified layer (batch accumulation + parallel writes)
            let phase5_start = std::time::Instant::now();

            // Accumulate ALL simplified layer data from batch
            let mut all_performs_data = Vec::new();
            let mut all_benefits_to_data = Vec::new();

            for (_height, block, _file_name) in chunk {
                // Extract simplified layer data for this block
                let (performs_data, benefits_to_data) = self.extract_simplified_layer_data(block).await?;
                all_performs_data.extend(performs_data);
                all_benefits_to_data.extend(benefits_to_data);
            }

            // Partition data by address hash to enable parallel writes without deadlocks
            const NUM_BUCKETS: usize = 4;

            let performs_buckets = Self::partition_performs_by_address(&all_performs_data, NUM_BUCKETS);
            let benefits_buckets = Self::partition_benefits_by_address(&all_benefits_to_data, NUM_BUCKETS);

            // Spawn parallel tasks (one per bucket)
            let mut tasks = Vec::new();

            for bucket_idx in 0..NUM_BUCKETS {
                let performs = performs_buckets[bucket_idx].clone();
                let benefits = benefits_buckets[bucket_idx].clone();
                let writer = Arc::clone(&self.writer);

                let task = tokio::spawn(async move {
                    // Write PERFORMS first, then BENEFITS_TO (sequential within bucket)
                    // This ensures same addresses are handled without deadlock
                    if !performs.is_empty() {
                        writer.write_performs(&performs).await?;
                    }
                    if !benefits.is_empty() {
                        writer.write_benefits_to(&benefits).await?;
                    }
                    Ok::<_, WriterError>(())
                });

                tasks.push(task);
            }

            // Wait for all buckets to complete
            for (idx, task) in tasks.into_iter().enumerate() {
                task.await
                    .map_err(|e| WriterError::QueryFailed(format!("Bucket {} task panicked: {}", idx, e)))??;
            }

            tracing::debug!(
                phase = "5_simplified",
                performs_count = all_performs_data.len(),
                benefits_count = all_benefits_to_data.len(),
                write_secs = format!("{:.2}", phase5_start.elapsed().as_secs_f64()),
                "Phase complete"
            );

            // Phase 6: Remove spent outputs from cache (deferred from Phase 4)
            // This must happen AFTER Phase 5 because Phase 5 needs to lookup spent outputs
            for (_height, block, _file_name) in chunk {
                for tx in &block.txdata {
                    if !tx.is_coinbase() {
                        for input in &tx.input {
                            let output_id = format!("{}:{}", input.previous_output.txid, input.previous_output.vout);
                            self.utxo_cache.remove(&output_id);
                        }
                    }
                }
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

    /// Phase 4: Create Input nodes (cache removal deferred to Phase 7)
    ///
    /// Creates Input nodes with properties, HAS_INPUT relationships to transactions,
    /// and SPENDS relationships to previous outputs. Also updates spent outputs
    /// with spent metadata.
    ///
    /// **M7 Note**: Cache removal is deferred until after Phase 6 (simplified layer)
    /// because Phase 6 needs to lookup spent outputs to build PERFORMS relationships.
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

        // Write inputs to database
        self.writer.write_inputs(&all_inputs).await
    }

    /// Phase 7: Remove spent outputs from UTXO cache
    ///
    /// Must be called AFTER Phase 6 (simplified layer) because Phase 6 needs
    /// to lookup spent outputs to build PERFORMS relationships.
    ///
    /// Removes spent outputs from cache to keep it focused on unspent outputs.
    async fn remove_spent_outputs_from_cache(&self, block: &Block) -> Result<()> {
        for tx in &block.txdata {
            if !tx.is_coinbase() {
                for input in &tx.input {
                    let output_id = format!("{}:{}", input.previous_output.txid, input.previous_output.vout);
                    self.utxo_cache.remove(&output_id);
                }
            }
        }
        Ok(())
    }

    /// Extract simplified layer data from a block (PERFORMS + BENEFITS_TO)
    ///
    /// Extracts data WITHOUT writing to database, allowing batch accumulation.
    /// This enables batching multiple blocks together for parallel writes.
    ///
    /// # Returns
    /// Tuple of (PERFORMS data, BENEFITS_TO data) for this block
    async fn extract_simplified_layer_data(&self, block: &Block) -> Result<(Vec<PerformsData>, Vec<BenefitsToData>)> {
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

        Ok((performs_data, benefits_to_data))
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
    ///
    /// This method is used for single-block ingestion. For batch ingestion,
    /// use `extract_simplified_layer_data()` followed by parallel writes.
    async fn write_simplified_layer_rust(&self, block: &Block) -> Result<()> {
        // Extract data using the shared extraction method
        let (performs_data, benefits_to_data) = self.extract_simplified_layer_data(block).await?;

        // Write pre-aggregated relationships to database (no graph traversal)
        if !performs_data.is_empty() {
            self.writer.write_performs(&performs_data).await?;
        }

        if !benefits_to_data.is_empty() {
            self.writer.write_benefits_to(&benefits_to_data).await?;
        }

        Ok(())
    }

    /// Calculate deterministic hash for a string
    ///
    /// Uses standard library hash to ensure consistent bucket assignment.
    fn calculate_string_hash(s: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        s.hash(&mut hasher);
        hasher.finish()
    }

    /// Partition PERFORMS data into buckets by address hash
    ///
    /// Distributes data into N buckets based on from_address hash to enable
    /// parallel writes without deadlocks (different buckets = different addresses).
    fn partition_performs_by_address(performs: &[PerformsData], num_buckets: usize) -> Vec<Vec<PerformsData>> {
        let mut buckets: Vec<Vec<PerformsData>> = (0..num_buckets)
            .map(|_| Vec::new())
            .collect();

        for item in performs {
            // Hash address and map to bucket
            let hash = Self::calculate_string_hash(&item.from_address);
            let bucket_idx = (hash % num_buckets as u64) as usize;
            buckets[bucket_idx].push(item.clone());
        }

        buckets
    }

    /// Partition BENEFITS_TO data into buckets by address hash
    ///
    /// Distributes data into N buckets based on to_address hash to enable
    /// parallel writes without deadlocks (different buckets = different addresses).
    fn partition_benefits_by_address(benefits: &[BenefitsToData], num_buckets: usize) -> Vec<Vec<BenefitsToData>> {
        let mut buckets: Vec<Vec<BenefitsToData>> = (0..num_buckets)
            .map(|_| Vec::new())
            .collect();

        for item in benefits {
            // Hash address and map to bucket
            let hash = Self::calculate_string_hash(&item.to_address);
            let bucket_idx = (hash % num_buckets as u64) as usize;
            buckets[bucket_idx].push(item.clone());
        }

        buckets
    }
}
