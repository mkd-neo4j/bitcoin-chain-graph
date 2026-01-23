# Bitcoin Binary Format Parsing

Implementation guide for parsing Bitcoin Core `.blk` files in Rust using the `bitcoin` crate.

---

## Overview

Bitcoin Core stores raw blockchain data in binary `.blk` files with a specific format:
- **Magic bytes** (4 bytes): Network identifier
- **Block size** (4 bytes): Size of following block data
- **Block data**: Serialized block (header + transactions)

This document covers how to parse these files efficiently in Rust.

---

## Block File Format

### File Structure

```
┌─────────────────────────────────────────────────────────────────┐
│ blk00000.dat                                                    │
├─────────────────────────────────────────────────────────────────┤
│ [Magic (4)] [Size (4)] [Block Data (Size bytes)]               │
│ [Magic (4)] [Size (4)] [Block Data (Size bytes)]               │
│ [Magic (4)] [Size (4)] [Block Data (Size bytes)]               │
│ ...                                                             │
└─────────────────────────────────────────────────────────────────┘
```

### Magic Bytes (Network Identifier)

| Network | Bytes (hex) | Bytes (dec) |
|---------|-------------|-------------|
| Mainnet | `F9 BE B4 D9` | `249 190 180 217` |
| Testnet | `0B 11 09 07` | `11 17 9 7` |
| Regtest | `FA BF B5 DA` | `250 191 181 218` |

### Block Data Structure

Each block contains:
1. **Block Header** (80 bytes)
   - Version (4 bytes)
   - Previous block hash (32 bytes)
   - Merkle root (32 bytes)
   - Timestamp (4 bytes)
   - Bits/difficulty (4 bytes)
   - Nonce (4 bytes)

2. **Transaction Count** (varint, 1-9 bytes)

3. **Transactions** (variable length)
   - Each transaction contains inputs, outputs, locktime, version

---

## Using the `bitcoin` Crate

### Key Types

```rust
use bitcoin::{Block, BlockHeader, Transaction, TxIn, TxOut, Script, Address};
use bitcoin::consensus::{Decodable, Encodable, deserialize};
use bitcoin::network::constants::Network;
```

### Block Deserialization

```rust
use bitcoin::{Block, consensus::deserialize};
use std::io::Read;

// Read block data from .blk file
let mut block_data = vec![0u8; block_size as usize];
file.read_exact(&mut block_data)?;

// Deserialize using bitcoin crate
let block: Block = deserialize(&block_data)?;

// Access block header
println!("Block hash: {}", block.block_hash());
println!("Previous hash: {}", block.header.prev_blockhash);
println!("Merkle root: {}", block.header.merkle_root);
println!("Timestamp: {}", block.header.time);

// Access transactions
for (i, tx) in block.txdata.iter().enumerate() {
    println!("Transaction {}: {}", i, tx.txid());
}
```

---

## Streaming Block File Parser Implementation

### Complete Implementation

```rust
use std::fs::File;
use std::io::{BufReader, Read, Result as IoResult};
use bitcoin::{Block, consensus::deserialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid magic bytes: expected {expected:X}, got {got:X}")]
    InvalidMagic { expected: u32, got: u32 },

    #[error("Bitcoin deserialization error: {0}")]
    Deserialize(#[from] bitcoin::consensus::encode::Error),

    #[error("Unexpected EOF")]
    UnexpectedEof,
}

pub struct BlockFileReader {
    reader: BufReader<File>,
    file_path: String,
    magic_bytes: [u8; 4],
    blocks_read: usize,
}

impl BlockFileReader {
    /// Create new block file reader
    pub fn new(path: &str, network: Network) -> Result<Self, ParseError> {
        let file = File::open(path)?;
        let reader = BufReader::with_capacity(8 * 1024 * 1024, file); // 8MB buffer

        let magic_bytes = match network {
            Network::Bitcoin => [0xF9, 0xBE, 0xB4, 0xD9],
            Network::Testnet => [0x0B, 0x11, 0x09, 0x07],
            Network::Regtest => [0xFA, 0xBF, 0xB5, 0xDA],
            _ => return Err(ParseError::InvalidMagic {
                expected: 0,
                got: 0,
            }),
        };

        Ok(Self {
            reader,
            file_path: path.to_string(),
            magic_bytes,
            blocks_read: 0,
        })
    }

    /// Read next block from file (returns None at EOF)
    pub fn next_block(&mut self) -> Result<Option<Block>, ParseError> {
        // Read magic bytes (4 bytes)
        let mut magic = [0u8; 4];
        match self.reader.read_exact(&mut magic) {
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(None); // End of file reached
            }
            Err(e) => return Err(ParseError::Io(e)),
        }

        // Verify magic bytes
        if magic != self.magic_bytes {
            let expected = u32::from_le_bytes(self.magic_bytes);
            let got = u32::from_le_bytes(magic);
            return Err(ParseError::InvalidMagic { expected, got });
        }

        // Read block size (4 bytes, little-endian)
        let mut size_bytes = [0u8; 4];
        self.reader.read_exact(&mut size_bytes)?;
        let block_size = u32::from_le_bytes(size_bytes) as usize;

        // Validate block size (sanity check)
        if block_size == 0 || block_size > 4_000_000 {
            // Max block size is ~4MB (after SegWit)
            return Err(ParseError::InvalidMagic {
                expected: 0,
                got: block_size as u32,
            });
        }

        // Read block data
        let mut block_data = vec![0u8; block_size];
        self.reader.read_exact(&mut block_data)?;

        // Deserialize block using bitcoin crate
        let block: Block = deserialize(&block_data)?;

        self.blocks_read += 1;

        Ok(Some(block))
    }

    /// Get number of blocks read so far
    pub fn blocks_read(&self) -> usize {
        self.blocks_read
    }

    /// Get file path
    pub fn file_path(&self) -> &str {
        &self.file_path
    }
}

// Usage example
use bitcoin::Network;

fn parse_block_file(path: &str) -> Result<Vec<Block>, ParseError> {
    let mut reader = BlockFileReader::new(path, Network::Bitcoin)?;
    let mut blocks = Vec::new();

    while let Some(block) = reader.next_block()? {
        blocks.push(block);
    }

    println!("Read {} blocks from {}", blocks.len(), path);
    Ok(blocks)
}
```

---

## Transaction Parsing

### Accessing Transaction Data

```rust
use bitcoin::{Transaction, TxIn, TxOut};

fn parse_transaction(tx: &Transaction) {
    println!("TXID: {}", tx.txid());
    println!("Version: {}", tx.version);
    println!("Locktime: {}", tx.lock_time);

    // Check if coinbase
    let is_coinbase = tx.is_coin_base();
    println!("Is coinbase: {}", is_coinbase);

    // Parse inputs
    println!("Inputs ({}):", tx.input.len());
    for (i, input) in tx.input.iter().enumerate() {
        parse_input(i, input);
    }

    // Parse outputs
    println!("Outputs ({}):", tx.output.len());
    for (i, output) in tx.output.iter().enumerate() {
        parse_output(i, output);
    }
}

fn parse_input(index: usize, input: &TxIn) {
    if input.previous_output.is_null() {
        // Coinbase input
        println!("  Input {}: COINBASE", index);
        println!("    Coinbase data: {}", hex::encode(&input.script_sig.to_bytes()));
    } else {
        // Regular input
        println!("  Input {}: {}", index, input.previous_output);
        println!("    Prev TXID: {}", input.previous_output.txid);
        println!("    Prev vout: {}", input.previous_output.vout);
        println!("    ScriptSig: {}", hex::encode(&input.script_sig.to_bytes()));
        println!("    Sequence: {}", input.sequence);

        // SegWit witness data
        if !input.witness.is_empty() {
            println!("    Witness items: {}", input.witness.len());
            for (j, witness_item) in input.witness.iter().enumerate() {
                println!("      Witness[{}]: {} bytes", j, witness_item.len());
            }
        }
    }
}

fn parse_output(index: usize, output: &TxOut) {
    println!("  Output {}:", index);
    println!("    Value: {} sat ({} BTC)", output.value, output.value as f64 / 100_000_000.0);
    println!("    ScriptPubKey: {}", hex::encode(&output.script_pubkey.to_bytes()));

    // Detect script type
    let script_type = detect_script_type(&output.script_pubkey);
    println!("    Script type: {:?}", script_type);

    // Derive address (if possible)
    if let Some(address) = derive_address(&output.script_pubkey, Network::Bitcoin) {
        println!("    Address: {}", address);
    }
}
```

---

## Address Derivation

### Using `bitcoin::Address`

```rust
use bitcoin::{Script, Address, Network};

/// Derive Bitcoin address from scriptPubKey
pub fn derive_address(script: &Script, network: Network) -> Option<Address> {
    // bitcoin crate can automatically parse standard scripts
    Address::from_script(script, network).ok()
}

/// Detect script type
#[derive(Debug, PartialEq)]
pub enum ScriptType {
    P2PKH,      // Pay to Public Key Hash
    P2SH,       // Pay to Script Hash
    P2WPKH,     // Pay to Witness Public Key Hash (SegWit v0)
    P2WSH,      // Pay to Witness Script Hash (SegWit v0)
    P2TR,       // Pay to Taproot (SegWit v1)
    P2PK,       // Pay to Public Key (obsolete)
    NullData,   // OP_RETURN
    Unknown,
}

pub fn detect_script_type(script: &Script) -> ScriptType {
    if script.is_p2pkh() {
        ScriptType::P2PKH
    } else if script.is_p2sh() {
        ScriptType::P2SH
    } else if script.is_v0_p2wpkh() {
        ScriptType::P2WPKH
    } else if script.is_v0_p2wsh() {
        ScriptType::P2WSH
    } else if script.is_v1_p2tr() {
        ScriptType::P2TR
    } else if script.is_op_return() {
        ScriptType::NullData
    } else if script.is_p2pk() {
        ScriptType::P2PK
    } else {
        ScriptType::Unknown
    }
}

// Usage
for output in &tx.output {
    if let Some(address) = derive_address(&output.script_pubkey, Network::Bitcoin) {
        println!("Address: {}", address);
        println!("Address type: {}", address.address_type().unwrap());
    } else {
        let script_type = detect_script_type(&output.script_pubkey);
        println!("Non-standard script: {:?}", script_type);
    }
}
```

---

## Variable-Length Integers (VarInt)

Bitcoin uses compact integer encoding (handled by `bitcoin` crate automatically):

| Value Range | Encoding |
|-------------|----------|
| 0-252 | 1 byte (value itself) |
| 253-65535 | 3 bytes (`0xFD` + 2-byte little-endian) |
| 65536-4294967295 | 5 bytes (`0xFE` + 4-byte little-endian) |
| 4294967296+ | 9 bytes (`0xFF` + 8-byte little-endian) |

**Note**: The `bitcoin` crate handles varint encoding/decoding automatically in `Decodable`/`Encodable` traits.

---

## Endianness Handling

Bitcoin uses **little-endian** encoding for multi-byte integers:

```rust
// Reading 4-byte little-endian integer
let mut bytes = [0u8; 4];
reader.read_exact(&mut bytes)?;
let value = u32::from_le_bytes(bytes); // Little-endian conversion

// Writing 4-byte little-endian integer
let bytes = value.to_le_bytes();
writer.write_all(&bytes)?;
```

**Note**: The `bitcoin` crate handles endianness automatically.

---

## Zero-Copy Parsing Patterns

### Borrowing Instead of Copying

```rust
// BAD: Multiple copies
fn process_transaction_bad(tx_data: Vec<u8>) -> Result<()> {
    let tx: Transaction = deserialize(&tx_data)?; // Copy 1
    let script_bytes = tx.output[0].script_pubkey.to_bytes(); // Copy 2
    let address = derive_address_from_bytes(script_bytes)?; // Copy 3
    Ok(())
}

// GOOD: Borrow references
fn process_transaction_good(tx: &Transaction) -> Result<()> {
    let script = &tx.output[0].script_pubkey; // Borrow, no copy
    let address = derive_address(script, Network::Bitcoin)?; // Pass by reference
    Ok(())
}
```

### Streaming Parser (No Full File Load)

```rust
// BAD: Load entire file into memory
let file_data = std::fs::read("/path/to/blk00000.dat")?; // 128 MB in memory
let mut cursor = std::io::Cursor::new(file_data);
// ... parse blocks

// GOOD: Stream blocks one at a time
let mut reader = BlockFileReader::new("/path/to/blk00000.dat", Network::Bitcoin)?;
while let Some(block) = reader.next_block()? {
    process_block(&block)?;
    // Block dropped here, memory freed
}
```

---

## Error Handling

### Common Parsing Errors

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Invalid magic bytes")]
    InvalidMagic,

    #[error("Invalid block size: {0}")]
    InvalidBlockSize(usize),

    #[error("Bitcoin deserialization error: {0}")]
    DeserializeError(#[from] bitcoin::consensus::encode::Error),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Unexpected EOF while reading {0}")]
    UnexpectedEof(String),
}

// Usage
match reader.next_block() {
    Ok(Some(block)) => process_block(&block)?,
    Ok(None) => println!("End of file reached"),
    Err(ParseError::InvalidMagic) => {
        eprintln!("Corrupted block file");
    },
    Err(e) => return Err(e),
}
```

---

## Complete Example: Parse All Blocks in Directory

```rust
use std::fs;
use std::path::Path;
use bitcoin::{Block, Network};

pub struct BlockDirectoryReader {
    files: Vec<String>,
    current_file_index: usize,
    current_reader: Option<BlockFileReader>,
    network: Network,
}

impl BlockDirectoryReader {
    pub fn new(blocks_dir: &str, network: Network) -> Result<Self, ParseError> {
        // Find all .blk files
        let mut files: Vec<String> = fs::read_dir(blocks_dir)?
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                if path.extension()? == "dat" &&
                   path.file_stem()?.to_str()?.starts_with("blk") {
                    Some(path.to_str()?.to_string())
                } else {
                    None
                }
            })
            .collect();

        // Sort files by name (blk00000.dat, blk00001.dat, ...)
        files.sort();

        Ok(Self {
            files,
            current_file_index: 0,
            current_reader: None,
            network,
        })
    }

    /// Read next block across all files
    pub fn next_block(&mut self) -> Result<Option<Block>, ParseError> {
        loop {
            // If no current reader, open next file
            if self.current_reader.is_none() {
                if self.current_file_index >= self.files.len() {
                    return Ok(None); // No more files
                }

                let file_path = &self.files[self.current_file_index];
                println!("Opening {}", file_path);
                self.current_reader = Some(BlockFileReader::new(file_path, self.network)?);
            }

            // Try to read from current file
            if let Some(reader) = &mut self.current_reader {
                match reader.next_block()? {
                    Some(block) => return Ok(Some(block)),
                    None => {
                        // End of current file, move to next
                        self.current_reader = None;
                        self.current_file_index += 1;
                    }
                }
            }
        }
    }
}

// Usage
fn main() -> Result<(), ParseError> {
    let blocks_dir = "/home/user/.bitcoin/blocks";
    let mut reader = BlockDirectoryReader::new(blocks_dir, Network::Bitcoin)?;

    let mut block_count = 0;
    while let Some(block) = reader.next_block()? {
        println!("Block {}: {} (height unknown, {} txs)",
                 block_count,
                 block.block_hash(),
                 block.txdata.len());

        block_count += 1;

        // Process first 1000 blocks only (for testing)
        if block_count >= 1000 {
            break;
        }
    }

    println!("Parsed {} blocks", block_count);
    Ok(())
}
```

---

## Performance Considerations

### Buffer Size Tuning

```rust
// Small buffer (slower I/O, less memory)
BufReader::with_capacity(1 * 1024 * 1024, file); // 1 MB

// Large buffer (faster I/O, more memory)
BufReader::with_capacity(16 * 1024 * 1024, file); // 16 MB

// Recommended: 8 MB (good balance)
BufReader::with_capacity(8 * 1024 * 1024, file); // 8 MB
```

### Deserialization Performance

```rust
// The bitcoin crate's deserialize is optimized, but if you need even more speed:
use bitcoin::consensus::Decodable;
use std::io::Cursor;

// Direct decoding (slightly faster than deserialize)
let mut cursor = Cursor::new(&block_data);
let block = Block::consensus_decode(&mut cursor)?;
```

---

## Testing Block Parser

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_genesis_block() {
        // Genesis block data (hardcoded for testing)
        let genesis_data = include_bytes!("../test_data/genesis.dat");

        let block: Block = deserialize(genesis_data).unwrap();

        assert_eq!(block.block_hash().to_string(),
                   "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f");
        assert_eq!(block.txdata.len(), 1);
        assert!(block.txdata[0].is_coin_base());
    }

    #[test]
    fn test_parse_block_with_segwit() {
        // Block with SegWit transactions
        let block_data = include_bytes!("../test_data/block_segwit.dat");
        let block: Block = deserialize(block_data).unwrap();

        // Check for witness data
        let has_witness = block.txdata.iter().any(|tx| {
            tx.input.iter().any(|input| !input.witness.is_empty())
        });

        assert!(has_witness);
    }

    #[test]
    fn test_detect_script_types() {
        use bitcoin::blockdata::script::Builder;
        use bitcoin::blockdata::opcodes;

        // P2PKH script
        let p2pkh = Builder::new()
            .push_opcode(opcodes::all::OP_DUP)
            .push_opcode(opcodes::all::OP_HASH160)
            .push_slice(&[0; 20])
            .push_opcode(opcodes::all::OP_EQUALVERIFY)
            .push_opcode(opcodes::all::OP_CHECKSIG)
            .into_script();

        assert_eq!(detect_script_type(&p2pkh), ScriptType::P2PKH);

        // OP_RETURN script
        let op_return = Builder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_slice(b"Hello Bitcoin")
            .into_script();

        assert_eq!(detect_script_type(&op_return), ScriptType::NullData);
    }
}
```

---

## References

- [rust-bitcoin Documentation](https://docs.rs/bitcoin/latest/bitcoin/)
- [Bitcoin Developer Reference - Block Format](https://developer.bitcoin.org/reference/block_chain.html)
- [Bitcoin Wiki - Block File Format](https://en.bitcoin.it/wiki/Blk.dat)
- [BIP-144: Segregated Witness](https://github.com/bitcoin/bips/blob/master/bip-0144.mediawiki)

---

## Next Steps

1. Read [NEO4J_INTEGRATION.md](NEO4J_INTEGRATION.md) for ingesting parsed blocks into Neo4j
2. Read [MEMORY_STRATEGY.md](MEMORY_STRATEGY.md) for memory-efficient parsing strategies
3. Read [TESTING.md](TESTING.md) for parser testing strategies
