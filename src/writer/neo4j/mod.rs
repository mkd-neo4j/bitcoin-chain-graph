//! Neo4j implementation of GraphWriter trait
//!
//! Provides Neo4j-backed blockchain ingestion with connection pooling,
//! bulk operations, retry logic, and configuration-driven performance tuning.

use async_trait::async_trait;
use neo4rs::{query, BoltType, ConfigBuilder, Graph};
use std::sync::Arc;

use crate::config::Neo4jConfig;
use crate::domain::{
    BenefitsToData, BlockData, CheckpointData, InputData, OutputData, PerformsData, TransactionData,
};
use crate::writer::{GraphWriter, Result, WriterError};

mod conversions;
mod queries;
mod schema;

use conversions::*;

/// Sentinel height value for initial checkpoint state ("not yet started").
///
/// Uses -999 instead of -1 to avoid collision with the neo4rs driver bug
/// that misreads -1 as 255 (which is a valid block height).
const CHECKPOINT_INITIAL_HEIGHT: i64 = -999;

/// Neo4j implementation of GraphWriter
///
/// Connects to Neo4j database and implements all blockchain ingestion operations
/// with bulk writes, connection pooling, retry with exponential backoff, and
/// configurable performance settings.
pub struct Neo4jWriter {
    graph: Arc<Graph>,
    batch_size: usize,
    max_retries: usize,
}

impl Neo4jWriter {
    /// Create a new Neo4jWriter with configuration
    ///
    /// Establishes a connection to Neo4j and verifies connectivity with a
    /// health check query.
    ///
    /// # Arguments
    /// * `config` - Neo4j connection and pool configuration
    ///
    /// # Errors
    /// Returns error if connection to Neo4j fails or health check fails
    pub async fn new(config: Neo4jConfig) -> Result<Self> {
        let graph = Self::connect(&config).await?;

        let writer = Self {
            graph: Arc::new(graph),
            batch_size: config.write_batch_size,
            max_retries: config.max_retries,
        };

        // Verify the connection is alive
        writer.health_check().await?;

        Ok(writer)
    }

    /// Establish connection to Neo4j with configured connection pool
    async fn connect(config: &Neo4jConfig) -> Result<Graph> {
        let cfg = ConfigBuilder::default()
            .uri(&config.uri)
            .user(&config.user)
            .password(&config.password)
            .db(&*config.database)
            .max_connections(config.max_connections)
            .fetch_size(config.fetch_size)
            .build()
            .map_err(|e| WriterError::ConnectionFailed(format!("Config error: {}", e)))?;

        Graph::connect(cfg)
            .await
            .map_err(|e| WriterError::ConnectionFailed(format!("Connection error: {}", e)))
    }

    /// Verify the Neo4j connection is alive
    ///
    /// Executes a trivial query to check connectivity. Called at startup
    /// and can be used for monitoring.
    pub async fn health_check(&self) -> Result<()> {
        self.graph
            .run(query("RETURN 1"))
            .await
            .map_err(|e| WriterError::ConnectionFailed(format!("Health check failed: {}", e)))?;
        Ok(())
    }

    /// Get reference to underlying graph
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// Execute a query in batched chunks with retry, timing, and structured logging.
    ///
    /// Generic helper that eliminates repeated boilerplate across write methods.
    /// Each chunk is converted to bolt format, timed, and executed with automatic
    /// retry on transient failures.
    ///
    /// # Type Parameters
    /// * `T` - Domain type (BlockData, TransactionData, etc.)
    ///
    /// # Arguments
    /// * `items` - Full slice of domain objects to write
    /// * `query_str` - The Cypher query constant to execute
    /// * `param_name` - The UNWIND parameter name ("blocks", "transactions", etc.)
    /// * `operation_name` - Human-readable name for logging ("write_blocks", etc.)
    /// * `convert` - Closure that converts a chunk &[T] into Vec<BoltType>
    async fn execute_batched<T, F>(
        &self,
        items: &[T],
        query_str: &str,
        param_name: &str,
        operation_name: &str,
        convert: F,
    ) -> Result<()>
    where
        F: Fn(&[T]) -> Vec<BoltType>,
    {
        if items.is_empty() {
            return Ok(());
        }

        let total_batches = items.len().div_ceil(self.batch_size);

        for (i, chunk) in items.chunks(self.batch_size).enumerate() {
            let bolt_data = convert(chunk);
            let batch_num = i + 1;

            if i > 0 {
                tracing::debug!(
                    operation = operation_name,
                    batch = batch_num,
                    total_batches,
                    records = chunk.len(),
                    "Writing batch"
                );
            }

            let start = std::time::Instant::now();

            self.run_with_retry(
                operation_name,
                || {
                    let q = query(query_str).param(param_name, bolt_data.as_slice());
                    async { self.graph.run(q).await }
                },
                batch_num,
                total_batches,
                chunk.len(),
            )
            .await?;

            tracing::debug!(
                operation = operation_name,
                batch = batch_num,
                total_batches,
                records = chunk.len(),
                elapsed_ms = start.elapsed().as_millis() as u64,
                "Batch write complete"
            );
        }

        Ok(())
    }

    /// Execute an async Neo4j operation with exponential backoff retry.
    ///
    /// Retries on transient errors (QueryFailed, ConnectionFailed) up to
    /// `max_retries` times with exponential backoff (200ms, 400ms, 800ms, ...).
    async fn run_with_retry<F, Fut>(
        &self,
        operation_name: &str,
        f: F,
        batch_num: usize,
        total_batches: usize,
        record_count: usize,
    ) -> Result<()>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = std::result::Result<(), neo4rs::Error>>,
    {
        let mut attempt = 0;
        loop {
            match f().await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    let writer_err = WriterError::QueryFailed(format!(
                        "{} failed (batch {}/{}, {} records): {}",
                        operation_name, batch_num, total_batches, record_count, e
                    ));

                    if attempt < self.max_retries && writer_err.is_retryable() {
                        attempt += 1;
                        let delay =
                            std::time::Duration::from_millis(200 * (2_u64.pow(attempt as u32 - 1)));
                        tracing::warn!(
                            operation = operation_name,
                            attempt,
                            max_retries = self.max_retries,
                            delay_ms = delay.as_millis() as u64,
                            error = %e,
                            "Retrying after transient failure"
                        );
                        tokio::time::sleep(delay).await;
                    } else {
                        return Err(writer_err);
                    }
                }
            }
        }
    }
}

#[async_trait]
impl GraphWriter for Neo4jWriter {
    async fn init_schema(&self) -> Result<()> {
        schema::init_schema(&self.graph).await
    }

    async fn write_blocks(&self, blocks: &[BlockData]) -> Result<()> {
        self.execute_batched(
            blocks,
            queries::CREATE_BLOCKS_QUERY,
            "blocks",
            "write_blocks",
            blocks_to_bolt_list,
        )
        .await
    }

    async fn write_transactions(&self, transactions: &[TransactionData]) -> Result<()> {
        self.execute_batched(
            transactions,
            queries::CREATE_TRANSACTIONS_QUERY,
            "transactions",
            "write_transactions",
            transactions_to_bolt_list,
        )
        .await
    }

    async fn write_outputs(&self, outputs: &[OutputData]) -> Result<()> {
        if outputs.is_empty() {
            return Ok(());
        }

        let total_batches = outputs.len().div_ceil(self.batch_size);

        // write_outputs needs special handling: two queries per chunk (outputs + LOCKED_TO)
        for (i, chunk) in outputs.chunks(self.batch_size).enumerate() {
            let output_data = outputs_to_bolt_list(chunk);
            let batch_num = i + 1;

            if i > 0 {
                tracing::debug!(
                    operation = "write_outputs",
                    batch = batch_num,
                    total_batches,
                    records = chunk.len(),
                    "Writing batch"
                );
            }

            // Query 1: Create output nodes
            let start = std::time::Instant::now();

            self.run_with_retry(
                "write_outputs",
                || {
                    let q = query(queries::CREATE_OUTPUTS_QUERY)
                        .param("outputs", output_data.as_slice());
                    async { self.graph.run(q).await }
                },
                batch_num,
                total_batches,
                chunk.len(),
            )
            .await?;

            tracing::debug!(
                operation = "write_outputs",
                batch = batch_num,
                records = chunk.len(),
                elapsed_ms = start.elapsed().as_millis() as u64,
                "Output nodes written"
            );

            // Query 2: Create LOCKED_TO relationships for outputs with addresses
            let outputs_with_address = filter_outputs_with_address(chunk);

            if !outputs_with_address.is_empty() {
                let address_data = output_refs_to_bolt_list(&outputs_with_address);
                let addr_count = outputs_with_address.len();
                let start = std::time::Instant::now();

                self.run_with_retry(
                    "write_outputs:locked_to",
                    || {
                        let q = query(queries::CREATE_LOCKED_TO_QUERY)
                            .param("outputs", address_data.as_slice());
                        async { self.graph.run(q).await }
                    },
                    batch_num,
                    total_batches,
                    addr_count,
                )
                .await?;

                tracing::debug!(
                    operation = "write_outputs:locked_to",
                    batch = batch_num,
                    records = addr_count,
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    "LOCKED_TO relationships written"
                );
            }
        }

        Ok(())
    }

    async fn write_has_output_relationships(&self, outputs: &[OutputData]) -> Result<()> {
        self.execute_batched(
            outputs,
            queries::CREATE_HAS_OUTPUT_QUERY,
            "outputs",
            "write_has_output_relationships",
            outputs_to_bolt_list,
        )
        .await
    }

    async fn write_inputs(&self, inputs: &[InputData]) -> Result<()> {
        self.execute_batched(
            inputs,
            queries::CREATE_INPUTS_QUERY,
            "inputs",
            "write_inputs",
            inputs_to_bolt_list,
        )
        .await
    }

    async fn write_performs(&self, performs: &[PerformsData]) -> Result<()> {
        self.execute_batched(
            performs,
            queries::CREATE_PERFORMS_BULK_QUERY,
            "performs",
            "write_performs",
            performs_to_bolt_list,
        )
        .await
    }

    async fn write_benefits_to(&self, benefits_to: &[BenefitsToData]) -> Result<()> {
        self.execute_batched(
            benefits_to,
            queries::CREATE_BENEFITS_TO_BULK_QUERY,
            "benefitsTo",
            "write_benefits_to",
            benefits_to_to_bolt_list,
        )
        .await
    }

    async fn lookup_outputs_batch(&self, output_ids: &[String]) -> Result<Vec<OutputData>> {
        if output_ids.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<&str> = output_ids.iter().map(|s| s.as_str()).collect();
        let mut result = self
            .graph
            .execute(query(queries::LOOKUP_OUTPUTS_BATCH_QUERY).param("outputIds", ids))
            .await
            .map_err(|e| {
                WriterError::QueryFailed(format!(
                    "lookup_outputs_batch failed ({} ids): {}",
                    output_ids.len(),
                    e
                ))
            })?;

        let mut outputs = Vec::with_capacity(output_ids.len());
        while let Some(row) = result
            .next()
            .await
            .map_err(|e| WriterError::QueryFailed(format!("Failed to fetch row: {}", e)))?
        {
            let output_id: String = row
                .get("outputId")
                .map_err(|e| WriterError::DatabaseError(format!("Missing outputId: {}", e)))?;
            outputs.push(OutputData {
                output_id: output_id.clone(),
                output_index: row.get("outputIndex").map_err(|e| {
                    WriterError::DatabaseError(format!("Missing outputIndex: {}", e))
                })?,
                txid: output_id.split(':').next().unwrap_or("").to_string(),
                amount: row
                    .get("amount")
                    .map_err(|e| WriterError::DatabaseError(format!("Missing amount: {}", e)))?,
                script_pubkey: row.get("scriptPubKey").map_err(|e| {
                    WriterError::DatabaseError(format!("Missing scriptPubKey: {}", e))
                })?,
                script_type: row.get("scriptType").map_err(|e| {
                    WriterError::DatabaseError(format!("Missing scriptType: {}", e))
                })?,
                address: row.get("address").ok(),
            });
        }

        Ok(outputs)
    }

    async fn lookup_output(&self, output_id: &str) -> Result<OutputData> {
        let mut result = self
            .graph
            .execute(query(queries::LOOKUP_OUTPUT_QUERY).param("outputId", output_id))
            .await
            .map_err(|e| {
                WriterError::QueryFailed(format!("lookup_output failed ({}): {}", output_id, e))
            })?;

        let row = result
            .next()
            .await
            .map_err(|e| WriterError::QueryFailed(format!("Failed to fetch row: {}", e)))?
            .ok_or_else(|| WriterError::OutputNotFound(output_id.to_string()))?;

        Ok(OutputData {
            output_id: row
                .get("outputId")
                .map_err(|e| WriterError::DatabaseError(format!("Missing outputId: {}", e)))?,
            output_index: row
                .get("outputIndex")
                .map_err(|e| WriterError::DatabaseError(format!("Missing outputIndex: {}", e)))?,
            txid: output_id.split(':').next().unwrap_or("").to_string(),
            amount: row
                .get("amount")
                .map_err(|e| WriterError::DatabaseError(format!("Missing amount: {}", e)))?,
            script_pubkey: row
                .get("scriptPubKey")
                .map_err(|e| WriterError::DatabaseError(format!("Missing scriptPubKey: {}", e)))?,
            script_type: row
                .get("scriptType")
                .map_err(|e| WriterError::DatabaseError(format!("Missing scriptType: {}", e)))?,
            address: row.get("address").ok(),
        })
    }

    async fn mark_output_spent(
        &self,
        output_id: &str,
        spent_in_txid: &str,
        spent_at_height: u32,
    ) -> Result<()> {
        self.graph
            .run(
                query(queries::MARK_OUTPUT_SPENT_QUERY)
                    .param("outputId", output_id)
                    .param("spentInTxid", spent_in_txid)
                    .param("spentAtHeight", spent_at_height),
            )
            .await
            .map_err(|e| WriterError::QueryFailed(format!("mark_output_spent failed: {}", e)))?;

        Ok(())
    }

    async fn create_checkpoint(&self) -> Result<()> {
        // Step 1: Delete any existing checkpoints
        self.graph
            .run(query(queries::DELETE_CHECKPOINT_QUERY))
            .await
            .map_err(|e| WriterError::CheckpointError(format!("delete failed: {}", e)))?;

        // Step 2: Create new checkpoint at sentinel height (initial state, "not yet started")
        self.graph
            .run(query(queries::CREATE_CHECKPOINT_QUERY))
            .await
            .map_err(|e| WriterError::CheckpointError(format!("create failed: {}", e)))?;

        Ok(())
    }

    async fn update_checkpoint(&self, checkpoint: &CheckpointData) -> Result<()> {
        self.graph
            .run(
                query(queries::UPDATE_CHECKPOINT_QUERY)
                    .param("height", checkpoint.last_processed_height)
                    .param("hash", checkpoint.last_processed_hash.clone())
                    .param("file", checkpoint.last_processed_file.clone())
                    .param(
                        "offset",
                        checkpoint.last_processed_file_offset.unwrap_or(0) as i64,
                    )
                    .param("status", checkpoint.status.clone()),
            )
            .await
            .map_err(|e| {
                WriterError::CheckpointError(format!("update_checkpoint failed: {}", e))
            })?;

        Ok(())
    }

    async fn get_checkpoint(&self) -> Result<Option<CheckpointData>> {
        let mut result = self
            .graph
            .execute(query(queries::GET_CHECKPOINT_QUERY))
            .await
            .map_err(|e| WriterError::CheckpointError(format!("get_checkpoint failed: {}", e)))?;

        if let Some(row) = result.next().await.map_err(|e| {
            WriterError::CheckpointError(format!("Failed to fetch checkpoint: {}", e))
        })? {
            Ok(Some(CheckpointData {
                last_processed_height: {
                    let val: i64 = row.get("lastProcessedHeight").map_err(|e| {
                        WriterError::DatabaseError(format!("Missing lastProcessedHeight: {}", e))
                    })?;

                    // Convert sentinel value back to -1 for domain layer.
                    // We store CHECKPOINT_INITIAL_HEIGHT (-999) in Neo4j to avoid a neo4rs
                    // driver bug that misreads -1 as 255. Also handle legacy -1/255 values
                    // for databases created before this fix.
                    if val == CHECKPOINT_INITIAL_HEIGHT || val == 255 || val == -1 {
                        -1
                    } else {
                        val as i32
                    }
                },
                last_processed_hash: row.get("lastProcessedHash").map_err(|e| {
                    WriterError::DatabaseError(format!("Missing lastProcessedHash: {}", e))
                })?,
                last_processed_file: row.get("lastProcessedFile").map_err(|e| {
                    WriterError::DatabaseError(format!("Missing lastProcessedFile: {}", e))
                })?,
                last_processed_file_offset: row.get("lastProcessedFileOffset").ok(),
                timestamp: chrono::Utc::now().timestamp(),
                status: row
                    .get("status")
                    .map_err(|e| WriterError::DatabaseError(format!("Missing status: {}", e)))?,
            }))
        } else {
            Ok(None)
        }
    }

    async fn mark_checkpoint_complete(&self) -> Result<()> {
        self.graph
            .run(query(queries::MARK_CHECKPOINT_COMPLETE_QUERY))
            .await
            .map_err(|e| {
                WriterError::CheckpointError(format!("mark_checkpoint_complete failed: {}", e))
            })?;

        Ok(())
    }

    async fn set_checkpoint_status(&self, status: &str) -> Result<()> {
        self.graph
            .run(query(queries::SET_CHECKPOINT_STATUS_QUERY).param("status", status))
            .await
            .map_err(|e| {
                WriterError::CheckpointError(format!("set_checkpoint_status failed: {}", e))
            })?;

        Ok(())
    }

    async fn lookup_block_hash(&self, height: u32) -> Result<Option<String>> {
        let mut result = self
            .graph
            .execute(query(queries::LOOKUP_BLOCK_HASH_QUERY).param("height", height))
            .await
            .map_err(|e| {
                WriterError::QueryFailed(format!("lookup_block_hash failed (height {}): {}", height, e))
            })?;

        if let Some(row) = result.next().await.map_err(|e| {
            WriterError::QueryFailed(format!("Failed to fetch block hash row: {}", e))
        })? {
            let hash: String = row
                .get("hash")
                .map_err(|e| WriterError::DatabaseError(format!("Missing hash field: {}", e)))?;
            Ok(Some(hash))
        } else {
            Ok(None)
        }
    }

    async fn rollback_block(&self, height: u32) -> Result<()> {
        tracing::warn!(height, "Rolling back block");

        // Step 1: Revert spent status on outputs spent by this block's transactions
        self.graph
            .run(query(queries::ROLLBACK_REVERT_SPENT_QUERY).param("height", height))
            .await
            .map_err(|e| {
                WriterError::QueryFailed(format!(
                    "rollback_block step 1 (revert spent) failed at height {}: {}",
                    height, e
                ))
            })?;

        // Step 2: Delete all Input nodes (DETACH removes HAS_INPUT and SPENDS)
        self.graph
            .run(query(queries::ROLLBACK_DELETE_INPUTS_QUERY).param("height", height))
            .await
            .map_err(|e| {
                WriterError::QueryFailed(format!(
                    "rollback_block step 2 (delete inputs) failed at height {}: {}",
                    height, e
                ))
            })?;

        // Step 3: Delete all Output nodes (DETACH removes HAS_OUTPUT and LOCKED_TO)
        self.graph
            .run(query(queries::ROLLBACK_DELETE_OUTPUTS_QUERY).param("height", height))
            .await
            .map_err(|e| {
                WriterError::QueryFailed(format!(
                    "rollback_block step 3 (delete outputs) failed at height {}: {}",
                    height, e
                ))
            })?;

        // Step 4: Delete all Transaction nodes (DETACH removes INCLUDED_IN, PERFORMS, BENEFITS_TO)
        self.graph
            .run(query(queries::ROLLBACK_DELETE_TRANSACTIONS_QUERY).param("height", height))
            .await
            .map_err(|e| {
                WriterError::QueryFailed(format!(
                    "rollback_block step 4 (delete transactions) failed at height {}: {}",
                    height, e
                ))
            })?;

        // Step 5: Delete the Block node (DETACH removes NEXT_BLOCK)
        self.graph
            .run(query(queries::ROLLBACK_DELETE_BLOCK_QUERY).param("height", height))
            .await
            .map_err(|e| {
                WriterError::QueryFailed(format!(
                    "rollback_block step 5 (delete block) failed at height {}: {}",
                    height, e
                ))
            })?;

        tracing::warn!(height, "Block rolled back successfully");
        Ok(())
    }
}
