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

    /// Backend-specific error (for backends like PostgreSQL, DynamoDB, etc.).
    #[error("backend error: {0}")]
    Backend(String),

    /// PostgreSQL database error.
    #[cfg(feature = "postgres")]
    #[error("postgres error: {0}")]
    Postgres(#[from] sqlx_core::Error),

    /// DynamoDB error.
    #[cfg(feature = "dynamodb")]
    #[error("dynamodb error: {0}")]
    DynamoDB(String),
}

impl StoreError {
    /// Creates a backend error from any error type.
    pub fn backend<E: std::error::Error>(err: E) -> Self {
        StoreError::Backend(err.to_string())
    }

    /// Creates a DynamoDB error from any error type.
    #[cfg(feature = "dynamodb")]
    pub fn dynamodb<E: std::error::Error>(err: E) -> Self {
        StoreError::DynamoDB(err.to_string())
    }
}

#[cfg(feature = "dynamodb")]
impl<E, R> From<aws_sdk_dynamodb::error::SdkError<E, R>> for StoreError
where
    E: std::error::Error + Send + Sync + 'static,
    R: std::fmt::Debug,
{
    fn from(err: aws_sdk_dynamodb::error::SdkError<E, R>) -> Self {
        StoreError::DynamoDB(err.to_string())
    }
}

#[cfg(feature = "dynamodb")]
impl From<aws_sdk_dynamodb::error::BuildError> for StoreError {
    fn from(err: aws_sdk_dynamodb::error::BuildError) -> Self {
        StoreError::DynamoDB(err.to_string())
    }
}
