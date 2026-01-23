//! Conversion functions from bitcoin crate types to domain models
//!
//! These functions form the boundary between the Parser layer (bitcoin crate types)
//! and the Domain layer (our domain models). They extract all necessary data and
//! perform any required transformations.

use bitcoin::{Block, Transaction, TxIn, TxOut, Network};
use crate::parser::{extract_address, ScriptType};
use super::models::{BlockData, TransactionData, OutputData, InputData};

impl BlockData {
    /// Convert a bitcoin::Block to BlockData
    ///
    /// # Arguments
    /// * `block` - The block from bitcoin crate
    /// * `height` - Block height (must be provided, not in block data)
    ///
    /// # Returns
    /// BlockData with all properties extracted
    pub fn from_block(block: &Block, height: u32) -> Self {
        let header = &block.header;

        BlockData {
            height,
            hash: block.block_hash().to_string(),
            previous_hash: header.prev_blockhash.to_string(),
            merkle_root: header.merkle_root.to_string(),
            timestamp: header.time as i64,
            bits: format!("{:08x}", header.bits.to_consensus()),
            difficulty: difficulty_from_bits(header.bits.to_consensus()),
            nonce: header.nonce,
            version: header.version.to_consensus(),
            tx_count: block.txdata.len() as u32,
            size: block.total_size() as u64,
            weight: block.weight().to_wu(),
        }
    }
}

impl TransactionData {
    /// Convert a bitcoin::Transaction to TransactionData
    ///
    /// # Arguments
    /// * `tx` - The transaction from bitcoin crate
    /// * `block_height` - Block height containing this transaction
    /// * `block_hash` - Hash of containing block
    /// * `timestamp` - Block timestamp
    ///
    /// # Returns
    /// TransactionData with all properties extracted
    ///
    /// # Note
    /// total_input, total_output, and fee are initialized as None and will be
    /// calculated in Phase 5 using the UTXO cache (Rust-based calculation).
    pub fn from_transaction(
        tx: &Transaction,
        block_height: u32,
        block_hash: &str,
        timestamp: i64,
    ) -> Self {
        TransactionData {
            txid: tx.txid().to_string(),
            block_height,
            block_hash: block_hash.to_string(),
            timestamp,
            version: tx.version.0,
            locktime: tx.lock_time.to_consensus_u32(),
            size: tx.total_size(),
            vsize: tx.vsize(),
            weight: tx.weight().to_wu() as usize,
            is_coinbase: tx.is_coinbase(),
            total_input: None,
            total_output: None,
            fee: None,
        }
    }
}

impl OutputData {
    /// Convert a bitcoin::TxOut to OutputData
    ///
    /// # Arguments
    /// * `output` - The transaction output from bitcoin crate
    /// * `txid` - Transaction ID this output belongs to
    /// * `output_index` - Position in transaction outputs (0, 1, 2...)
    /// * `network` - Bitcoin network (for address derivation)
    ///
    /// # Returns
    /// OutputData with address extracted from scriptPubKey
    ///
    /// # Note
    /// Uses parser::extract_address() to derive address and script type.
    /// Returns None for address if script is NULL_DATA or UNKNOWN.
    pub fn from_output(
        output: &TxOut,
        txid: &str,
        output_index: u32,
        network: Network,
    ) -> Self {
        // Extract address and script type using parser
        let addr_info = extract_address(&output.script_pubkey, network);

        let script_type = match addr_info.script_type {
            ScriptType::P2PKH => "P2PKH",
            ScriptType::P2SH => "P2SH",
            ScriptType::P2WPKH => "P2WPKH",
            ScriptType::P2WSH => "P2WSH",
            ScriptType::P2TR => "P2TR",
            ScriptType::P2PK => "P2PK",
            ScriptType::NullData => "NULL_DATA",
            ScriptType::Unknown => "UNKNOWN",
        };

        OutputData {
            output_id: format!("{}:{}", txid, output_index),
            output_index,
            txid: txid.to_string(),
            amount: output.value.to_sat(),
            script_pubkey: output.script_pubkey.to_hex_string(),
            script_type: script_type.to_string(),
            address: addr_info.address.map(|a| a.to_string()),
        }
    }
}

impl InputData {
    /// Convert a bitcoin::TxIn to InputData
    ///
    /// # Arguments
    /// * `input` - The transaction input from bitcoin crate
    /// * `txid` - Transaction ID this input belongs to
    /// * `input_index` - Position in transaction inputs (0, 1, 2...)
    ///
    /// # Returns
    /// InputData with all properties extracted
    ///
    /// # Special Cases
    /// For coinbase inputs:
    /// - previous_txid will be all zeros (0000...0000)
    /// - previous_output_index will be 0xFFFFFFFF
    /// - This is handled naturally by the bitcoin crate
    pub fn from_input(
        input: &TxIn,
        txid: &str,
        input_index: u32,
    ) -> Self {
        // Convert witness data to hex strings
        let witness: Vec<String> = input
            .witness
            .iter()
            .map(hex::encode)
            .collect();

        InputData {
            input_id: format!("{}:{}", txid, input_index),
            input_index,
            txid: txid.to_string(),
            previous_txid: input.previous_output.txid.to_string(),
            previous_output_index: input.previous_output.vout,
            script_sig: input.script_sig.to_hex_string(),
            sequence: input.sequence.0,
            witness,
        }
    }
}

/// Calculate difficulty from bits (compact target format)
///
/// Difficulty = max_target / current_target
/// where max_target = 0x1d00ffff (difficulty 1)
fn difficulty_from_bits(bits: u32) -> f64 {
    // Max target (difficulty 1): 0x1d00ffff
    const MAX_BITS: u32 = 0x1d00ffff;

    // Extract exponent and mantissa from compact format
    let max_exp = MAX_BITS >> 24;
    let max_mant = MAX_BITS & 0xffffff;

    let curr_exp = bits >> 24;
    let curr_mant = bits & 0xffffff;

    // difficulty = (max_mant * 2^(8*(max_exp-3))) / (curr_mant * 2^(8*(curr_exp-3)))
    // Simplified: difficulty = max_mant / curr_mant * 2^(8*(max_exp - curr_exp))

    let base_diff = max_mant as f64 / curr_mant as f64;
    let exp_diff = (max_exp as i32 - curr_exp as i32) * 8;

    base_diff * 2f64.powi(exp_diff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_difficulty_calculation() {
        // Genesis block bits: 0x1d00ffff (difficulty 1.0)
        let genesis_bits = 0x1d00ffff;
        let diff = difficulty_from_bits(genesis_bits);
        assert!((diff - 1.0).abs() < 0.01, "Genesis difficulty should be ~1.0");

        // Higher difficulty example (more zeros in target)
        let high_bits = 0x1b0404cb; // Example from later blocks
        let diff2 = difficulty_from_bits(high_bits);
        assert!(diff2 > 1.0, "Higher difficulty should be > 1.0");
    }

    // Note: Genesis block conversion is already tested in tests/ingestion.rs
    // using real .blk files, so no need for a redundant hex-based test here
}
