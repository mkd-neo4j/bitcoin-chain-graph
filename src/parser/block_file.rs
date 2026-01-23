use bitcoin::{Block, consensus::deserialize, Network};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use thiserror::Error;

/// Magic bytes for different Bitcoin networks
const MAGIC_MAINNET: [u8; 4] = [0xF9, 0xBE, 0xB4, 0xD9];
const MAGIC_TESTNET: [u8; 4] = [0x0B, 0x11, 0x09, 0x07];
const MAGIC_REGTEST: [u8; 4] = [0xFA, 0xBF, 0xB5, 0xDA];

/// Default buffer size for block file reading (8MB)
const DEFAULT_BUFFER_SIZE: usize = 8 * 1024 * 1024;

/// Errors that can occur during block file parsing
#[derive(Error, Debug)]
pub enum ParseError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid magic bytes: expected {expected:X}, got {got:X}")]
    InvalidMagic { expected: u32, got: u32 },

    #[error("Unsupported network type")]
    UnsupportedNetwork,

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
    magic_bytes: &'static [u8; 4],
    blocks_read: usize,
}

impl BlockFileReader {
    /// Create a new block file reader with default buffer size
    ///
    /// # Arguments
    /// * `path` - Path to the `.blk` file
    /// * `network` - Bitcoin network (determines magic bytes to expect)
    ///
    /// # Returns
    /// A new reader or an error if the file cannot be opened
    pub fn new(path: &str, network: Network) -> Result<Self, ParseError> {
        Self::with_buffer_size(path, network, DEFAULT_BUFFER_SIZE)
    }

    /// Create a new block file reader with custom buffer size
    ///
    /// # Arguments
    /// * `path` - Path to the `.blk` file
    /// * `network` - Bitcoin network (determines magic bytes to expect)
    /// * `buffer_size` - Size of the read buffer in bytes
    ///
    /// # Returns
    /// A new reader or an error if the file cannot be opened
    ///
    /// # Example
    /// ```no_run
    /// use bitcoin_chain_graph::parser::BlockFileReader;
    /// use bitcoin::Network;
    ///
    /// // Use larger 16MB buffer for faster reading
    /// let mut reader = BlockFileReader::with_buffer_size(
    ///     "blk00000.dat",
    ///     Network::Bitcoin,
    ///     16 * 1024 * 1024
    /// ).unwrap();
    /// ```
    pub fn with_buffer_size(path: &str, network: Network, buffer_size: usize) -> Result<Self, ParseError> {
        let file = File::open(path)?;
        let reader = BufReader::with_capacity(buffer_size, file);

        let magic_bytes = match network {
            Network::Bitcoin => &MAGIC_MAINNET,
            Network::Testnet => &MAGIC_TESTNET,
            Network::Regtest => &MAGIC_REGTEST,
            _ => return Err(ParseError::UnsupportedNetwork),
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
        if magic != *self.magic_bytes {
            let expected = u32::from_le_bytes(*self.magic_bytes);
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

    /// Read block at specific file offset (for random access)
    ///
    /// This method seeks to a specific byte offset in the file and reads
    /// the block at that position. Used by OrderedBlockIterator to read
    /// blocks in blockchain height order rather than file order.
    ///
    /// # Arguments
    /// * `offset` - Byte offset in file where block data starts
    ///   (should point to magic bytes, not block header)
    ///
    /// # Returns
    /// - `Ok(Some(Block))` - Successfully read and deserialized the block
    /// - `Ok(None)` - Invalid offset or end of file
    /// - `Err(ParseError)` - Parse error
    ///
    /// # Example
    /// ```no_run
    /// use bitcoin_chain_graph::parser::BlockFileReader;
    /// use bitcoin::Network;
    ///
    /// let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    /// // Read genesis block at offset 8 (after file header)
    /// if let Some(block) = reader.read_block_at(8).unwrap() {
    ///     println!("Block hash: {}", block.block_hash());
    /// }
    /// ```
    pub fn read_block_at(&mut self, offset: u64) -> Result<Option<Block>, ParseError> {
        // Seek to specified offset
        self.reader.seek(SeekFrom::Start(offset))?;

        // Read magic bytes (4 bytes)
        let mut magic = [0u8; 4];
        match self.reader.read_exact(&mut magic) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(None); // End of file or invalid offset
            }
            Err(e) => return Err(ParseError::Io(e)),
        }

        // Verify magic bytes
        if magic != *self.magic_bytes {
            let expected = u32::from_le_bytes(*self.magic_bytes);
            let got = u32::from_le_bytes(magic);
            return Err(ParseError::InvalidMagic { expected, got });
        }

        // Read block size (4 bytes, little-endian)
        let mut size_bytes = [0u8; 4];
        self.reader.read_exact(&mut size_bytes)?;
        let block_size = u32::from_le_bytes(size_bytes) as usize;

        // Validate block size (sanity check)
        if block_size == 0 || block_size > 4_000_000 {
            return Err(ParseError::InvalidBlockSize(block_size));
        }

        // Read block data
        let mut block_data = vec![0u8; block_size];
        self.reader.read_exact(&mut block_data)?;

        // Deserialize block using bitcoin crate
        let block: Block = deserialize(&block_data)?;

        Ok(Some(block))
    }
}

// Comprehensive tests moved to tests/parser_tests.rs
// Only minimal inline tests here for doc examples

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_open_file() {
        // Minimal smoke test - detailed tests in tests/parser_tests.rs
        let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin)
            .expect("Failed to open test data file");

        let genesis = reader
            .next_block()
            .expect("Failed to read genesis block")
            .expect("Expected genesis block");

        assert_eq!(genesis.txdata.len(), 1);
        assert!(genesis.txdata[0].is_coinbase());
    }
}
