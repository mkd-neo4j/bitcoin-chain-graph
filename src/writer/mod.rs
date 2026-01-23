//! Writer layer - Database abstraction for blockchain ingestion
//!
//! Provides the GraphWriter trait and implementations for persisting blockchain
//! data to graph databases. This layer is isolated from database-specific
//! concerns, enabling testing without Neo4j and future database support.
//!
//! # Architecture
//! - `traits::GraphWriter` - Database abstraction trait
//! - `mock::MockWriter` - In-memory implementation for testing
//! - `error::WriterError` - Error types for writer operations
//!
//! # Example
//! ```
//! use bitcoin_chain_graph::writer::{GraphWriter, MockWriter};
//!
//! #[tokio::main]
//! async fn main() {
//!     let writer = MockWriter::new();
//!     writer.init_schema().await.unwrap();
//! }
//! ```

mod error;
mod traits;
pub mod mock;

pub use error::{WriterError, Result};
pub use traits::GraphWriter;
pub use mock::MockWriter;
