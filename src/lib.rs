//! # Bitcoin Chain Graph
//!
//! High-performance Bitcoin blockchain ingestion system for Neo4j.
//!
//! This crate provides a robust, production-ready system for parsing Bitcoin Core
//! `.blk` files and ingesting blockchain data into Neo4j graph database for
//! financial crime investigation, blockchain forensics, and network analysis.
//!
//! ## Architecture
//!
//! The crate uses a clean three-layer architecture:
//!
//! - **Parser Layer** ([`parser`]): Reads Bitcoin Core `.blk` files and deserializes blocks
//!   using the `bitcoin` crate. Provides streaming API for memory-efficient processing.
//!
//! - **Domain Layer** ([`domain`]): Type-safe domain models that bridge the parser
//!   and database layers. Uses primitive types suitable for serialization and storage.
//!
//! - **Writer Layer** ([`writer`]): Database abstraction with trait-based design.
//!   Supports mock implementations for testing and Neo4j for production.
//!
//! ## Example: Basic Block Parsing
//!
//! ```no_run
//! use bitcoin_chain_graph::parser::BlockFileReader;
//! use bitcoin::Network;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Open a Bitcoin Core block file
//! let mut reader = BlockFileReader::new("blk00000.dat", Network::Bitcoin)?;
//!
//! // Stream blocks one at a time (low memory usage)
//! while let Some(block) = reader.next_block()? {
//!     println!("Block {}: {} transactions",
//!         block.block_hash(),
//!         block.txdata.len()
//!     );
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Example: Address Extraction
//!
//! ```no_run
//! use bitcoin_chain_graph::parser::{extract_address, BlockFileReader};
//! use bitcoin::Network;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut reader = BlockFileReader::new("blk00000.dat", Network::Bitcoin)?;
//! let block = reader.next_block()?.expect("No blocks in file");
//!
//! // Extract addresses from transaction outputs
//! for tx in &block.txdata {
//!     for output in &tx.output {
//!         let addr_info = extract_address(&output.script_pubkey, Network::Bitcoin);
//!         if let Some(address) = addr_info.address {
//!             println!("Found address: {} (type: {:?})",
//!                 address, addr_info.script_type);
//!         }
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Example: Domain Model Conversion
//!
//! ```no_run
//! use bitcoin_chain_graph::parser::BlockFileReader;
//! use bitcoin_chain_graph::domain::BlockData;
//! use bitcoin::Network;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut reader = BlockFileReader::new("blk00000.dat", Network::Bitcoin)?;
//! let block = reader.next_block()?.expect("No blocks in file");
//!
//! // Convert to domain model (suitable for database storage)
//! let block_data = BlockData::from_block(&block, 0);
//!
//! println!("Block: {}", block_data.hash);
//! println!("Timestamp: {}", block_data.timestamp);
//! println!("Transactions: {}", block_data.tx_count);
//! # Ok(())
//! # }
//! ```
//!
//! ## Features
//!
//! - **Streaming Parser**: Memory-efficient block-by-block processing
//! - **Address Derivation**: Supports all standard Bitcoin address types (P2PKH, P2SH, P2WPKH, P2WSH, P2TR, P2PK)
//! - **Type Safety**: Rich error types and Result-based error handling
//! - **Zero Unsafe**: 100% safe Rust code
//! - **Well Tested**: Comprehensive test coverage with integration tests
//! - **Documented**: Extensive inline documentation and design docs
//!
//! ## Documentation
//!
//! For detailed architecture and design information, see:
//! - [ARCHITECTURE.md](https://github.com/yourusername/bitcoin-chain-graph/blob/main/docs/ARCHITECTURE.md)
//! - [INGESTION_ARCHITECTURE.md](https://github.com/yourusername/bitcoin-chain-graph/blob/main/docs/INGESTION_ARCHITECTURE.md)
//! - [DATA_MODEL.md](https://github.com/yourusername/bitcoin-chain-graph/blob/main/docs/DATA_MODEL.md)
//!
//! ## Project Status
//!
//! Currently at Milestone 3 (M3) - Domain models complete. See [PLAN.md](https://github.com/yourusername/bitcoin-chain-graph/blob/main/PLAN.md)
//! for development roadmap and progress tracking.

pub mod config;
pub mod domain;
pub mod parser;
pub mod writer;
