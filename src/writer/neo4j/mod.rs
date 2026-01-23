//! Neo4j implementation of GraphWriter trait
//!
//! Provides Neo4j-backed blockchain ingestion with connection pooling,
//! bulk operations, and configuration-driven performance tuning.

use async_trait::async_trait;
use neo4rs::{Graph, ConfigBuilder, query};
use std::sync::Arc;

use crate::config::Neo4jConfig;
use crate::domain::{BlockData, TransactionData, OutputData, InputData, CheckpointData};
use crate::writer::{GraphWriter, WriterError, Result};

mod queries;
mod conversions;
mod schema;

use conversions::*;

/// Neo4j implementation of GraphWriter
///
/// Connects to Neo4j database and implements all blockchain ingestion operations
/// with bulk writes, connection pooling, and configurable performance settings.
pub struct Neo4jWriter {
    graph: Arc<Graph>,
}

impl Neo4jWriter {
    /// Create a new Neo4jWriter with configuration
    ///
    /// # Arguments
    /// * `config` - Neo4j connection and pool configuration
    ///
    /// # Errors
    /// Returns error if connection to Neo4j fails
    pub async fn new(config: Neo4jConfig) -> Result<Self> {
        let graph = Self::connect(&config).await?;

        Ok(Self {
            graph: Arc::new(graph),
        })
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

    /// Get reference to underlying graph
    pub fn graph(&self) -> &Graph {
        &self.graph
    }
}

#[async_trait]
impl GraphWriter for Neo4jWriter {
    async fn init_schema(&self) -> Result<()> {
        schema::init_schema(&self.graph).await
    }

    async fn write_blocks(&self, blocks: &[BlockData]) -> Result<()> {
        let blocks_data = blocks_to_bolt_list(blocks)?;

        self.graph
            .run(query(queries::CREATE_BLOCKS_QUERY)
                .param("blocks", blocks_data.as_slice()))
            .await
            .map_err(|e| WriterError::QueryFailed(format!("write_blocks failed: {}", e)))?;

        Ok(())
    }

    async fn write_transactions(&self, transactions: &[TransactionData]) -> Result<()> {
        let tx_data = transactions_to_bolt_list(transactions)?;

        self.graph
            .run(query(queries::CREATE_TRANSACTIONS_QUERY)
                .param("transactions", tx_data.as_slice()))
            .await
            .map_err(|e| WriterError::QueryFailed(format!("write_transactions failed: {}", e)))?;

        Ok(())
    }

    async fn write_outputs(&self, outputs: &[OutputData]) -> Result<()> {
        // Create output nodes
        let output_data = outputs_to_bolt_list(outputs)?;

        self.graph
            .run(query(queries::CREATE_OUTPUTS_QUERY)
                .param("outputs", output_data.as_slice()))
            .await
            .map_err(|e| WriterError::QueryFailed(format!("write_outputs failed: {}", e)))?;

        // Create LOCKED_TO relationships for outputs with addresses
        let outputs_with_address = filter_outputs_with_address(outputs);

        if !outputs_with_address.is_empty() {
            let owned_outputs: Vec<OutputData> = outputs_with_address.into_iter().cloned().collect();
            let address_data = outputs_to_bolt_list(&owned_outputs)?;

            self.graph
                .run(query(queries::CREATE_LOCKED_TO_QUERY)
                    .param("outputs", address_data.as_slice()))
                .await
                .map_err(|e| WriterError::QueryFailed(format!("CREATE_LOCKED_TO failed: {}", e)))?;
        }

        Ok(())
    }

    async fn write_inputs(&self, inputs: &[InputData]) -> Result<()> {
        // Infer block height from first input (all inputs in batch are from same block)
        let block_height = if !inputs.is_empty() {
            // Parse block height from transaction ID lookup
            // For now, we'll need to pass block_height separately
            // This is a simplification - in production we'd track this better
            0 // TODO: Fix this - need block height context
        } else {
            return Ok(()); // No inputs to process
        };

        let input_data = inputs_to_bolt_list(inputs, block_height)?;

        self.graph
            .run(query(queries::CREATE_INPUTS_QUERY)
                .param("inputs", input_data.as_slice()))
            .await
            .map_err(|e| WriterError::QueryFailed(format!("write_inputs failed: {}", e)))?;

        Ok(())
    }

    async fn calculate_amounts(&self, transactions: &[TransactionData]) -> Result<()> {
        let txids: Vec<String> = transactions.iter()
            .map(|tx| tx.txid.clone())
            .collect();

        self.graph
            .run(query(queries::CALCULATE_AMOUNTS_QUERY)
                .param("txids", txids.as_slice()))
            .await
            .map_err(|e| WriterError::QueryFailed(format!("calculate_amounts failed: {}", e)))?;

        Ok(())
    }

    async fn write_simplified_layer(&self, transactions: &[TransactionData]) -> Result<()> {
        let txids: Vec<String> = transactions.iter()
            .map(|tx| tx.txid.clone())
            .collect();

        // Create PERFORMS relationships
        self.graph
            .run(query(queries::CREATE_PERFORMS_QUERY)
                .param("txids", txids.as_slice()))
            .await
            .map_err(|e| WriterError::QueryFailed(format!("CREATE_PERFORMS failed: {}", e)))?;

        // Create BENEFITS_TO relationships
        self.graph
            .run(query(queries::CREATE_BENEFITS_TO_QUERY)
                .param("txids", txids.as_slice()))
            .await
            .map_err(|e| WriterError::QueryFailed(format!("CREATE_BENEFITS_TO failed: {}", e)))?;

        Ok(())
    }

    async fn lookup_output(&self, output_id: &str) -> Result<OutputData> {
        let mut result = self.graph
            .execute(query(queries::LOOKUP_OUTPUT_QUERY)
                .param("outputId", output_id))
            .await
            .map_err(|e| WriterError::QueryFailed(format!("lookup_output failed: {}", e)))?;

        let row = result.next().await
            .map_err(|e| WriterError::QueryFailed(format!("Failed to fetch row: {}", e)))?
            .ok_or_else(|| WriterError::OutputNotFound(output_id.to_string()))?;

        Ok(OutputData {
            output_id: row.get("outputId")
                .map_err(|e| WriterError::DatabaseError(format!("Missing outputId: {}", e)))?,
            output_index: row.get("outputIndex")
                .map_err(|e| WriterError::DatabaseError(format!("Missing outputIndex: {}", e)))?,
            txid: output_id.split(':').next().unwrap_or("").to_string(),
            amount: row.get("amount")
                .map_err(|e| WriterError::DatabaseError(format!("Missing amount: {}", e)))?,
            script_pubkey: row.get("scriptPubKey")
                .map_err(|e| WriterError::DatabaseError(format!("Missing scriptPubKey: {}", e)))?,
            script_type: row.get("scriptType")
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
            .run(query(queries::MARK_OUTPUT_SPENT_QUERY)
                .param("outputId", output_id)
                .param("spentInTxid", spent_in_txid)
                .param("spentAtHeight", spent_at_height))
            .await
            .map_err(|e| WriterError::QueryFailed(format!("mark_output_spent failed: {}", e)))?;

        Ok(())
    }

    async fn create_checkpoint(&self) -> Result<()> {
        self.graph
            .run(query(queries::CREATE_CHECKPOINT_QUERY))
            .await
            .map_err(|e| WriterError::CheckpointError(format!("create_checkpoint failed: {}", e)))?;

        Ok(())
    }

    async fn update_checkpoint(&self, checkpoint: &CheckpointData) -> Result<()> {
        self.graph
            .run(query(queries::UPDATE_CHECKPOINT_QUERY)
                .param("height", checkpoint.last_processed_height)
                .param("hash", checkpoint.last_processed_hash.clone())
                .param("file", checkpoint.last_processed_file.clone())
                .param("offset", checkpoint.last_processed_file_offset.unwrap_or(0) as i64)
                .param("status", checkpoint.status.clone()))
            .await
            .map_err(|e| WriterError::CheckpointError(format!("update_checkpoint failed: {}", e)))?;

        Ok(())
    }

    async fn get_checkpoint(&self) -> Result<Option<CheckpointData>> {
        let mut result = self.graph
            .execute(query(queries::GET_CHECKPOINT_QUERY))
            .await
            .map_err(|e| WriterError::CheckpointError(format!("get_checkpoint failed: {}", e)))?;

        if let Some(row) = result.next().await
            .map_err(|e| WriterError::CheckpointError(format!("Failed to fetch checkpoint: {}", e)))? {

            Ok(Some(CheckpointData {
                last_processed_height: row.get("lastProcessedHeight")
                    .map_err(|e| WriterError::DatabaseError(format!("Missing lastProcessedHeight: {}", e)))?,
                last_processed_hash: row.get("lastProcessedHash")
                    .map_err(|e| WriterError::DatabaseError(format!("Missing lastProcessedHash: {}", e)))?,
                last_processed_file: row.get("lastProcessedFile")
                    .map_err(|e| WriterError::DatabaseError(format!("Missing lastProcessedFile: {}", e)))?,
                last_processed_file_offset: row.get("lastProcessedFileOffset").ok(),
                timestamp: chrono::Utc::now().timestamp(),
                status: row.get("status")
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
            .map_err(|e| WriterError::CheckpointError(format!("mark_checkpoint_complete failed: {}", e)))?;

        Ok(())
    }
}
