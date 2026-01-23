//! BlockFileReader unit tests
//!
//! Tests for reading and parsing Bitcoin Core .blk files

use bitcoin::Network;
use bitcoin_chain_graph::parser::BlockFileReader;
use bitcoin_chain_graph::parser::block_file::ParseError;

#[test]
fn test_parse_genesis_block() {
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin)
        .expect("Failed to open test data file");

    let genesis = reader
        .next_block()
        .expect("Failed to read genesis block")
        .expect("Expected genesis block, got None");

    // Verify genesis block properties
    assert_eq!(genesis.header.version.to_consensus(), 1);
    assert_eq!(genesis.header.time, 1231006505);
    assert_eq!(genesis.txdata.len(), 1);
    assert!(genesis.txdata[0].is_coinbase());
    assert_eq!(genesis.txdata[0].output[0].value.to_sat(), 50_0000_0000); // 50 BTC

    // Verify genesis block hash
    let genesis_hash = genesis.block_hash().to_string();
    assert_eq!(
        genesis_hash,
        "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f"
    );

    assert_eq!(reader.blocks_read(), 1);
}

#[test]
fn test_stream_100_blocks() {
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin)
        .expect("Failed to open test data file");

    let mut count = 0;

    while let Some(block) = reader.next_block().expect("Failed to read block") {
        // Verify basic block properties
        assert!(!block.txdata.is_empty(), "Block should have transactions");
        assert!(
            block.header.version.to_consensus() >= 1,
            "Version should be >= 1"
        );

        count += 1;
        if count >= 100 {
            break;
        }
    }

    assert_eq!(count, 100, "Should have read 100 blocks");
    assert_eq!(reader.blocks_read(), 100);
}

#[test]
fn test_handle_eof_gracefully() {
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin)
        .expect("Failed to open test data file");

    // Read all blocks
    let mut total_blocks = 0;
    while let Some(_block) = reader.next_block().expect("Failed to read block") {
        total_blocks += 1;
    }

    // Should have read many blocks from blk00000.dat (Genesis + more)
    assert!(
        total_blocks > 100,
        "Expected more than 100 blocks in test file, got {}",
        total_blocks
    );

    // Next read should return None (EOF), not error
    let result = reader.next_block();
    assert!(result.is_ok(), "EOF should not be an error");
    assert!(
        result.unwrap().is_none(),
        "Should return None at EOF, not Some"
    );
}

#[test]
fn test_file_not_found() {
    let result = BlockFileReader::new("nonexistent.dat", Network::Bitcoin);
    assert!(result.is_err(), "Should error when file doesn't exist");
}

#[test]
fn test_wrong_network_magic_bytes() {
    // Open mainnet file with testnet reader - should fail on magic byte check
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Testnet)
        .expect("Failed to open test data file");

    // First read should fail due to magic byte mismatch
    let result = reader.next_block();
    assert!(result.is_err(), "Should error on wrong network magic bytes");

    match result {
        Err(ParseError::InvalidMagic { expected, got }) => {
            // Testnet expects 0x0709110B, mainnet file has 0xD9B4BEF9
            assert_eq!(expected, 0x0709110B);
            assert_eq!(got, 0xD9B4BEF9);
        }
        _ => panic!("Expected InvalidMagic error"),
    }
}

#[test]
fn test_block_chain_continuity() {
    // Verify that blocks link together via previousHash
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin)
        .expect("Failed to open test data file");

    let genesis = reader
        .next_block()
        .expect("Failed to read genesis")
        .expect("Expected genesis block");

    let block1 = reader
        .next_block()
        .expect("Failed to read block 1")
        .expect("Expected block 1");

    // Block 1's previousHash should equal Genesis block hash
    assert_eq!(
        block1.header.prev_blockhash,
        genesis.block_hash(),
        "Block 1 should reference Genesis block"
    );

    // Verify chain continues for next few blocks
    let mut prev_block = block1;
    for i in 2..=10 {
        let block = reader
            .next_block()
            .unwrap_or_else(|e| panic!("Failed to read block {}: {}", i, e))
            .unwrap_or_else(|| panic!("Expected block {}, got None", i));

        assert_eq!(
            block.header.prev_blockhash,
            prev_block.block_hash(),
            "Block {} should reference block {}",
            i,
            i - 1
        );

        prev_block = block;
    }
}

#[test]
fn test_coinbase_detection() {
    // Every block should have exactly one coinbase transaction (first tx)
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin)
        .expect("Failed to open test data file");

    for block_num in 0..10 {
        let block = reader
            .next_block()
            .expect("Failed to read block")
            .expect("Expected block");

        assert!(!block.txdata.is_empty(), "Block should have transactions");

        // First transaction should be coinbase
        assert!(
            block.txdata[0].is_coinbase(),
            "First transaction in block {} should be coinbase",
            block_num
        );

        // No other transactions should be coinbase
        for (i, tx) in block.txdata.iter().enumerate().skip(1) {
            assert!(
                !tx.is_coinbase(),
                "Transaction {} in block {} should not be coinbase",
                i,
                block_num
            );
        }
    }
}

#[test]
fn test_witness_data_present() {
    // SegWit activated at block 481824 (not in our test data)
    // But we can verify witness structure exists in parsed blocks
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin)
        .expect("Failed to open test data file");

    let genesis = reader.next_block().unwrap().unwrap();

    // Genesis block has no witness data (pre-SegWit)
    for tx in &genesis.txdata {
        for input in &tx.input {
            assert!(
                input.witness.is_empty(),
                "Genesis block should have no witness data"
            );
        }
    }
}
