//! Edge case and stress tests for Writer layer
//!
//! Tests edge cases, concurrency, and error conditions that aren't covered
//! in the basic integration tests.

mod edge_case_tests {
    use bitcoin_chain_graph::domain::{BlockData, OutputData, InputData, CheckpointData};
    use bitcoin_chain_graph::writer::{GraphWriter, MockWriter, WriterError};
    use std::sync::Arc;
    use tokio::task::JoinSet;

    #[tokio::test]
    async fn test_write_empty_slices() {
        let writer = MockWriter::new();

        // Empty writes should succeed (no-op)
        writer.write_blocks(&[]).await.unwrap();
        writer.write_transactions(&[]).await.unwrap();
        writer.write_outputs(&[]).await.unwrap();
        writer.write_inputs(&[]).await.unwrap();

        // Verify nothing was stored
        assert_eq!(writer.get_blocks().await.len(), 0);
        assert_eq!(writer.get_transactions().await.len(), 0);
        assert_eq!(writer.get_outputs().await.len(), 0);
        assert_eq!(writer.get_inputs().await.len(), 0);
    }

    #[tokio::test]
    async fn test_duplicate_block_writes() {
        let writer = MockWriter::new();

        let block = BlockData {
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
        };

        // Write same block twice
        writer.write_blocks(&[block.clone()]).await.unwrap();
        writer.write_blocks(&[block.clone()]).await.unwrap();

        // MockWriter allows duplicates (real implementation would use MERGE)
        let stored = writer.get_blocks().await;
        assert_eq!(stored.len(), 2, "MockWriter stores duplicates");
    }

    #[tokio::test]
    async fn test_spend_nonexistent_output() {
        let writer = MockWriter::new();

        let input = InputData {
            input_id: String::from("tx1:0"),
            input_index: 0,
            txid: String::from("tx1"),
            previous_txid: String::from("nonexistent_tx"),
            previous_output_index: 0,
            script_sig: String::from(""),
            sequence: 0xFFFFFFFF,
            witness: vec![],
        };

        // MockWriter doesn't validate this (writes succeed)
        // Real Neo4j implementation would need to handle this
        writer.write_inputs(&[input]).await.unwrap();

        let stored = writer.get_inputs().await;
        assert_eq!(stored.len(), 1);
    }

    #[tokio::test]
    async fn test_checkpoint_operations_before_creation() {
        let writer = MockWriter::new();

        // Get checkpoint before creation - should return None
        let checkpoint = writer.get_checkpoint().await.unwrap();
        assert!(checkpoint.is_none());

        // Update checkpoint before creation - should succeed (creates it)
        let checkpoint = CheckpointData {
            last_processed_height: 100,
            last_processed_hash: String::from("block100"),
            last_processed_file: String::from("blk00000.dat"),
            last_processed_file_offset: Some(12345),
            timestamp: chrono::Utc::now().timestamp(),
            status: String::from("in_progress"),
        };

        writer.update_checkpoint(&checkpoint).await.unwrap();

        // Should now exist
        let stored = writer.get_checkpoint().await.unwrap().unwrap();
        assert_eq!(stored.last_processed_height, 100);
    }

    #[tokio::test]
    async fn test_mark_checkpoint_complete_before_creation() {
        let writer = MockWriter::new();

        // Mark complete before creation - no-op in MockWriter
        writer.mark_checkpoint_complete().await.unwrap();

        // Checkpoint still doesn't exist
        let checkpoint = writer.get_checkpoint().await.unwrap();
        assert!(checkpoint.is_none());
    }

    #[tokio::test]
    async fn test_concurrent_writes_same_type() {
        let writer = Arc::new(MockWriter::new());
        let mut tasks = JoinSet::new();

        // Spawn 10 concurrent tasks writing blocks
        for i in 0..10 {
            let writer_clone = Arc::clone(&writer);
            tasks.spawn(async move {
                let block = BlockData {
                    height: i,
                    hash: format!("block{}", i),
                    previous_hash: String::from("prev"),
                    merkle_root: String::from("merkle"),
                    timestamp: 1231006505,
                    bits: String::from("1d00ffff"),
                    difficulty: 1.0,
                    nonce: 2083236893,
                    version: 1,
                    tx_count: 1,
                    size: 285,
                    weight: 1140,
                };
                writer_clone.write_blocks(&[block]).await.unwrap();
            });
        }

        // Wait for all tasks to complete
        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }

        // All 10 blocks should be stored
        let stored = writer.get_blocks().await;
        assert_eq!(stored.len(), 10);

        // Verify all heights are present (order may vary)
        let mut heights: Vec<_> = stored.iter().map(|b| b.height).collect();
        heights.sort();
        assert_eq!(heights, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[tokio::test]
    async fn test_concurrent_mixed_operations() {
        let writer = Arc::new(MockWriter::new());
        let mut tasks = JoinSet::new();

        // Spawn mixed read/write operations
        for i in 0..5 {
            let writer_clone = Arc::clone(&writer);
            tasks.spawn(async move {
                let block = BlockData {
                    height: i,
                    hash: format!("block{}", i),
                    previous_hash: String::from("prev"),
                    merkle_root: String::from("merkle"),
                    timestamp: 1231006505,
                    bits: String::from("1d00ffff"),
                    difficulty: 1.0,
                    nonce: 2083236893,
                    version: 1,
                    tx_count: 1,
                    size: 285,
                    weight: 1140,
                };
                writer_clone.write_blocks(&[block]).await.unwrap();
                let _blocks = writer_clone.get_blocks().await;
            });
        }

        // Wait for all tasks
        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }

        let stored = writer.get_blocks().await;
        assert_eq!(stored.len(), 5);
    }

    #[tokio::test]
    async fn test_lookup_output_complete_data_integrity() {
        let writer = MockWriter::new();

        let output = OutputData {
            output_id: String::from("tx1:0"),
            output_index: 0,
            txid: String::from("tx1"),
            amount: 5000000000,
            script_pubkey: String::from("76a914...88ac"),
            script_type: String::from("P2PKH"),
            address: Some(String::from("1Address123")),
        };

        writer.write_outputs(&[output.clone()]).await.unwrap();

        let found = writer.lookup_output("tx1:0").await.unwrap();

        // Verify ALL fields match
        assert_eq!(found.output_id, output.output_id);
        assert_eq!(found.output_index, output.output_index);
        assert_eq!(found.txid, output.txid);
        assert_eq!(found.amount, output.amount);
        assert_eq!(found.script_pubkey, output.script_pubkey);
        assert_eq!(found.script_type, output.script_type);
        assert_eq!(found.address, output.address);
    }

    #[tokio::test]
    async fn test_large_batch_write() {
        let writer = MockWriter::new();

        // Create 1000 blocks
        let blocks: Vec<_> = (0..1000)
            .map(|i| BlockData {
                height: i,
                hash: format!("block{}", i),
                previous_hash: format!("prev{}", i.saturating_sub(1)),
                merkle_root: String::from("merkle"),
                timestamp: 1231006505 + i as i64,
                bits: String::from("1d00ffff"),
                difficulty: 1.0,
                nonce: 2083236893,
                version: 1,
                tx_count: 1,
                size: 285,
                weight: 1140,
            })
            .collect();

        writer.write_blocks(&blocks).await.unwrap();

        let stored = writer.get_blocks().await;
        assert_eq!(stored.len(), 1000);

        // Verify first and last
        assert_eq!(stored[0].height, 0);
        assert_eq!(stored[999].height, 999);
    }

    #[tokio::test]
    async fn test_output_with_null_address() {
        let writer = MockWriter::new();

        // NULL_DATA output has no address
        let output = OutputData {
            output_id: String::from("tx1:0"),
            output_index: 0,
            txid: String::from("tx1"),
            amount: 0,
            script_pubkey: String::from("6a0548656c6c6f"),
            script_type: String::from("NULL_DATA"),
            address: None,
        };

        writer.write_outputs(&[output.clone()]).await.unwrap();

        let found = writer.lookup_output("tx1:0").await.unwrap();
        assert!(found.address.is_none());
        assert_eq!(found.script_type, "NULL_DATA");
    }

    #[tokio::test]
    async fn test_coinbase_input_handling() {
        let writer = MockWriter::new();

        // Coinbase input (previous_output_index = 0xFFFFFFFF)
        let input = InputData {
            input_id: String::from("coinbase:0"),
            input_index: 0,
            txid: String::from("coinbase"),
            previous_txid: String::from("0000000000000000000000000000000000000000000000000000000000000000"),
            previous_output_index: 0xFFFFFFFF,
            script_sig: String::from("coinbase_data"),
            sequence: 0xFFFFFFFF,
            witness: vec![],
        };

        // Should succeed without looking up previous output
        writer.write_inputs(&[input]).await.unwrap();

        let stored = writer.get_inputs().await;
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].previous_output_index, 0xFFFFFFFF);
    }

    #[tokio::test]
    async fn test_multiple_checkpoint_updates() {
        let writer = MockWriter::new();

        writer.create_checkpoint().await.unwrap();

        // Update checkpoint 100 times rapidly
        for i in 1..=100 {
            let checkpoint = CheckpointData {
                last_processed_height: i,
                last_processed_hash: format!("block{}", i),
                last_processed_file: String::from("blk00000.dat"),
                last_processed_file_offset: Some(i as u64 * 1000),
                timestamp: chrono::Utc::now().timestamp(),
                status: String::from("in_progress"),
            };

            writer.update_checkpoint(&checkpoint).await.unwrap();
        }

        // Final checkpoint should be block 100
        let final_checkpoint = writer.get_checkpoint().await.unwrap().unwrap();
        assert_eq!(final_checkpoint.last_processed_height, 100);
    }

    #[tokio::test]
    async fn test_clear_storage_is_complete() {
        let writer = MockWriter::new();

        // Add data to all storage types
        let block = BlockData {
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
        };

        writer.write_blocks(&[block]).await.unwrap();
        writer.init_schema().await.unwrap();
        writer.create_checkpoint().await.unwrap();

        // Clear everything
        writer.clear().await;

        // Verify all state is reset
        assert_eq!(writer.get_blocks().await.len(), 0);
        assert_eq!(writer.get_transactions().await.len(), 0);
        assert_eq!(writer.get_outputs().await.len(), 0);
        assert_eq!(writer.get_inputs().await.len(), 0);
        assert!(writer.get_checkpoint().await.unwrap().is_none());
        assert!(!writer.is_schema_initialized().await);
    }

    #[tokio::test]
    async fn test_error_type_is_send_sync() {
        // Verify WriterError can be sent across threads
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<WriterError>();
        assert_sync::<WriterError>();
    }

    #[tokio::test]
    async fn test_mock_writer_is_clone() {
        let writer1 = MockWriter::new();
        writer1.init_schema().await.unwrap();

        // Clone should share the same storage
        let writer2 = writer1.clone();

        let block = BlockData {
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
        };

        writer1.write_blocks(&[block]).await.unwrap();

        // writer2 should see the block written by writer1
        assert_eq!(writer2.get_blocks().await.len(), 1);
        assert!(writer2.is_schema_initialized().await);
    }
}
