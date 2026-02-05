//! Error types for the git crate.

use thiserror::Error;

/// Errors that can occur during git milestone operations.
#[derive(Debug, Error)]
pub enum GitError {
    /// I/O error during file operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization error.
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    /// YAML serialization error.
    #[error("YAML serialization error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// TOML serialization error.
    #[error("TOML serialization error: {0}")]
    Toml(#[from] toml::ser::Error),

    /// Git operation error.
    #[error("git error: {0}")]
    Git(String),

    /// Store operation error.
    #[error("store error: {0}")]
    Store(#[from] conflux_store::StoreError),

    /// Invalid output format.
    #[error("unknown output format: {0}")]
    UnknownFormat(String),

    /// Repository not found or not initialized.
    #[error("repository not found: {0}")]
    RepoNotFound(String),

    /// No changes to commit.
    #[error("no changes to commit")]
    NothingToCommit,

    /// Serialization error from custom serializer.
    #[error("serialization error: {0}")]
    SerializationError(String),
}
