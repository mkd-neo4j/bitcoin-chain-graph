use bitcoin::{Block, consensus::deserialize, Network};
use std::fs::File;
use std::io::{BufReader, Read};
use thiserror::Error;

/// Errors that can occur during block file parsing
#[derive(Error, Debug)]
pub enum ParseError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid magic bytes: expected {expected:X}, got {got:X}")]
    InvalidMagic { expected: u32, got: u32 },

    #[error("Bitcoin deserialization error: {0}")]
    Deserialize(#[from] bitcoin::consensus::encode::Error),

    #[error("Invalid block size: {0}")]
    InvalidBlockSize(usize),
}

/// Streaming reader for Bitcoin Core `.blk` files
///
/// Reads and deserializes blocks one at a time from Bitcoin Core's raw block files.
/// Uses buffered I/O and the `bitcoin` crate for zero-copy parsing.
///
/// # Example
/// ```no_run
/// use bitcoin_chain_graph::parser::BlockFileReader;
/// use bitcoin::Network;
///
/// let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
/// while let Some(block) = reader.next_block().unwrap() {
///     println!("Block hash: {}", block.block_hash());
/// }
/// ```
pub struct BlockFileReader {
    reader: BufReader<File>,
    file_path: String,
    magic_bytes: [u8; 4],
    blocks_read: usize,
}

impl BlockFileReader {
    /// Create a new block file reader
    ///
    /// # Arguments
    /// * `path` - Path to the `.blk` file
    /// * `network` - Bitcoin network (determines magic bytes to expect)
    ///
    /// # Returns
    /// A new reader or an error if the file cannot be opened
    pub fn new(path: &str, network: Network) -> Result<Self, ParseError> {
        let file = File::open(path)?;
        let reader = BufReader::with_capacity(8 * 1024 * 1024, file); // 8MB buffer

        let magic_bytes = match network {
            Network::Bitcoin => [0xF9, 0xBE, 0xB4, 0xD9],
            Network::Testnet => [0x0B, 0x11, 0x09, 0x07],
            Network::Regtest => [0xFA, 0xBF, 0xB5, 0xDA],
            _ => {
                return Err(ParseError::InvalidMagic {
                    expected: 0,
                    got: 0,
                })
            }
        };

        Ok(Self {
            reader,
            file_path: path.to_string(),
            magic_bytes,
            blocks_read: 0,
        })
    }

    /// Read next block from file
    ///
    /// Returns `None` when end of file is reached.
    ///
    /// # Returns
    /// - `Ok(Some(Block))` - Successfully read and deserialized a block
    /// - `Ok(None)` - End of file reached (normal termination)
    /// - `Err(ParseError)` - Parse error (corrupted file, wrong network, etc.)
    pub fn next_block(&mut self) -> Result<Option<Block>, ParseError> {
        // Read magic bytes (4 bytes)
        let mut magic = [0u8; 4];
        match self.reader.read_exact(&mut magic) {
            Ok(_) => {}
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
            return Err(ParseError::InvalidBlockSize(block_size));
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(
            result.is_err(),
            "Should error when file doesn't exist"
        );
    }
}
