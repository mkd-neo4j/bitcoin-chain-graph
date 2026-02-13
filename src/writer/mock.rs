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
    BenefitsToData, BlockData, CheckpointData, InputData, OutputData, OutputLookupResult,
    PerformsData, TransactionData,
};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// Snapshot of MockStorage state at transaction begin, used for rollback
#[derive(Debug, Clone)]
struct MockSnapshot {
    blocks_len: usize,
    transactions_len: usize,
    outputs_len: usize,
    inputs_len: usize,
    performs_len: usize,
    benefits_to_len: usize,
    checkpoint: Option<CheckpointData>,
}

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
    transaction_commit_count: u32,
    in_transaction: bool,
    checkpoint_written_in_txn: bool,
    /// Snapshot saved at begin_transaction for rollback support
    snapshot: Option<MockSnapshot>,
    /// Configured failures: method_name -> error
    failures: std::collections::HashMap<String, Option<WriterError>>,
    /// Transient failures: method_name -> (error, remaining_failures)
    transient_failures: std::collections::HashMap<String, (WriterError, u32)>,
    /// Failures after N calls: method_name -> (succeed_count, error)
    failure_after_n: std::collections::HashMap<String, (u32, u32, WriterError)>,
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

    /// Get the number of committed transactions
    pub async fn transaction_commit_count(&self) -> u32 {
        let storage = self.storage.lock().unwrap();
        storage.transaction_commit_count
    }

    /// Get the number of committed transactions (alias for `transaction_commit_count`)
    pub async fn get_commit_count(&self) -> u32 {
        self.transaction_commit_count().await
    }

    /// Check if the checkpoint was written inside a transaction
    pub async fn checkpoint_written_in_transaction(&self) -> bool {
        let storage = self.storage.lock().unwrap();
        storage.checkpoint_written_in_txn
    }

    /// Configure a permanent failure on a specific method
    pub async fn set_failure_on(&self, method: &str, error: WriterError) {
        let mut storage = self.storage.lock().unwrap();
        storage.failures.insert(method.to_string(), Some(error));
    }

    /// Configure a transient failure that occurs N times then succeeds
    pub async fn set_transient_failure_on(&self, method: &str, error: WriterError, times: u32) {
        let mut storage = self.storage.lock().unwrap();
        storage
            .transient_failures
            .insert(method.to_string(), (error, times));
    }

    /// Configure a failure that triggers after N successful calls
    pub async fn set_failure_after_n_calls(&self, method: &str, n: u32, error: WriterError) {
        let mut storage = self.storage.lock().unwrap();
        storage
            .failure_after_n
            .insert(method.to_string(), (n, 0, error));
    }

    /// Clear all configured failures
    pub async fn clear_failures(&self) {
        let mut storage = self.storage.lock().unwrap();
        storage.failures.clear();
        storage.transient_failures.clear();
        storage.failure_after_n.clear();
    }

    /// Check if a method should fail, and return the error if so
    fn check_failure(storage: &mut MockStorage, method: &str) -> Option<WriterError> {
        // Check permanent failures
        if let Some(Some(err)) = storage.failures.get(method) {
            return Some(err.clone());
        }

        // Check transient failures
        if let Some((err, remaining)) = storage.transient_failures.get_mut(method) {
            if *remaining > 0 {
                *remaining -= 1;
                return Some(err.clone());
            }
        }

        // Check failure-after-N-calls
        if let Some((threshold, count, err)) = storage.failure_after_n.get_mut(method) {
            *count += 1;
            if *count > *threshold {
                return Some(err.clone());
            }
        }

        None
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
        if let Some(err) = Self::check_failure(&mut storage, "write_blocks") {
            return Err(err);
        }
        storage.blocks.extend_from_slice(blocks);
        Ok(())
    }

    async fn write_transactions(&self, transactions: &[TransactionData]) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        if let Some(err) = Self::check_failure(&mut storage, "write_transactions") {
            return Err(err);
        }
        storage.transactions.extend_from_slice(transactions);
        Ok(())
    }

    async fn write_outputs(&self, outputs: &[OutputData]) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        if let Some(err) = Self::check_failure(&mut storage, "write_outputs") {
            return Err(err);
        }
        storage.outputs.extend_from_slice(outputs);
        Ok(())
    }

    async fn write_has_output_relationships(&self, _outputs: &[OutputData]) -> Result<()> {
        // MockWriter doesn't track relationships separately.
        // The HAS_OUTPUT relationship is implicit from the output's txid field.
        Ok(())
    }

    async fn write_inputs(&self, inputs: &[InputData]) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        if let Some(err) = Self::check_failure(&mut storage, "write_inputs") {
            return Err(err);
        }
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
        if let Some(err) = Self::check_failure(&mut storage, "write_performs") {
            return Err(err);
        }
        storage.performs.extend_from_slice(performs);
        Ok(())
    }

    async fn write_benefits_to(&self, benefits_to: &[BenefitsToData]) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        if let Some(err) = Self::check_failure(&mut storage, "write_benefits_to") {
            return Err(err);
        }
        storage.benefits_to.extend_from_slice(benefits_to);
        Ok(())
    }

    async fn lookup_outputs_batch(
        &self,
        _output_ids: &[String],
    ) -> Result<Vec<OutputLookupResult>> {
        let mut storage = self.storage.lock().unwrap();
        if let Some(err) = Self::check_failure(&mut storage, "lookup_outputs_batch") {
            return Err(err);
        }
        Ok(vec![])
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
        if let Some(err) = Self::check_failure(&mut storage, "update_checkpoint") {
            return Err(err);
        }
        if storage.in_transaction {
            storage.checkpoint_written_in_txn = true;
        }
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

    async fn lookup_block_hash(&self, height: u32) -> Result<Option<String>> {
        let storage = self.storage.lock().unwrap();
        Ok(storage
            .blocks
            .iter()
            .find(|b| b.height == height)
            .map(|b| b.hash.clone()))
    }

    async fn rollback_block(&self, height: u32) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();

        // Find txids of transactions in this block
        let txids: Vec<String> = storage
            .transactions
            .iter()
            .filter(|t| t.block_height == height)
            .map(|t| t.txid.clone())
            .collect();

        // Remove performs and benefits_to for these transactions
        storage.performs.retain(|p| !txids.contains(&p.to_txid));
        storage
            .benefits_to
            .retain(|b| !txids.contains(&b.from_txid));

        // Remove inputs for these transactions
        storage.inputs.retain(|i| !txids.contains(&i.txid));

        // Remove outputs for these transactions
        storage.outputs.retain(|o| !txids.contains(&o.txid));

        // Remove transactions
        storage.transactions.retain(|t| t.block_height != height);

        // Remove block
        storage.blocks.retain(|b| b.height != height);

        Ok(())
    }

    async fn get_max_block_height(&self) -> Result<Option<u32>> {
        let storage = self.storage.lock().unwrap();
        Ok(storage.blocks.iter().map(|b| b.height).max())
    }

    async fn write_blocks_fast(&self, blocks: &[BlockData]) -> Result<()> {
        {
            let mut storage = self.storage.lock().unwrap();
            if let Some(err) = Self::check_failure(&mut storage, "write_blocks_fast") {
                return Err(err);
            }
        }
        self.write_blocks(blocks).await
    }

    async fn write_transactions_fast(&self, transactions: &[TransactionData]) -> Result<()> {
        {
            let mut storage = self.storage.lock().unwrap();
            if let Some(err) = Self::check_failure(&mut storage, "write_transactions_fast") {
                return Err(err);
            }
        }
        self.write_transactions(transactions).await
    }

    async fn write_outputs_fast(&self, outputs: &[OutputData]) -> Result<()> {
        {
            let mut storage = self.storage.lock().unwrap();
            if let Some(err) = Self::check_failure(&mut storage, "write_outputs_fast") {
                return Err(err);
            }
        }
        self.write_outputs(outputs).await
    }

    async fn write_inputs_fast(&self, inputs: &[InputData]) -> Result<()> {
        {
            let mut storage = self.storage.lock().unwrap();
            if let Some(err) = Self::check_failure(&mut storage, "write_inputs_fast") {
                return Err(err);
            }
        }
        self.write_inputs(inputs).await
    }

    async fn begin_transaction(&self) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        if let Some(err) = Self::check_failure(&mut storage, "begin_transaction") {
            return Err(err);
        }
        storage.in_transaction = true;
        storage.snapshot = Some(MockSnapshot {
            blocks_len: storage.blocks.len(),
            transactions_len: storage.transactions.len(),
            outputs_len: storage.outputs.len(),
            inputs_len: storage.inputs.len(),
            performs_len: storage.performs.len(),
            benefits_to_len: storage.benefits_to.len(),
            checkpoint: storage.checkpoint.clone(),
        });
        Ok(())
    }

    async fn commit_transaction(&self) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        if let Some(err) = Self::check_failure(&mut storage, "commit_transaction") {
            // On commit failure, rollback the transaction
            if let Some(snap) = storage.snapshot.take() {
                storage.blocks.truncate(snap.blocks_len);
                storage.transactions.truncate(snap.transactions_len);
                storage.outputs.truncate(snap.outputs_len);
                storage.inputs.truncate(snap.inputs_len);
                storage.performs.truncate(snap.performs_len);
                storage.benefits_to.truncate(snap.benefits_to_len);
                storage.checkpoint = snap.checkpoint;
            }
            storage.in_transaction = false;
            return Err(err);
        }
        storage.in_transaction = false;
        storage.snapshot = None;
        storage.transaction_commit_count += 1;
        Ok(())
    }

    async fn rollback_transaction(&self) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        if let Some(err) = Self::check_failure(&mut storage, "rollback_transaction") {
            return Err(err);
        }
        if let Some(snap) = storage.snapshot.take() {
            storage.blocks.truncate(snap.blocks_len);
            storage.transactions.truncate(snap.transactions_len);
            storage.outputs.truncate(snap.outputs_len);
            storage.inputs.truncate(snap.inputs_len);
            storage.performs.truncate(snap.performs_len);
            storage.benefits_to.truncate(snap.benefits_to_len);
            storage.checkpoint = snap.checkpoint;
        }
        storage.in_transaction = false;
        Ok(())
    }

    async fn check_block_complete(&self, height: u32) -> Result<(u32, u32)> {
        let storage = self.storage.lock().unwrap();

        // Find the block
        let block = storage
            .blocks
            .iter()
            .find(|b| b.height == height)
            .ok_or_else(|| WriterError::QueryFailed(format!("Block {} not found", height)))?;

        let expected = block.tx_count;

        // Count transactions in this block
        let actual = storage
            .transactions
            .iter()
            .filter(|t| t.block_height == height)
            .count() as u32;

        Ok((expected, actual))
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
}
