//! Error types for the Writer layer
//!
//! Defines errors that can occur during database operations.

use thiserror::Error;

/// Errors that can occur during graph database operations
#[derive(Error, Debug, Clone)]
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

    /// Schema constraint violation (e.g., duplicate unique property)
    ///
    /// Non-retryable: deterministic failures should not be retried.
    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),

    #[error("Chain reorganization detected at height {height}: expected parent {expected}, got {actual}")]
    ReorgDetected {
        height: u32,
        expected: String,
        actual: String,
    },
}

impl WriterError {
    /// Whether this error is transient and safe to retry.
    ///
    /// Returns true for connection and query execution failures, which may be
    /// caused by transient issues like Neo4j cluster leader switches, network
    /// blips, or temporary resource exhaustion.
    ///
    /// Returns false for logic errors (OutputNotFound, SerializationError),
    /// schema errors (DatabaseError), and checkpoint errors, which are not
    /// expected to resolve by retrying.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            WriterError::QueryFailed(_) | WriterError::ConnectionFailed(_)
        )
    }
}

/// Result type alias for writer operations
pub type Result<T> = std::result::Result<T, WriterError>;
