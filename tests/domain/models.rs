//! Domain model conversion tests
//!
//! Tests for converting bitcoin crate types to domain models

use bitcoin::Network;
use bitcoin_chain_graph::domain::{BlockData, InputData, OutputData, TransactionData};
use bitcoin_chain_graph::parser::BlockFileReader;

#[test]
fn test_block_conversion_genesis() {
    // Load genesis block from test data
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin)
        .expect("Failed to open test data file");

    let genesis = reader
        .next_block()
        .expect("Failed to read genesis block")
        .expect("Expected genesis block");

    // Convert to domain model
    let block_data = BlockData::from_block(&genesis, 0);

    // Verify all properties
    assert_eq!(block_data.height, 0);
    assert_eq!(
        block_data.hash,
        "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f"
    );
    assert_eq!(
        block_data.previous_hash,
        "0000000000000000000000000000000000000000000000000000000000000000"
    );
    assert_eq!(
        block_data.merkle_root,
        "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b"
    );
    assert_eq!(block_data.timestamp, 1231006505);
    assert_eq!(block_data.nonce, 2083236893);
    assert_eq!(block_data.version, 1);
    assert_eq!(block_data.tx_count, 1);

    // Difficulty should be approximately 1.0 for genesis
    assert!(
        (block_data.difficulty - 1.0).abs() < 0.01,
        "Genesis difficulty should be ~1.0, got {}",
        block_data.difficulty
    );
}

#[test]
fn test_transaction_conversion_genesis_coinbase() {
    // Load genesis block
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin)
        .expect("Failed to open test data file");

    let genesis = reader
        .next_block()
        .expect("Failed to read genesis")
        .expect("Expected genesis block");

    let coinbase = &genesis.txdata[0];
    let block_hash = genesis.block_hash().to_string();
    let timestamp = genesis.header.time as i64;

    // Convert to domain model
    let tx_data = TransactionData::from_transaction(coinbase, 0, &block_hash, timestamp);

    // Verify properties
    assert_eq!(
        tx_data.txid,
        "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b"
    );
    assert_eq!(tx_data.block_height, 0);
    assert_eq!(tx_data.block_hash, block_hash);
    assert_eq!(tx_data.timestamp, 1231006505);
    assert_eq!(tx_data.version, 1);
    assert_eq!(tx_data.locktime, 0);
    assert!(tx_data.is_coinbase);
}

#[test]
fn test_output_conversion_genesis_coinbase() {
    // Load genesis block
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin)
        .expect("Failed to open test data file");

    let genesis = reader
        .next_block()
        .expect("Failed to read genesis")
        .expect("Expected genesis block");

    let coinbase = &genesis.txdata[0];
    let txid = coinbase.txid().to_string();
    let output = &coinbase.output[0];

    // Convert to domain model
    let output_data = OutputData::from_output(output, &txid, 0, Network::Bitcoin);

    // Verify properties
    assert_eq!(output_data.output_id, format!("{}:0", txid));
    assert_eq!(output_data.output_index, 0);
    assert_eq!(output_data.txid, txid);
    assert_eq!(output_data.amount, 50_0000_0000); // 50 BTC in satoshis
    assert_eq!(output_data.script_type, "P2PK");

    // Verify address extraction
    assert!(output_data.address.is_some());
    assert_eq!(
        output_data.address.unwrap(),
        "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"
    );
}

#[test]
fn test_input_conversion_coinbase() {
    // Load genesis block
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin)
        .expect("Failed to open test data file");

    let genesis = reader
        .next_block()
        .expect("Failed to read genesis")
        .expect("Expected genesis block");

    let coinbase = &genesis.txdata[0];
    let txid = coinbase.txid().to_string();
    let input = &coinbase.input[0];

    // Convert to domain model
    let input_data = InputData::from_input(input, &txid, 0, 0);

    // Verify properties
    assert_eq!(input_data.input_id, format!("{}:0", txid));
    assert_eq!(input_data.input_index, 0);
    assert_eq!(input_data.txid, txid);

    // Coinbase inputs have special previous output reference
    assert_eq!(
        input_data.previous_txid,
        "0000000000000000000000000000000000000000000000000000000000000000"
    );
    assert_eq!(input_data.previous_output_index, 0xFFFFFFFF);

    // Genesis has no witness data (pre-SegWit)
    assert!(input_data.witness.is_empty());
}

#[test]
fn test_all_model_types_implement_clone_debug() {
    // Verify all domain models implement required traits
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin)
        .expect("Failed to open test data file");

    let genesis = reader.next_block().unwrap().unwrap();

    // BlockData
    let block_data = BlockData::from_block(&genesis, 0);
    let _cloned = block_data.clone();
    let _debug = format!("{:?}", block_data);

    // TransactionData
    let tx_data = TransactionData::from_transaction(
        &genesis.txdata[0],
        0,
        &genesis.block_hash().to_string(),
        genesis.header.time as i64,
    );
    let _cloned = tx_data.clone();
    let _debug = format!("{:?}", tx_data);

    // OutputData
    let output_data = OutputData::from_output(
        &genesis.txdata[0].output[0],
        &genesis.txdata[0].txid().to_string(),
        0,
        Network::Bitcoin,
    );
    let _cloned = output_data.clone();
    let _debug = format!("{:?}", output_data);

    // InputData
    let input_data = InputData::from_input(
        &genesis.txdata[0].input[0],
        &genesis.txdata[0].txid().to_string(),
        0,
        0, // block height (genesis)
    );
    let _cloned = input_data.clone();
    let _debug = format!("{:?}", input_data);
}
