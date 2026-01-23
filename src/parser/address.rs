use bitcoin::{Address, Network, ScriptBuf, PublicKey};
use bitcoin::script::Instruction;

/// Types of Bitcoin script patterns
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptType {
    P2PKH,      // Pay to Public Key Hash
    P2SH,       // Pay to Script Hash
    P2WPKH,     // Pay to Witness Public Key Hash (SegWit v0)
    P2WSH,      // Pay to Witness Script Hash (SegWit v0)
    P2TR,       // Pay to Taproot (SegWit v1)
    P2PK,       // Pay to Public Key (obsolete)
    NullData,   // OP_RETURN (unspendable)
    Unknown,    // Non-standard or future script types
}

/// Result of address derivation from a scriptPubKey
#[derive(Debug, Clone)]
pub struct AddressInfo {
    pub address: Option<Address>,
    pub script_type: ScriptType,
}

/// Extract address and script type from a scriptPubKey
///
/// Returns an `AddressInfo` with:
/// - `address`: Some(Address) for standard types with addresses, None for NULL_DATA/UNKNOWN
/// - `script_type`: The detected script pattern
///
/// # Arguments
/// * `script` - The scriptPubKey from a transaction output
/// * `network` - Bitcoin network (mainnet, testnet, etc.)
///
/// # Example
/// ```
/// use bitcoin::{Network, ScriptBuf};
/// use bitcoin_chain_graph::parser::extract_address;
///
/// let script = ScriptBuf::from(vec![0x6a]); // OP_RETURN
/// let info = extract_address(&script, Network::Bitcoin);
/// assert!(info.address.is_none()); // NULL_DATA has no address
/// ```
pub fn extract_address(script: &ScriptBuf, network: Network) -> AddressInfo {
    // Try to detect script type and derive address using bitcoin crate

    // Check for OP_RETURN (NULL_DATA)
    if script.is_op_return() {
        return AddressInfo {
            address: None,
            script_type: ScriptType::NullData,
        };
    }

    // Check for P2PKH
    if script.is_p2pkh() {
        return match Address::from_script(script, network) {
            Ok(addr) => AddressInfo {
                address: Some(addr),
                script_type: ScriptType::P2PKH,
            },
            Err(_) => AddressInfo {
                address: None,
                script_type: ScriptType::Unknown,
            },
        };
    }

    // Check for P2SH
    if script.is_p2sh() {
        return match Address::from_script(script, network) {
            Ok(addr) => AddressInfo {
                address: Some(addr),
                script_type: ScriptType::P2SH,
            },
            Err(_) => AddressInfo {
                address: None,
                script_type: ScriptType::Unknown,
            },
        };
    }

    // Check for witness programs (SegWit v0, v1)
    if script.is_witness_program() {
        if let Some(version) = script.witness_version() {
            match version.to_num() {
                0 => {
                    // SegWit v0: P2WPKH (22 bytes total) or P2WSH (34 bytes total)
                    // Script is: <version> <program>
                    let script_type = match script.len() {
                        22 => ScriptType::P2WPKH,  // OP_0 + 20 byte program
                        34 => ScriptType::P2WSH,   // OP_0 + 32 byte program
                        _ => ScriptType::Unknown,
                    };

                    return match Address::from_script(script, network) {
                        Ok(addr) => AddressInfo {
                            address: Some(addr),
                            script_type,
                        },
                        Err(_) => AddressInfo {
                            address: None,
                            script_type: ScriptType::Unknown,
                        },
                    };
                }
                1 => {
                    // SegWit v1: P2TR (Taproot) - 34 bytes total
                    return match Address::from_script(script, network) {
                        Ok(addr) => AddressInfo {
                            address: Some(addr),
                            script_type: ScriptType::P2TR,
                        },
                        Err(_) => AddressInfo {
                            address: None,
                            script_type: ScriptType::Unknown,
                        },
                    };
                }
                _ => {
                    // Future witness versions (v2-v16)
                    return AddressInfo {
                        address: None,
                        script_type: ScriptType::Unknown,
                    };
                }
            }
        }
    }

    // Check for P2PK (obsolete - used in early blocks like Genesis)
    if let Some(address) = extract_p2pk_address(script, network) {
        return AddressInfo {
            address: Some(address),
            script_type: ScriptType::P2PK,
        };
    }

    // Unknown/non-standard script
    AddressInfo {
        address: None,
        script_type: ScriptType::Unknown,
    }
}

/// Extract address from P2PK (Pay to Public Key) script
///
/// P2PK is obsolete but appears in early Bitcoin blocks (including Genesis).
/// Pattern: `<pubkey> OP_CHECKSIG`
///
/// We derive a P2PKH-style address by hashing the public key.
fn extract_p2pk_address(script: &ScriptBuf, network: Network) -> Option<Address> {
    let instructions: Vec<_> = script.instructions().collect();

    // P2PK pattern: <pubkey> OP_CHECKSIG
    if instructions.len() != 2 {
        return None;
    }

    // First instruction should be a pubkey push
    let pubkey_bytes = match &instructions[0] {
        Ok(Instruction::PushBytes(bytes)) => bytes.as_bytes(),
        _ => return None,
    };

    // Second instruction should be OP_CHECKSIG
    match instructions[1] {
        Ok(Instruction::Op(op)) if op == bitcoin::opcodes::all::OP_CHECKSIG => {}
        _ => return None,
    }

    // Parse the public key
    let pubkey = PublicKey::from_slice(pubkey_bytes).ok()?;

    // Derive P2PKH address from the public key
    // This is what bitcoin addresses do - hash the pubkey and encode it
    let address = Address::p2pkh(&pubkey, network);

    Some(address)
}

// Comprehensive tests moved to tests/parser_tests.rs
// Only minimal inline tests here for doc examples

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::ScriptBuf;
    use hex;

    // Smoke tests - comprehensive tests in tests/parser_tests.rs

    #[test]
    fn test_genesis_p2pk() {
        let script_hex = "4104678afdb0fe5548271967f1a67130b7105cd6a828e03909a67962e0ea1f61deb649f6bc3f4cef38c4f35504e51ec112de5c384df7ba0b8d578a4c702b6bf11d5fac";
        let script = ScriptBuf::from(hex::decode(script_hex).unwrap());
        let info = extract_address(&script, Network::Bitcoin);

        assert_eq!(info.script_type, ScriptType::P2PK);
        assert_eq!(info.address.unwrap().to_string(), "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa");
    }

    #[test]
    fn test_all_script_types() {
        // P2PKH
        let script = ScriptBuf::from(hex::decode("76a91489abcdefabbaabbaabbaabbaabbaabbaabbaabba88ac").unwrap());
        assert_eq!(extract_address(&script, Network::Bitcoin).script_type, ScriptType::P2PKH);

        // P2SH
        let script = ScriptBuf::from(hex::decode("a91489abcdefabbaabbaabbaabbaabbaabbaabbaabba87").unwrap());
        assert_eq!(extract_address(&script, Network::Bitcoin).script_type, ScriptType::P2SH);

        // P2WPKH
        let script = ScriptBuf::from(hex::decode("0014751e76e8199196d454941c45d1b3a323f1433bd6").unwrap());
        assert_eq!(extract_address(&script, Network::Bitcoin).script_type, ScriptType::P2WPKH);

        // P2TR
        let script = ScriptBuf::from(hex::decode("512079be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap());
        assert_eq!(extract_address(&script, Network::Bitcoin).script_type, ScriptType::P2TR);

        // NULL_DATA
        let script = ScriptBuf::from(hex::decode("6a0548656c6c6f").unwrap());
        assert_eq!(extract_address(&script, Network::Bitcoin).script_type, ScriptType::NullData);
        assert!(extract_address(&script, Network::Bitcoin).address.is_none());
    }
}
