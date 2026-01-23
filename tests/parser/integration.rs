//! Parser integration tests
//!
//! End-to-end tests combining BlockFileReader and address extraction

use bitcoin::Network;
use bitcoin_chain_graph::parser::{extract_address, BlockFileReader, ScriptType};

#[test]
fn test_extract_genesis_address_from_real_block() {
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin)
        .expect("Failed to open test data file");

    let genesis = reader
        .next_block()
        .expect("Failed to read genesis block")
        .expect("Expected genesis block");

    assert_eq!(genesis.txdata.len(), 1);
    assert!(genesis.txdata[0].is_coinbase());

    let coinbase = &genesis.txdata[0];
    assert_eq!(coinbase.output.len(), 1);

    let output = &coinbase.output[0];
    let script = &output.script_pubkey;

    let info = extract_address(script, Network::Bitcoin);

    assert_eq!(info.script_type, ScriptType::P2PK);
    assert!(info.address.is_some());

    let address = info.address.unwrap();
    assert_eq!(
        address.to_string(),
        "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa",
        "Genesis coinbase output should be Satoshi's address"
    );

    assert_eq!(output.value.to_sat(), 50_0000_0000);
}

#[test]
fn test_extract_addresses_from_first_100_blocks() {
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin)
        .expect("Failed to open test data file");

    let mut total_outputs = 0;
    let mut addressable_count = 0;

    for block_num in 0..100 {
        let block = reader
            .next_block()
            .unwrap_or_else(|e| panic!("Failed to read block {}: {}", block_num, e))
            .unwrap_or_else(|| panic!("Expected block {}, got None", block_num));

        for tx in &block.txdata {
            for output in &tx.output {
                total_outputs += 1;

                let info = extract_address(&output.script_pubkey, Network::Bitcoin);

                if info.address.is_some() {
                    addressable_count += 1;
                }
            }
        }
    }

    assert!(
        total_outputs >= 100,
        "Should have at least 100 outputs (coinbase outputs)"
    );
    assert!(
        addressable_count >= 100,
        "Should have at least 100 addressable outputs (one per block from coinbase)"
    );
}
