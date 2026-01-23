//! Integration tests for Writer layer
//!
//! Tests the GraphWriter trait and MockWriter implementation.

mod mock_writer_tests {
    use bitcoin::Network;
    use bitcoin_chain_graph::domain::{BlockData, TransactionData, OutputData, InputData, CheckpointData};
    use bitcoin_chain_graph::parser::BlockFileReader;
    use bitcoin_chain_graph::writer::{GraphWriter, MockWriter};

    #[tokio::test]
    async fn test_mock_writer_implements_graph_writer() {
        // Verify MockWriter implements GraphWriter trait
        let writer: Box<dyn GraphWriter> = Box::new(MockWriter::new());

        // Should be able to call trait methods
        writer.init_schema().await.unwrap();
    }

    #[tokio::test]
    async fn test_mock_writer_schema_initialization() {
        let writer = MockWriter::new();

        // Schema not initialized initially
        assert!(!writer.is_schema_initialized().await);

        // Initialize schema
        writer.init_schema().await.unwrap();

        // Schema should be initialized now
        assert!(writer.is_schema_initialized().await);
    }

    #[tokio::test]
    async fn test_write_and_retrieve_blocks() {
        let writer = MockWriter::new();

        // Create test blocks
        let blocks = vec![
            BlockData {
                height: 0,
                hash: String::from("000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f"),
                previous_hash: String::from("0000000000000000000000000000000000000000000000000000000000000000"),
                merkle_root: String::from("4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b"),
                timestamp: 1231006505,
                bits: String::from("1d00ffff"),
                difficulty: 1.0,
                nonce: 2083236893,
                version: 1,
                tx_count: 1,
                size: 285,
                weight: 1140,
            },
            BlockData {
                height: 1,
                hash: String::from("00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048"),
                previous_hash: String::from("000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f"),
                merkle_root: String::from("0e3e2357e806b6cdb1f70b54c3a3a17b6714ee1f0e68bebb44a74b1efd512098"),
                timestamp: 1231469665,
                bits: String::from("1d00ffff"),
                difficulty: 1.0,
                nonce: 2573394689,
                version: 1,
                tx_count: 1,
                size: 215,
                weight: 860,
            },
        ];

        // Write blocks
        writer.write_blocks(&blocks).await.unwrap();

        // Retrieve and verify
        let stored = writer.get_blocks().await;
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].height, 0);
        assert_eq!(stored[0].hash, "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f");
        assert_eq!(stored[1].height, 1);
        assert_eq!(stored[1].hash, "00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048");
    }

    #[tokio::test]
    async fn test_write_and_retrieve_transactions() {
        let writer = MockWriter::new();

        let transactions = vec![
            TransactionData {
                txid: String::from("4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b"),
                block_height: 0,
                block_hash: String::from("000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f"),
                timestamp: 1231006505,
                version: 1,
                locktime: 0,
                size: 204,
                vsize: 204,
                weight: 816,
                is_coinbase: true,
            },
        ];

        writer.write_transactions(&transactions).await.unwrap();

        let stored = writer.get_transactions().await;
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].txid, "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b");
        assert!(stored[0].is_coinbase);
    }

    #[tokio::test]
    async fn test_write_and_retrieve_outputs() {
        let writer = MockWriter::new();

        let outputs = vec![
            OutputData {
                output_id: String::from("4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b:0"),
                output_index: 0,
                txid: String::from("4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b"),
                amount: 5000000000,
                script_pubkey: String::from("4104678afdb0fe5548271967f1a67130b7105cd6a828e03909a67962e0ea1f61deb649f6bc3f4cef38c4f35504e51ec112de5c384df7ba0b8d578a4c702b6bf11d5fac"),
                script_type: String::from("P2PK"),
                address: Some(String::from("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa")),
            },
        ];

        writer.write_outputs(&outputs).await.unwrap();

        let stored = writer.get_outputs().await;
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].output_id, "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b:0");
        assert_eq!(stored[0].amount, 5000000000);
        assert_eq!(stored[0].script_type, "P2PK");
    }

    #[tokio::test]
    async fn test_write_and_retrieve_inputs() {
        let writer = MockWriter::new();

        let inputs = vec![
            InputData {
                input_id: String::from("0e3e2357e806b6cdb1f70b54c3a3a17b6714ee1f0e68bebb44a74b1efd512098:0"),
                input_index: 0,
                txid: String::from("0e3e2357e806b6cdb1f70b54c3a3a17b6714ee1f0e68bebb44a74b1efd512098"),
                previous_txid: String::from("4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b"),
                previous_output_index: 0,
                script_sig: String::from("4730440220..."),
                sequence: 0xFFFFFFFF,
                witness: vec![],
            },
        ];

        writer.write_inputs(&inputs).await.unwrap();

        let stored = writer.get_inputs().await;
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].input_id, "0e3e2357e806b6cdb1f70b54c3a3a17b6714ee1f0e68bebb44a74b1efd512098:0");
        assert_eq!(stored[0].previous_txid, "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b");
    }

    #[tokio::test]
    async fn test_checkpoint_lifecycle() {
        let writer = MockWriter::new();

        // No checkpoint initially
        let checkpoint = writer.get_checkpoint().await.unwrap();
        assert!(checkpoint.is_none());

        // Create initial checkpoint
        writer.create_checkpoint().await.unwrap();

        let checkpoint = writer.get_checkpoint().await.unwrap().unwrap();
        assert_eq!(checkpoint.status, "in_progress");
        assert_eq!(checkpoint.last_processed_height, 0);
        assert_eq!(checkpoint.last_processed_file, "blk00000.dat");

        // Update checkpoint
        let updated = CheckpointData {
            last_processed_height: 100,
            last_processed_hash: String::from("block100hash"),
            last_processed_file: String::from("blk00000.dat"),
            last_processed_file_offset: Some(12345),
            timestamp: chrono::Utc::now().timestamp(),
            status: String::from("in_progress"),
        };

        writer.update_checkpoint(&updated).await.unwrap();

        let checkpoint = writer.get_checkpoint().await.unwrap().unwrap();
        assert_eq!(checkpoint.last_processed_height, 100);
        assert_eq!(checkpoint.last_processed_hash, "block100hash");
        assert_eq!(checkpoint.status, "in_progress");

        // Mark complete
        writer.mark_checkpoint_complete().await.unwrap();

        let checkpoint = writer.get_checkpoint().await.unwrap().unwrap();
        assert_eq!(checkpoint.status, "completed");
    }

    #[tokio::test]
    async fn test_lookup_output_success() {
        let writer = MockWriter::new();

        let output = OutputData {
            output_id: String::from("genesis:0"),
            output_index: 0,
            txid: String::from("genesis"),
            amount: 5000000000,
            script_pubkey: String::from("4104...ac"),
            script_type: String::from("P2PK"),
            address: Some(String::from("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa")),
        };

        writer.write_outputs(&[output]).await.unwrap();

        // Lookup should succeed
        let found = writer.lookup_output("genesis:0").await.unwrap();
        assert_eq!(found.output_id, "genesis:0");
        assert_eq!(found.amount, 5000000000);
    }

    #[tokio::test]
    async fn test_lookup_output_not_found() {
        let writer = MockWriter::new();

        // Lookup non-existent output should fail
        let result = writer.lookup_output("nonexistent:0").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mark_output_spent() {
        let writer = MockWriter::new();

        // Create an output
        let output = OutputData {
            output_id: String::from("tx1:0"),
            output_index: 0,
            txid: String::from("tx1"),
            amount: 5000000000,
            script_pubkey: String::from("76a914...88ac"),
            script_type: String::from("P2PKH"),
            address: Some(String::from("1Address...")),
        };

        writer.write_outputs(&[output]).await.unwrap();

        // Mark as spent should succeed
        writer.mark_output_spent("tx1:0", "tx2", 100).await.unwrap();

        // Mark non-existent output as spent should fail
        let result = writer.mark_output_spent("nonexistent:0", "tx2", 100).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_calculate_amounts_no_op() {
        let writer = MockWriter::new();

        let transactions = vec![
            TransactionData {
                txid: String::from("tx1"),
                block_height: 0,
                block_hash: String::from("block0"),
                timestamp: 1231006505,
                version: 1,
                locktime: 0,
                size: 204,
                vsize: 204,
                weight: 816,
                is_coinbase: true,
            },
        ];

        // Should succeed (no-op in MockWriter)
        writer.calculate_amounts(&transactions).await.unwrap();
    }

    #[tokio::test]
    async fn test_write_simplified_layer_no_op() {
        let writer = MockWriter::new();

        let transactions = vec![
            TransactionData {
                txid: String::from("tx1"),
                block_height: 0,
                block_hash: String::from("block0"),
                timestamp: 1231006505,
                version: 1,
                locktime: 0,
                size: 204,
                vsize: 204,
                weight: 816,
                is_coinbase: true,
            },
        ];

        // Should succeed (no-op in MockWriter)
        writer.write_simplified_layer(&transactions).await.unwrap();
    }

    #[tokio::test]
    async fn test_clear_storage() {
        let writer = MockWriter::new();

        // Add some data
        let blocks = vec![
            BlockData {
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
            },
        ];

        writer.write_blocks(&blocks).await.unwrap();
        writer.create_checkpoint().await.unwrap();
        writer.init_schema().await.unwrap();

        // Verify data exists
        assert_eq!(writer.get_blocks().await.len(), 1);
        assert!(writer.get_checkpoint().await.unwrap().is_some());
        assert!(writer.is_schema_initialized().await);

        // Clear
        writer.clear().await;

        // Verify all data is gone
        assert_eq!(writer.get_blocks().await.len(), 0);
        assert!(writer.get_checkpoint().await.unwrap().is_none());
        assert!(!writer.is_schema_initialized().await);
    }

    #[tokio::test]
    async fn test_end_to_end_genesis_block() {
        // Test ingesting genesis block through MockWriter
        let writer = MockWriter::new();

        // Initialize schema
        writer.init_schema().await.unwrap();

        // Load genesis block
        let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin)
            .expect("Failed to open test data file");

        let genesis = reader
            .next_block()
            .expect("Failed to read genesis block")
            .expect("Expected genesis block");

        // Convert to domain model
        let block_data = BlockData::from_block(&genesis, 0);

        // Write block
        writer.write_blocks(&[block_data]).await.unwrap();

        // Convert and write transaction
        let tx = &genesis.txdata[0];
        let tx_data = TransactionData::from_transaction(
            tx,
            0,
            &genesis.block_hash().to_string(),
            genesis.header.time as i64,
        );

        writer.write_transactions(&[tx_data]).await.unwrap();

        // Convert and write output
        let output_data = OutputData::from_output(
            &tx.output[0],
            &tx.txid().to_string(),
            0,
            Network::Bitcoin,
        );

        writer.write_outputs(&[output_data]).await.unwrap();

        // Convert and write input (coinbase)
        let input_data = InputData::from_input(
            &tx.input[0],
            &tx.txid().to_string(),
            0,
        );

        writer.write_inputs(&[input_data]).await.unwrap();

        // Verify all data was stored
        assert_eq!(writer.get_blocks().await.len(), 1);
        assert_eq!(writer.get_transactions().await.len(), 1);
        assert_eq!(writer.get_outputs().await.len(), 1);
        assert_eq!(writer.get_inputs().await.len(), 1);

        // Verify block data
        let stored_block = &writer.get_blocks().await[0];
        assert_eq!(stored_block.height, 0);
        assert_eq!(stored_block.hash, "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f");

        // Verify transaction data
        let stored_tx = &writer.get_transactions().await[0];
        assert!(stored_tx.is_coinbase);

        // Verify output data
        let stored_output = &writer.get_outputs().await[0];
        assert_eq!(stored_output.amount, 5000000000);
        assert_eq!(stored_output.address.as_ref().unwrap(), "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa");
    }
}
