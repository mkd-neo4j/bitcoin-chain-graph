use bitcoin::{Block, consensus::deserialize, Network};
use std::fs::File;
use memmap2::Mmap;
use tracing::{debug, instrument, trace};
use thiserror::Error;

/// Magic bytes for different Bitcoin networks
const MAGIC_MAINNET: [u8; 4] = [0xF9, 0xBE, 0xB4, 0xD9];
const MAGIC_TESTNET: [u8; 4] = [0x0B, 0x11, 0x09, 0x07];
const MAGIC_REGTEST: [u8; 4] = [0xFA, 0xBF, 0xB5, 0xDA];



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
/// Uses memory-mapped I/O (mmap) for high-performance zero-copy reading.
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
    mmap: Mmap,
    cursor: usize,
    file_path: String,
    magic_bytes: &'static [u8; 4],
    blocks_read: usize,
}

impl BlockFileReader {
    /// Create a new block file reader using memory mapping
    ///
    /// # Arguments
    /// * `path` - Path to the `.blk` file
    /// * `network` - Bitcoin network (determines magic bytes to expect)
    ///
    /// # Returns
    /// A new reader or an error if the file cannot be opened
    pub fn new(path: &str, network: Network) -> Result<Self, ParseError> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };

        let magic_bytes = match network {
            Network::Bitcoin => &MAGIC_MAINNET,
            Network::Testnet => &MAGIC_TESTNET,
            Network::Regtest => &MAGIC_REGTEST,
            _ => return Err(ParseError::UnsupportedNetwork),
        };

        debug!(path, size = mmap.len(), "Opened block file with mmap");

        Ok(Self {
            mmap,
            cursor: 0,
            file_path: path.to_string(),
            magic_bytes,
            blocks_read: 0,
        })
    }

    /// Create a new block file reader with custom buffer size
    ///
    /// # Deprecated
    /// This method is deprecated as the implementation now uses mmap.
    /// It effectively calls `new()` and ignores the buffer size.
    pub fn with_buffer_size(path: &str, network: Network, _buffer_size: usize) -> Result<Self, ParseError> {
        Self::new(path, network)
    }

    /// Read next block from file
    ///
    /// Returns `None` when end of file is reached.
    ///
    /// # Returns
    /// - `Ok(Some(Block))` - Successfully read and deserialized a block
    /// - `Ok(None)` - End of file reached (normal termination)
    /// - `Err(ParseError)` - Parse error (corrupted file, wrong network, etc.)
    #[instrument(skip(self), level = "trace")]
    pub fn next_block(&mut self) -> Result<Option<Block>, ParseError> {
        if self.cursor >= self.mmap.len() {
            return Ok(None);
        }

        // Check if enough bytes remain for magic (4) + size (4)
        if self.cursor + 8 > self.mmap.len() {
            // If we're at the very end (exact match), it's EOF.
            // If we have trailing bytes but < 8, it's technically EOF or corrupt.
            // Bitcoin Core files usually pad with zeroes at the end.
            return Ok(None);
        }

        // Read magic bytes (4 bytes)
        let magic_slice = &self.mmap[self.cursor..self.cursor + 4];
        
        // Verify magic bytes
        if magic_slice != *self.magic_bytes {
            // Check for zero-padding at end of file (common in blk files)
            if magic_slice == [0, 0, 0, 0] {
                return Ok(None);
            }

            let expected = u32::from_le_bytes(*self.magic_bytes);
            let got = u32::from_le_bytes(magic_slice.try_into().unwrap());
            return Err(ParseError::InvalidMagic { expected, got });
        }
        self.cursor += 4;

        // Read block size (4 bytes, little-endian)
        let size_slice = &self.mmap[self.cursor..self.cursor + 4];
        let block_size = u32::from_le_bytes(size_slice.try_into().unwrap()) as usize;
        self.cursor += 4;

        // Validate block size (sanity check)
        if block_size == 0 || block_size > 4_000_000 {
            // Max block size is ~4MB (after SegWit)
            return Err(ParseError::InvalidBlockSize(block_size));
        }

        // Check if enough data follows
        if self.cursor + block_size > self.mmap.len() {
            return Err(ParseError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "File ended before block data complete",
            )));
        }

        // Read block data
        let block_data = &self.mmap[self.cursor..self.cursor + block_size];
        
        // Deserialize block using bitcoin crate (zero-copy from mmap slice)
        let block: Block = deserialize(block_data)?;

        self.cursor += block_size;
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
    /// This method reads from the memory map at a specific byte offset.
    ///
    /// # Arguments
    /// * `offset` - Byte offset in file where block data starts
    ///   (should point to magic bytes, not block header)
    ///
    /// # Returns
    /// - `Ok(Some(Block))` - Successfully read and deserialized the block
    /// - `Ok(None)` - Invalid offset or end of file
    /// - `Err(ParseError)` - Parse error
    pub fn read_block_at(&mut self, offset: u64) -> Result<Option<Block>, ParseError> {
        let offset = offset as usize;
        
        if offset >= self.mmap.len() {
            return Ok(None);
        }

        // Verify enough bytes for header
        if offset + 8 > self.mmap.len() {
            return Ok(None);
        }

        // Read magic bytes
        let magic_slice = &self.mmap[offset..offset + 4];
        if magic_slice != *self.magic_bytes {
             let expected = u32::from_le_bytes(*self.magic_bytes);
             let got = u32::from_le_bytes(magic_slice.try_into().unwrap());
             return Err(ParseError::InvalidMagic { expected, got });
        }

        // Read size
        let size_slice = &self.mmap[offset + 4..offset + 8];
        let block_size = u32::from_le_bytes(size_slice.try_into().unwrap()) as usize;

        if block_size == 0 || block_size > 4_000_000 {
            return Err(ParseError::InvalidBlockSize(block_size));
        }

        if offset + 8 + block_size > self.mmap.len() {
             return Err(ParseError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "File ended before block data complete",
            )));
        }

        let block_data = &self.mmap[offset + 8..offset + 8 + block_size];
        let block: Block = deserialize(block_data)?;

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
