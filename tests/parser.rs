//! Parser module tests
//!
//! Organized into:
//! - block_reader: BlockFileReader unit tests
//! - address_extraction: Address derivation unit tests
//! - integration: Parser integration tests

#[path = "parser/block_reader.rs"]
mod block_reader;

#[path = "parser/address_extraction.rs"]
mod address_extraction;

#[path = "parser/integration.rs"]
mod integration;
