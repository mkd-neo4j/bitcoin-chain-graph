//! Address extraction unit tests
//!
//! Tests for deriving Bitcoin addresses from scriptPubKey

use bitcoin::{Network, ScriptBuf};
use bitcoin_chain_graph::parser::{extract_address, ScriptType};

#[test]
fn test_genesis_block_p2pk_address() {
    // Genesis block coinbase output (P2PK)
    let script_hex = "4104678afdb0fe5548271967f1a67130b7105cd6a828e03909a67962e0ea1f61deb649f6bc3f4cef38c4f35504e51ec112de5c384df7ba0b8d578a4c702b6bf11d5fac";
    let script_bytes = hex::decode(script_hex).unwrap();
    let script = ScriptBuf::from(script_bytes);

    let info = extract_address(&script, Network::Bitcoin);

    assert_eq!(info.script_type, ScriptType::P2PK);
    assert!(info.address.is_some());

    let address = info.address.unwrap();
    assert_eq!(address.to_string(), "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa");
}

#[test]
fn test_p2pkh_address() {
    let script_hex = "76a91489abcdefabbaabbaabbaabbaabbaabbaabbaabba88ac";
    let script_bytes = hex::decode(script_hex).unwrap();
    let script = ScriptBuf::from(script_bytes);

    let info = extract_address(&script, Network::Bitcoin);

    assert_eq!(info.script_type, ScriptType::P2PKH);
    assert!(info.address.is_some());
    assert!(info.address.unwrap().to_string().starts_with('1'));
}

#[test]
fn test_p2sh_address() {
    let script_hex = "a91489abcdefabbaabbaabbaabbaabbaabbaabbaabba87";
    let script_bytes = hex::decode(script_hex).unwrap();
    let script = ScriptBuf::from(script_bytes);

    let info = extract_address(&script, Network::Bitcoin);

    assert_eq!(info.script_type, ScriptType::P2SH);
    assert!(info.address.is_some());
    assert!(info.address.unwrap().to_string().starts_with('3'));
}

#[test]
fn test_p2wpkh_address() {
    let script_hex = "0014751e76e8199196d454941c45d1b3a323f1433bd6";
    let script_bytes = hex::decode(script_hex).unwrap();
    let script = ScriptBuf::from(script_bytes);

    let info = extract_address(&script, Network::Bitcoin);

    assert_eq!(info.script_type, ScriptType::P2WPKH);
    assert!(info.address.is_some());

    let address = info.address.unwrap();
    assert_eq!(address.to_string(), "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4");
}

#[test]
fn test_p2wsh_address() {
    let script_hex = "00201863143c14c5166804bd19203356da136c985678cd4d27a1b8c6329604903262";
    let script_bytes = hex::decode(script_hex).unwrap();
    let script = ScriptBuf::from(script_bytes);

    let info = extract_address(&script, Network::Bitcoin);

    assert_eq!(info.script_type, ScriptType::P2WSH);
    assert!(info.address.is_some());

    let address = info.address.unwrap();
    assert!(address.to_string().starts_with("bc1q"));
    assert!(address.to_string().len() > 50);
}

#[test]
fn test_p2tr_address() {
    let script_hex = "512079be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
    let script_bytes = hex::decode(script_hex).unwrap();
    let script = ScriptBuf::from(script_bytes);

    let info = extract_address(&script, Network::Bitcoin);

    assert_eq!(info.script_type, ScriptType::P2TR);
    assert!(info.address.is_some());
    assert!(info.address.unwrap().to_string().starts_with("bc1p"));
}

#[test]
fn test_null_data_no_address() {
    let script_hex = "6a0548656c6c6f"; // OP_RETURN "Hello"
    let script_bytes = hex::decode(script_hex).unwrap();
    let script = ScriptBuf::from(script_bytes);

    let info = extract_address(&script, Network::Bitcoin);

    assert_eq!(info.script_type, ScriptType::NullData);
    assert!(info.address.is_none());
}

#[test]
fn test_unknown_script() {
    let script_hex = "ff"; // Invalid opcode
    let script_bytes = hex::decode(script_hex).unwrap();
    let script = ScriptBuf::from(script_bytes);

    let info = extract_address(&script, Network::Bitcoin);

    assert_eq!(info.script_type, ScriptType::Unknown);
    assert!(info.address.is_none());
}

#[test]
fn test_empty_script() {
    let script = ScriptBuf::from(vec![]);

    let info = extract_address(&script, Network::Bitcoin);

    assert_eq!(info.script_type, ScriptType::Unknown);
    assert!(info.address.is_none());
}

#[test]
fn test_testnet_addresses() {
    // P2PKH
    let script_hex = "76a91489abcdefabbaabbaabbaabbaabbaabbaabbaabba88ac";
    let script = ScriptBuf::from(hex::decode(script_hex).unwrap());
    let info = extract_address(&script, Network::Testnet);
    assert_eq!(info.script_type, ScriptType::P2PKH);
    let addr = info.address.unwrap().to_string();
    assert!(addr.starts_with('m') || addr.starts_with('n'));

    // P2WPKH
    let script_hex = "0014751e76e8199196d454941c45d1b3a323f1433bd6";
    let script = ScriptBuf::from(hex::decode(script_hex).unwrap());
    let info = extract_address(&script, Network::Testnet);
    assert_eq!(info.script_type, ScriptType::P2WPKH);
    assert!(info.address.unwrap().to_string().starts_with("tb1q"));

    // P2TR
    let script_hex = "512079be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
    let script = ScriptBuf::from(hex::decode(script_hex).unwrap());
    let info = extract_address(&script, Network::Testnet);
    assert_eq!(info.script_type, ScriptType::P2TR);
    assert!(info.address.unwrap().to_string().starts_with("tb1p"));
}

#[test]
fn test_compressed_p2pk() {
    // Real compressed pubkey from Bitcoin blockchain
    let script_hex = "210279BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798AC";
    let script_bytes = hex::decode(script_hex).unwrap();
    let script = ScriptBuf::from(script_bytes);

    let info = extract_address(&script, Network::Bitcoin);

    assert_eq!(info.script_type, ScriptType::P2PK);
    assert!(info.address.is_some());
    assert!(info.address.unwrap().to_string().starts_with('1'));
}

#[test]
fn test_malformed_scripts() {
    // P2PKH with wrong hash length
    let script_hex = "76a91389abcdefabbaabbaabbaabbaabbaabbaabbaabba88ac";
    let script = ScriptBuf::from(hex::decode(script_hex).unwrap());
    let info = extract_address(&script, Network::Bitcoin);
    assert_eq!(info.script_type, ScriptType::Unknown);
    assert!(info.address.is_none());

    // Future witness version
    let script_hex = "522079be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
    let script = ScriptBuf::from(hex::decode(script_hex).unwrap());
    let info = extract_address(&script, Network::Bitcoin);
    assert_eq!(info.script_type, ScriptType::Unknown);
    assert!(info.address.is_none());
}
