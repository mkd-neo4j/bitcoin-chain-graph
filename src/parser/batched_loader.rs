//! Batched Block Loader
//!
//! Loads and sorts blocks from Bitcoin Core .blk files in batches for efficient processing.
//! Designed for backlog processing - loads entire files into memory, then sorts by blockchain height.

use bitcoin::{Block, Network, consensus::deserialize};
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use super::block_index::{BlockIndexReader, IndexError};

/// Result type for batched loading operations
pub type Result<T> = std::result::Result<T, LoaderError>;

/// Errors that can occur during batched block loading
#[derive(Debug)]
pub enum LoaderError {
    /// Failed to read block index
    IndexError(IndexError),
    /// I/O error reading .blk file
    Io(std::io::Error),
    /// Failed to deserialize block
    ParseError(String),
    /// Invalid file number or path
    InvalidFile(String),
}

impl From<IndexError> for LoaderError {
    fn from(err: IndexError) -> Self {
        LoaderError::IndexError(err)
    }
}

impl From<std::io::Error> for LoaderError {
    fn from(err: std::io::Error) -> Self {
        LoaderError::Io(err)
    }
}

impl From<bitcoin::consensus::encode::Error> for LoaderError {
    fn from(err: bitcoin::consensus::encode::Error) -> Self {
        LoaderError::ParseError(format!("Block deserialization failed: {}", err))
    }
}

impl fmt::Display for LoaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoaderError::IndexError(e) => write!(f, "Block index error: {:?}", e),
            LoaderError::Io(e) => write!(f, "I/O error: {}", e),
            LoaderError::ParseError(s) => write!(f, "Parse error: {}", s),
            LoaderError::InvalidFile(s) => write!(f, "Invalid file: {}", s),
        }
    }
}

impl std::error::Error for LoaderError {}

/// Batched block loader for backlog processing
///
/// Loads entire .blk files into memory, parses all blocks, and sorts by blockchain height
/// for efficient batch ingestion. Suitable for processing historical data where files are
/// complete and not actively being written.
pub struct BatchedBlockLoader {
    blocks_dir: PathBuf,
    network: Network,
}

impl BatchedBlockLoader {
    /// Create a new batched block loader
    ///
    /// # Arguments
    /// * `blocks_dir` - Path to directory containing .blk files and block index
    /// * `network` - Bitcoin network (determines magic bytes)
    pub fn new(blocks_dir: &str, network: Network) -> Self {
        Self {
            blocks_dir: PathBuf::from(blocks_dir),
            network,
        }
    }

    /// Load and sort blocks from specified .blk files
    ///
    /// This method:
    /// 1. Loads the block index to get blockchain height mappings
    /// 2. Loads complete .blk files into memory
    /// 3. Parses all blocks from memory
    /// 4. Sorts blocks by blockchain height
    /// 5. Returns sorted vector ready for batch processing
    ///
    /// # Arguments
    /// * `file_numbers` - File numbers to load (e.g., &[0, 1, 2, 3] for blk00000-blk00003)
    ///
    /// # Returns
    /// Vector of (height, block, file_name) tuples sorted by height
    ///
    /// # Errors
    /// Returns error if:
    /// - Block index cannot be read
    /// - .blk files cannot be opened or read
    /// - Block parsing fails
    pub fn load_files(&mut self, file_numbers: &[u32]) -> Result<Vec<(u32, Block, String)>> {
        println!("🔍 Loading block index...");

        // Load block index to get height mappings
        let mut index_reader = BlockIndexReader::new(
            self.blocks_dir.to_str()
                .ok_or_else(|| LoaderError::InvalidFile("Invalid blocks directory path".to_string()))?
        )?;

        let all_entries = index_reader.get_main_chain()?;
        println!("   ✅ Loaded {} blocks from index", all_entries.len());

        // Build HashMap: (file_number, file_offset) -> (height, hash)
        let mut offset_to_height: HashMap<(u32, u64), (u32, String)> = HashMap::new();
        for entry in &all_entries {
            // Note: Index stores data offset (after magic+size), subtract 8 for magic offset
            let magic_offset = entry.file_offset.saturating_sub(8);
            offset_to_height.insert(
                (entry.file_number, magic_offset),
                (entry.height, entry.hash.clone())
            );
        }

        println!("\n📂 Loading .blk files into memory...");

        let mut all_blocks: Vec<(u32, Block, String)> = Vec::new();

        for file_num in file_numbers {
            let file_name = format!("blk{:05}.dat", file_num);
            let file_path = self.blocks_dir.join(&file_name);

            println!("   Loading {}...", file_name);

            // Open and read entire file into memory
            let mut file = File::open(&file_path)
                .map_err(|e| LoaderError::InvalidFile(format!("Cannot open {}: {}", file_name, e)))?;

            let file_size = file.metadata()?.len();
            let mut buffer = Vec::with_capacity(file_size as usize);
            file.read_to_end(&mut buffer)?;

            println!("      Loaded {} MB into memory", file_size / 1_048_576);

            // Parse all blocks from memory
            let magic_bytes = self.get_magic_bytes();
            let blocks = self.parse_blocks_from_buffer(&buffer, &magic_bytes, *file_num)?;

            println!("      Parsed {} blocks", blocks.len());

            // Look up heights from index
            let mut blocks_with_heights = Vec::new();
            let mut missing_count = 0;

            for (offset, block) in blocks {
                if let Some((height, _hash)) = offset_to_height.get(&(*file_num, offset)) {
                    blocks_with_heights.push((*height, block, file_name.clone()));
                } else {
                    missing_count += 1;
                }
            }

            if missing_count > 0 {
                println!("      ⚠️  {} blocks not found in index (orphans or invalid)", missing_count);
            }

            all_blocks.extend(blocks_with_heights);
        }

        // Sort by blockchain height
        println!("\n🔀 Sorting {} blocks by height...", all_blocks.len());
        all_blocks.sort_by_key(|(height, _, _)| *height);
        println!("   ✅ Sorted from height {} to {}",
            all_blocks.first().map(|(h, _, _)| *h).unwrap_or(0),
            all_blocks.last().map(|(h, _, _)| *h).unwrap_or(0)
        );

        Ok(all_blocks)
    }

    /// Parse all blocks from a memory buffer
    ///
    /// Returns Vec of (file_offset, Block) - offset is where magic bytes start
    fn parse_blocks_from_buffer(
        &self,
        buffer: &[u8],
        magic_bytes: &[u8; 4],
        file_num: u32,
    ) -> Result<Vec<(u64, Block)>> {
        let mut blocks = Vec::new();
        let mut cursor = 0u64;

        while (cursor as usize) < buffer.len() {
            // Check for magic bytes
            if buffer.len() - (cursor as usize) < 8 {
                break; // Not enough bytes for magic + size
            }

            let magic_offset = cursor;
            let magic = &buffer[cursor as usize..cursor as usize + 4];

            if magic != magic_bytes {
                // Skip to next byte and look for magic
                cursor += 1;
                continue;
            }

            // Read block size
            cursor += 4;
            let size_bytes: [u8; 4] = buffer[cursor as usize..cursor as usize + 4]
                .try_into()
                .map_err(|_| LoaderError::ParseError("Failed to read block size".to_string()))?;
            let block_size = u32::from_le_bytes(size_bytes) as usize;
            cursor += 4;

            // Validate block size
            if block_size == 0 || block_size > 4_000_000 {
                return Err(LoaderError::ParseError(format!(
                    "Invalid block size {} in blk{:05}.dat at offset {}",
                    block_size, file_num, magic_offset
                )));
            }

            // Check if we have enough data
            if buffer.len() - (cursor as usize) < block_size {
                return Err(LoaderError::ParseError(format!(
                    "Truncated block in blk{:05}.dat at offset {} (expected {} bytes, got {})",
                    file_num, magic_offset, block_size, buffer.len() - (cursor as usize)
                )));
            }

            // Deserialize block
            let block_data = &buffer[cursor as usize..cursor as usize + block_size];
            let block: Block = deserialize(block_data)?;

            blocks.push((magic_offset, block));

            cursor += block_size as u64;
        }

        Ok(blocks)
    }

    /// Get magic bytes for current network
    fn get_magic_bytes(&self) -> [u8; 4] {
        match self.network {
            Network::Bitcoin => [0xF9, 0xBE, 0xB4, 0xD9],
            Network::Testnet => [0x0B, 0x11, 0x09, 0x07],
            Network::Signet => [0x0A, 0x03, 0xCF, 0x40],
            Network::Regtest => [0xFA, 0xBF, 0xB5, 0xDA],
            _ => [0xF9, 0xBE, 0xB4, 0xD9], // Default to mainnet
        }
    }
}
