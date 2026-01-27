//! Mock implementation of GraphWriter for testing
//!
//! Provides an in-memory implementation of the GraphWriter trait that stores
//! all data in memory. Useful for:
//! - Unit testing domain logic without a database
//! - Fast integration tests
//! - Development without Neo4j running
//!
//! All operations succeed and data is stored in Vecs.

use super::error::{Result, WriterError};
use super::traits::GraphWriter;
use crate::domain::{
    BenefitsToData, BlockData, CheckpointData, InputData, OutputData, PerformsData, TransactionData,
};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// In-memory storage for mock writer
#[derive(Debug, Default)]
struct MockStorage {
    blocks: Vec<BlockData>,
    transactions: Vec<TransactionData>,
    outputs: Vec<OutputData>,
    inputs: Vec<InputData>,
    performs: Vec<PerformsData>,
    benefits_to: Vec<BenefitsToData>,
    checkpoint: Option<CheckpointData>,
    schema_initialized: bool,
}

/// Mock implementation of GraphWriter for testing
///
/// Stores all data in memory using Vec collections. Thread-safe using Arc<Mutex>.
///
/// # Example
/// ```
/// use bitcoin_chain_graph::writer::{GraphWriter, MockWriter};
/// use bitcoin_chain_graph::domain::BlockData;
///
/// #[tokio::main]
/// async fn main() {
///     let writer = MockWriter::new();
///
///     // Initialize schema
///     writer.init_schema().await.unwrap();
///
///     // Write data
///     let blocks = vec![/* BlockData */];
///     writer.write_blocks(&blocks).await.unwrap();
///
///     // Retrieve data
///     let stored_blocks = writer.get_blocks().await;
///     assert_eq!(stored_blocks.len(), blocks.len());
/// }
/// ```
#[derive(Clone, Debug)]
pub struct MockWriter {
    storage: Arc<Mutex<MockStorage>>,
}

impl MockWriter {
    /// Create a new MockWriter with empty storage
    pub fn new() -> Self {
        Self {
            storage: Arc::new(Mutex::new(MockStorage::default())),
        }
    }

    // Test accessors - retrieve stored data

    /// Get all stored blocks
    pub async fn get_blocks(&self) -> Vec<BlockData> {
        let storage = self.storage.lock().unwrap();
        storage.blocks.clone()
    }

    /// Get all stored transactions
    pub async fn get_transactions(&self) -> Vec<TransactionData> {
        let storage = self.storage.lock().unwrap();
        storage.transactions.clone()
    }

    /// Get all stored outputs
    pub async fn get_outputs(&self) -> Vec<OutputData> {
        let storage = self.storage.lock().unwrap();
        storage.outputs.clone()
    }

    /// Get all stored inputs
    pub async fn get_inputs(&self) -> Vec<InputData> {
        let storage = self.storage.lock().unwrap();
        storage.inputs.clone()
    }

    /// Get all stored PERFORMS relationships
    pub async fn get_performs(&self) -> Vec<PerformsData> {
        let storage = self.storage.lock().unwrap();
        storage.performs.clone()
    }

    /// Get all stored BENEFITS_TO relationships
    pub async fn get_benefits_to(&self) -> Vec<BenefitsToData> {
        let storage = self.storage.lock().unwrap();
        storage.benefits_to.clone()
    }

    /// Get stored checkpoint
    pub async fn get_stored_checkpoint(&self) -> Option<CheckpointData> {
        let storage = self.storage.lock().unwrap();
        storage.checkpoint.clone()
    }

    /// Check if schema was initialized
    pub async fn is_schema_initialized(&self) -> bool {
        let storage = self.storage.lock().unwrap();
        storage.schema_initialized
    }

    /// Clear all stored data (useful for test cleanup)
    pub async fn clear(&self) {
        let mut storage = self.storage.lock().unwrap();
        storage.blocks.clear();
        storage.transactions.clear();
        storage.outputs.clear();
        storage.inputs.clear();
        storage.performs.clear();
        storage.benefits_to.clear();
        storage.checkpoint = None;
        storage.schema_initialized = false;
    }
}

impl Default for MockWriter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GraphWriter for MockWriter {
    async fn init_schema(&self) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        storage.schema_initialized = true;
        Ok(())
    }

    async fn write_blocks(&self, blocks: &[BlockData]) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        storage.blocks.extend_from_slice(blocks);
        Ok(())
    }

    async fn write_transactions(&self, transactions: &[TransactionData]) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        storage.transactions.extend_from_slice(transactions);
        Ok(())
    }

    async fn write_outputs(&self, outputs: &[OutputData]) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        storage.outputs.extend_from_slice(outputs);
        Ok(())
    }

    async fn write_inputs(&self, inputs: &[InputData]) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        storage.inputs.extend_from_slice(inputs);

        // Update spent status on referenced outputs
        for input in inputs {
            // Skip coinbase inputs (previous_output_index = 0xFFFFFFFF)
            if input.previous_output_index == 0xFFFFFFFF {
                continue;
            }

            // Find and mark the spent output
            let output_id = format!("{}:{}", input.previous_txid, input.previous_output_index);
            if let Some(_output) = storage
                .outputs
                .iter_mut()
                .find(|o| o.output_id == output_id)
            {
                // Note: In a full implementation, we'd update isSpent, spentInTxid, spentAtHeight
                // These fields aren't in our current OutputData model but would be added later
                // For now, just verify the output exists
            }
        }

        Ok(())
    }

    async fn write_performs(&self, performs: &[PerformsData]) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        storage.performs.extend_from_slice(performs);
        Ok(())
    }

    async fn write_benefits_to(&self, benefits_to: &[BenefitsToData]) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        storage.benefits_to.extend_from_slice(benefits_to);
        Ok(())
    }

    async fn lookup_outputs_batch(&self, output_ids: &[String]) -> Result<Vec<OutputData>> {
        let storage = self.storage.lock().unwrap();
        let mut results = Vec::with_capacity(output_ids.len());
        for id in output_ids {
            if let Some(output) = storage.outputs.iter().find(|o| o.output_id == *id) {
                results.push(output.clone());
            }
            // Silently skip outputs not found (matches Neo4j MATCH behavior)
        }
        Ok(results)
    }

    async fn lookup_output(&self, output_id: &str) -> Result<OutputData> {
        let storage = self.storage.lock().unwrap();
        storage
            .outputs
            .iter()
            .find(|o| o.output_id == output_id)
            .cloned()
            .ok_or_else(|| WriterError::OutputNotFound(output_id.to_string()))
    }

    async fn mark_output_spent(
        &self,
        output_id: &str,
        _spent_in_txid: &str,
        _spent_at_height: u32,
    ) -> Result<()> {
        let storage = self.storage.lock().unwrap();

        // Verify output exists
        if !storage.outputs.iter().any(|o| o.output_id == output_id) {
            return Err(WriterError::OutputNotFound(output_id.to_string()));
        }

        // In real implementation, would update the output with spent metadata
        // Our current OutputData model doesn't have isSpent/spentInTxid/spentAtHeight fields
        // These would be added in later milestones
        Ok(())
    }

    async fn create_checkpoint(&self) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();

        // Create initial checkpoint at height 0 (no blocks processed yet)
        // This matches the Neo4jWriter behavior for consistency
        let checkpoint = CheckpointData {
            last_processed_height: 0,
            last_processed_hash: String::from(
                "0000000000000000000000000000000000000000000000000000000000000000",
            ),
            last_processed_file: String::from("blk00000.dat"),
            last_processed_file_offset: Some(0),
            timestamp: chrono::Utc::now().timestamp(),
            status: String::from("in_progress"),
        };

        storage.checkpoint = Some(checkpoint);
        Ok(())
    }

    async fn update_checkpoint(&self, checkpoint: &CheckpointData) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        storage.checkpoint = Some(checkpoint.clone());
        Ok(())
    }

    async fn get_checkpoint(&self) -> Result<Option<CheckpointData>> {
        let storage = self.storage.lock().unwrap();
        Ok(storage.checkpoint.clone())
    }

    async fn mark_checkpoint_complete(&self) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();

        if let Some(ref mut checkpoint) = storage.checkpoint {
            checkpoint.status = String::from("completed");
        }

        Ok(())
    }

    async fn set_checkpoint_status(&self, status: &str) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();

        if let Some(ref mut checkpoint) = storage.checkpoint {
            checkpoint.status = status.to_string();
            checkpoint.timestamp = chrono::Utc::now().timestamp();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_writer_creation() {
        let writer = MockWriter::new();
        assert!(!writer.is_schema_initialized().await);
    }

    #[tokio::test]
    async fn test_init_schema() {
        let writer = MockWriter::new();
        writer.init_schema().await.unwrap();
        assert!(writer.is_schema_initialized().await);
    }

    #[tokio::test]
    async fn test_write_and_retrieve_blocks() {
        let writer = MockWriter::new();

        let blocks = vec![BlockData {
            height: 0,
            hash: String::from("genesis"),
            previous_hash: String::from("0000"),
            merkle_root: String::from("merkle"),
            timestamp: 1231006505,
            bits: String::from("1d00ffff"),
            difficulty: 1.0,
            nonce: 2083236893,
            version: 1,
            tx_count: 1,
            size: 285,
            weight: 1140,
        }];

        writer.write_blocks(&blocks).await.unwrap();

        let stored = writer.get_blocks().await;
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].hash, "genesis");
    }

    #[tokio::test]
    async fn test_checkpoint_lifecycle() {
        let writer = MockWriter::new();

        // Create initial checkpoint
        writer.create_checkpoint().await.unwrap();

        let checkpoint = writer.get_checkpoint().await.unwrap().unwrap();
        assert_eq!(checkpoint.status, "in_progress");
        assert_eq!(checkpoint.last_processed_height, 0);

        // Update checkpoint
        let updated = CheckpointData {
            last_processed_height: 100,
            last_processed_hash: String::from("block100"),
            last_processed_file: String::from("blk00000.dat"),
            last_processed_file_offset: Some(12345),
            timestamp: chrono::Utc::now().timestamp(),
            status: String::from("in_progress"),
        };

        writer.update_checkpoint(&updated).await.unwrap();

        let checkpoint = writer.get_checkpoint().await.unwrap().unwrap();
        assert_eq!(checkpoint.last_processed_height, 100);
        assert_eq!(checkpoint.last_processed_hash, "block100");

        // Mark complete
        writer.mark_checkpoint_complete().await.unwrap();

        let checkpoint = writer.get_checkpoint().await.unwrap().unwrap();
        assert_eq!(checkpoint.status, "completed");
    }

    #[tokio::test]
    async fn test_lookup_output() {
        let writer = MockWriter::new();

        let output = OutputData {
            output_id: String::from("tx1:0"),
            output_index: 0,
            txid: String::from("tx1"),
            amount: 5000000000,
            script_pubkey: String::from("76a914...88ac"),
            script_type: String::from("P2PKH"),
            address: Some(String::from("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa")),
        };

        writer.write_outputs(&[output.clone()]).await.unwrap();

        // Lookup existing output
        let found = writer.lookup_output("tx1:0").await.unwrap();
        assert_eq!(found.output_id, "tx1:0");
        assert_eq!(found.amount, 5000000000);

        // Lookup non-existent output
        let result = writer.lookup_output("tx2:0").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            WriterError::OutputNotFound(_)
        ));
    }
}
