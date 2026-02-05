//! Error types for the store crate.

use thiserror::Error;

/// Errors that can occur during store operations.
#[derive(Debug, Error)]
pub enum StoreError {
    /// SQLite database error.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// JSON serialization/deserialization error.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Requested entity was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// Data integrity violation.
    #[error("integrity error: {0}")]
    Integrity(String),
}
