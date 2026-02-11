//! Conversions from domain models to Neo4j BoltType format
//!
//! These functions convert our domain models into the format expected by neo4rs
//! for bulk operations with UNWIND.
//!
//! M7 updates: Added conversions for PerformsData and BenefitsToData, and updated
//! transaction_to_bolt_map to include amount fields calculated in Rust.

use crate::domain::{
    BenefitsToData, BlockData, InputData, OutputData, PerformsData, TransactionData,
};
use neo4rs::{BoltMap, BoltType};

#[cfg(test)]
use neo4rs::BoltString;

/// Convert BlockData to BoltMap for Neo4j
pub fn block_to_bolt_map(block: &BlockData) -> BoltMap {
    let mut map = BoltMap::new();
    map.put("height".into(), (block.height as i64).into());
    map.put("hash".into(), block.hash.as_str().into());
    map.put("previousHash".into(), block.previous_hash.as_str().into());
    map.put("merkleRoot".into(), block.merkle_root.as_str().into());
    map.put("timestamp".into(), block.timestamp.into());
    map.put("bits".into(), block.bits.as_str().into());
    map.put("difficulty".into(), block.difficulty.into());
    map.put("nonce".into(), (block.nonce as i64).into());
    map.put("version".into(), (block.version as i64).into());
    map.put("txCount".into(), (block.tx_count as i64).into());
    map.put("size".into(), (block.size as i64).into());
    map.put("weight".into(), (block.weight as i64).into());
    map
}

/// Convert slice of BlockData to Vec<BoltType>
pub fn blocks_to_bolt_list(blocks: &[BlockData]) -> Vec<BoltType> {
    blocks
        .iter()
        .map(|b| BoltType::Map(block_to_bolt_map(b)))
        .collect()
}

/// Convert TransactionData to BoltMap for Neo4j (M7 - with amounts)
pub fn transaction_to_bolt_map(tx: &TransactionData) -> BoltMap {
    let mut map = BoltMap::new();
    map.put("txid".into(), tx.txid.as_str().into());
    map.put("blockHeight".into(), (tx.block_height as i64).into());
    map.put("blockHash".into(), tx.block_hash.as_str().into());
    map.put("timestamp".into(), tx.timestamp.into());
    map.put("version".into(), (tx.version as i64).into());
    map.put("locktime".into(), (tx.locktime as i64).into());
    map.put("size".into(), (tx.size as i64).into());
    map.put("vsize".into(), (tx.vsize as i64).into());
    map.put("weight".into(), (tx.weight as i64).into());
    map.put("isCoinbase".into(), tx.is_coinbase.into());

    // M7: Add amount fields (calculated in Rust using UTXO cache)
    // These fields may be None if not yet calculated (shouldn't happen in normal flow)
    if let Some(total_input) = tx.total_input {
        map.put("totalInput".into(), (total_input as i64).into());
    } else {
        map.put("totalInput".into(), 0i64.into());
    }

    if let Some(total_output) = tx.total_output {
        map.put("totalOutput".into(), (total_output as i64).into());
    } else {
        map.put("totalOutput".into(), 0i64.into());
    }

    if let Some(fee) = tx.fee {
        map.put("fee".into(), (fee as i64).into());
    } else {
        map.put("fee".into(), 0i64.into());
    }

    map
}

/// Convert slice of TransactionData to Vec<BoltType>
pub fn transactions_to_bolt_list(transactions: &[TransactionData]) -> Vec<BoltType> {
    transactions
        .iter()
        .map(|tx| BoltType::Map(transaction_to_bolt_map(tx)))
        .collect()
}

/// Convert OutputData to BoltMap for Neo4j
pub fn output_to_bolt_map(output: &OutputData) -> BoltMap {
    let mut map = BoltMap::new();
    map.put("outputId".into(), output.output_id.as_str().into());
    map.put("outputIndex".into(), (output.output_index as i64).into());
    map.put("txid".into(), output.txid.as_str().into());
    map.put("amount".into(), (output.amount as i64).into());
    map.put("scriptPubKey".into(), output.script_pubkey.as_str().into());
    map.put("scriptType".into(), output.script_type.as_str().into());

    // Include address if present (for LOCKED_TO relationships)
    if let Some(ref address) = output.address {
        map.put("address".into(), address.as_str().into());
    }

    map
}

/// Convert slice of OutputData to Vec<BoltType>
pub fn outputs_to_bolt_list(outputs: &[OutputData]) -> Vec<BoltType> {
    outputs
        .iter()
        .map(|o| BoltType::Map(output_to_bolt_map(o)))
        .collect()
}

/// Convert slice of OutputData references to Vec<BoltType>
///
/// Avoids cloning OutputData when we already have references
/// (e.g., from filter_outputs_with_address).
pub fn output_refs_to_bolt_list(outputs: &[&OutputData]) -> Vec<BoltType> {
    outputs
        .iter()
        .map(|o| BoltType::Map(output_to_bolt_map(o)))
        .collect()
}

/// Filter outputs that have addresses (for LOCKED_TO relationships)
pub fn filter_outputs_with_address(outputs: &[OutputData]) -> Vec<&OutputData> {
    outputs.iter().filter(|o| o.address.is_some()).collect()
}

/// Convert InputData to BoltMap for Neo4j
///
/// Block height is read from `input.block_height` to correctly set
/// `spentAtHeight` on spent outputs in the Cypher query.
pub fn input_to_bolt_map(input: &InputData) -> BoltMap {
    let mut map = BoltMap::new();
    map.put("inputId".into(), input.input_id.as_str().into());
    map.put("inputIndex".into(), (input.input_index as i64).into());
    map.put("txid".into(), input.txid.as_str().into());
    map.put("previousTxid".into(), input.previous_txid.as_str().into());
    map.put(
        "previousOutputIndex".into(),
        (input.previous_output_index as i64).into(),
    );
    map.put("scriptSig".into(), input.script_sig.as_str().into());
    map.put("sequence".into(), (input.sequence as i64).into());

    // Convert witness Vec<String> to Vec<BoltType> for list
    let witness_list: Vec<BoltType> = input
        .witness
        .iter()
        .map(|w| BoltType::String(w.as_str().into()))
        .collect();
    map.put("witness".into(), BoltType::List(witness_list.into()));

    map.put("blockHeight".into(), (input.block_height as i64).into());
    map
}

/// Convert slice of InputData to Vec<BoltType>
pub fn inputs_to_bolt_list(inputs: &[InputData]) -> Vec<BoltType> {
    inputs
        .iter()
        .map(|input| BoltType::Map(input_to_bolt_map(input)))
        .collect()
}

// =============================================================================
// M7: PERFORMS AND BENEFITS_TO CONVERSIONS
// =============================================================================

/// Convert PerformsData to BoltMap for Neo4j (M7)
pub fn performs_to_bolt_map(p: &PerformsData) -> BoltMap {
    let mut map = BoltMap::new();
    map.put("fromAddress".into(), p.from_address.as_str().into());
    map.put("toTxid".into(), p.to_txid.as_str().into());
    map.put("inputCount".into(), (p.input_count as i64).into());
    map.put("amountSpent".into(), (p.amount_spent as i64).into());
    map
}

/// Convert slice of PerformsData to Vec<BoltType>
pub fn performs_to_bolt_list(performs: &[PerformsData]) -> Vec<BoltType> {
    performs
        .iter()
        .map(|p| BoltType::Map(performs_to_bolt_map(p)))
        .collect()
}

/// Convert BenefitsToData to BoltMap for Neo4j (M7)
pub fn benefits_to_to_bolt_map(b: &BenefitsToData) -> BoltMap {
    let mut map = BoltMap::new();
    map.put("fromTxid".into(), b.from_txid.as_str().into());
    map.put("toAddress".into(), b.to_address.as_str().into());
    map.put("outputCount".into(), (b.output_count as i64).into());
    map.put("amountReceived".into(), (b.amount_received as i64).into());
    map
}

/// Convert slice of BenefitsToData to Vec<BoltType>
pub fn benefits_to_to_bolt_list(benefits_to: &[BenefitsToData]) -> Vec<BoltType> {
    benefits_to
        .iter()
        .map(|b| BoltType::Map(benefits_to_to_bolt_map(b)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_conversion() {
        let block = BlockData {
            height: 0,
            hash: "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f".to_string(),
            previous_hash: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            merkle_root: "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b"
                .to_string(),
            timestamp: 1231006505,
            bits: "1d00ffff".to_string(),
            difficulty: 1.0,
            nonce: 2083236893,
            version: 1,
            tx_count: 1,
            size: 285,
            weight: 1140,
        };

        let map = block_to_bolt_map(&block);

        // Verify key properties were converted
        assert!(map.value.contains_key(&BoltString::from("height")));
        assert!(map.value.contains_key(&BoltString::from("hash")));
        assert!(map.value.contains_key(&BoltString::from("timestamp")));
    }

    #[test]
    fn test_transaction_conversion() {
        let tx = TransactionData {
            txid: "abc123".to_string(),
            block_height: 100,
            block_hash: "block_hash".to_string(),
            timestamp: 1234567890,
            version: 1,
            locktime: 0,
            size: 250,
            vsize: 125,
            weight: 500,
            is_coinbase: false,
            total_input: Some(1000000),
            total_output: Some(999000),
            fee: Some(1000),
        };

        let map = transaction_to_bolt_map(&tx);

        assert!(map.value.contains_key(&BoltString::from("txid")));
        assert!(map.value.contains_key(&BoltString::from("isCoinbase")));
        assert!(map.value.contains_key(&BoltString::from("totalInput")));
        assert!(map.value.contains_key(&BoltString::from("totalOutput")));
        assert!(map.value.contains_key(&BoltString::from("fee")));
    }

    // =========================================================================
    // AC 5: input_to_bolt_map includes previousOutputId, does NOT include blockHeight
    // =========================================================================

    #[test]
    fn ac5_input_bolt_map_includes_previous_output_id() {
        let input = InputData {
            input_id: "tx2:0".to_string(),
            input_index: 0,
            txid: "tx2".to_string(),
            previous_txid: "tx1".to_string(),
            previous_output_index: 3,
            script_sig: "4830450221...".to_string(),
            sequence: 0xFFFFFFFF,
            witness: vec![],
            block_height: 500,
        };

        let map = input_to_bolt_map(&input);

        // Should include previousOutputId with format "{previous_txid}:{previous_output_index}"
        assert!(
            map.value
                .contains_key(&BoltString::from("previousOutputId")),
            "input_to_bolt_map should include a previousOutputId field"
        );

        // Verify the value is "tx1:3"
        let previous_output_id = map.value.get(&BoltString::from("previousOutputId"));
        assert_eq!(
            previous_output_id,
            Some(&BoltType::String("tx1:3".into())),
            "previousOutputId should be formatted as 'previous_txid:previous_output_index'"
        );
    }

    #[test]
    fn ac5_input_bolt_map_does_not_include_block_height() {
        let input = InputData {
            input_id: "tx2:0".to_string(),
            input_index: 0,
            txid: "tx2".to_string(),
            previous_txid: "tx1".to_string(),
            previous_output_index: 3,
            script_sig: "4830450221...".to_string(),
            sequence: 0xFFFFFFFF,
            witness: vec![],
            block_height: 500,
        };

        let map = input_to_bolt_map(&input);

        // Should NOT include blockHeight since no Cypher query consumes it
        assert!(
            !map.value.contains_key(&BoltString::from("blockHeight")),
            "input_to_bolt_map should NOT include a blockHeight field"
        );
    }

    #[test]
    fn ac5_input_bolt_map_previous_output_id_coinbase() {
        // Coinbase input: previousOutputIndex = 0xFFFFFFFF
        let input = InputData {
            input_id: "coinbase_tx:0".to_string(),
            input_index: 0,
            txid: "coinbase_tx".to_string(),
            previous_txid: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            previous_output_index: 4294967295,
            script_sig: "coinbase_data".to_string(),
            sequence: 0xFFFFFFFF,
            witness: vec![],
            block_height: 100,
        };

        let map = input_to_bolt_map(&input);

        // Even for coinbase inputs, previousOutputId should be present in the map
        // (the WHERE clause in Cypher handles filtering, not the Rust conversion)
        assert!(
            map.value
                .contains_key(&BoltString::from("previousOutputId")),
            "input_to_bolt_map should include previousOutputId even for coinbase inputs"
        );
    }

    #[test]
    fn test_output_with_address_filtering() {
        let outputs = vec![
            OutputData {
                output_id: "tx1:0".to_string(),
                output_index: 0,
                txid: "tx1".to_string(),
                amount: 5000000000,
                script_pubkey: "76a914...".to_string(),
                script_type: "P2PKH".to_string(),
                address: Some("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa".to_string()),
            },
            OutputData {
                output_id: "tx1:1".to_string(),
                output_index: 1,
                txid: "tx1".to_string(),
                amount: 0,
                script_pubkey: "6a...".to_string(),
                script_type: "NULL_DATA".to_string(),
                address: None,
            },
        ];

        let with_address = filter_outputs_with_address(&outputs);
        assert_eq!(with_address.len(), 1);
        assert!(with_address[0].address.is_some());
    }
}
