//! Error types for the Writer layer
//!
//! Defines errors that can occur during database operations.

use thiserror::Error;

/// Errors that can occur during graph database operations
#[derive(Error, Debug)]
pub enum WriterError {
    #[error("Output not found: {0}")]
    OutputNotFound(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Query execution failed: {0}")]
    QueryFailed(String),

    #[error("Checkpoint operation failed: {0}")]
    CheckpointError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Database error: {0}")]
    DatabaseError(String),
}

/// Result type alias for writer operations
pub type Result<T> = std::result::Result<T, WriterError>;
