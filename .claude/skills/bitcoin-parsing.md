---
name: bitcoin-parsing
description: Bitcoin crate 0.31 patterns for block, transaction, and address parsing
---

# Bitcoin Parsing — bitcoin crate 0.31

## Block Parsing

```rust
use bitcoin::{Block, Network};
use bitcoin::consensus::deserialize;

// From raw bytes (e.g., memory-mapped .blk file)
let block: Block = deserialize(&bytes)?;

// Header fields
let hash = block.block_hash();           // BlockHash (display-reversed)
let prev = block.header.prev_blockhash;  // BlockHash
let time = block.header.time;            // u32 unix timestamp
let nonce = block.header.nonce;          // u32
let bits = block.header.bits;            // CompactTarget
let version = block.header.version;      // Version
let merkle = block.header.merkle_root;   // TxMerkleNode

// Transaction access
for tx in &block.txdata {
    let txid = tx.txid();                // Txid
    let is_cb = tx.is_coinbase();        // bool
    let size = tx.total_size();          // usize (bytes)
    let vsize = tx.vsize();              // usize (virtual bytes)
    let weight = tx.weight();            // Weight
}
```

## Transaction Outputs

```rust
for (idx, output) in tx.output.iter().enumerate() {
    let amount = output.value.to_sat();  // u64 satoshis
    let script = &output.script_pubkey;  // &ScriptBuf

    // Check script type
    script.is_p2pkh();          // Pay to Public Key Hash
    script.is_p2sh();           // Pay to Script Hash
    script.is_witness_program(); // SegWit (v0 or v1)
    script.is_op_return();      // OP_RETURN (unspendable)

    // Derive address
    use bitcoin::Address;
    let addr = Address::from_script(script, Network::Bitcoin);
}
```

## Transaction Inputs

```rust
for (idx, input) in tx.input.iter().enumerate() {
    let prev_txid = input.previous_output.txid;    // Txid
    let prev_vout = input.previous_output.vout;     // u32
    let script_sig = &input.script_sig;             // &ScriptBuf
    let sequence = input.sequence;                   // Sequence
    let witness = &input.witness;                    // &Witness

    // Coinbase detection
    if tx.is_coinbase() {
        // prev_txid is all zeros
        // prev_vout is 0xFFFFFFFF
        // script_sig contains coinbase data (block height, miner tag)
    }
}
```

## Key Types

| Type | Size | Display | Notes |
|------|------|---------|-------|
| `BlockHash` | 32 bytes | Reversed hex | `hash.to_string()` gives human-readable |
| `Txid` | 32 bytes | Reversed hex | Same display behavior |
| `OutPoint` | 36 bytes | txid:vout | References a specific output |
| `Amount` | 8 bytes | Wraps u64 | Use `.to_sat()` for raw satoshis |
| `ScriptBuf` | Variable | Hex | Owned script bytes |
| `Network` | Enum | — | Bitcoin, Testnet, Signet, Regtest |

## Conversions

```rust
// Display order (human-readable): reversed bytes
let hash_str = hash.to_string();          // "000000000019d6..."

// Internal bytes (network order)
let bytes = hash.to_byte_array();         // [u8; 32]

// From hex string (parses display order)
use std::str::FromStr;
let txid = Txid::from_str("4a5e1e...")?;

// Satoshis to BTC (for display only, never store as float)
let btc = amount.to_sat() as f64 / 100_000_000.0;
```

## .blk File Format

```
[magic: 4 bytes] [size: 4 bytes LE] [block_data: N bytes]
[magic: 4 bytes] [size: 4 bytes LE] [block_data: N bytes]
...
```

- Mainnet magic: `0xF9BEB4D9`
- Testnet magic: `0x0B110907`
- Size is 4-byte unsigned little-endian
- Block data is raw Bitcoin consensus encoding

## Critical Bitcoin Invariants

1. **Coinbase**: No real inputs. prev_txid = 0x00...00, prev_vout = 0xFFFFFFFF. Creates new coins (mining reward).
2. **Satoshis**: Always u64. 1 BTC = 100,000,000 satoshis. Never use floats for storage.
3. **Fee**: `total_input - total_output`. Use `u64::saturating_sub()` to avoid underflow.
4. **Same-block UTXO spending**: A transaction CAN spend outputs created by earlier transactions in the SAME block. This is why Phase 2 (outputs) must run before Phase 3 (transactions).
5. **BIP30 duplicates**: Blocks 91842 and 91880 contain coinbase transactions with txids identical to earlier blocks.
6. **P2PK scripts**: Used in genesis era. No standard `Address::from_script()` — must manually hash the public key to derive a P2PKH-style address.
7. **OP_RETURN (NullData)**: Unspendable outputs. No address. Amount is typically 0 but can be non-zero (burned coins).
8. **SegWit**: Witness data is separate from scriptSig. vsize < size for SegWit transactions.
