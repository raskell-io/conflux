//! RBAC error types.

use thiserror::Error;

/// Errors that can occur during RBAC operations.
#[derive(Debug, Error)]
pub enum RbacError {
    /// Configuration file could not be read.
    #[error("failed to read RBAC config: {0}")]
    ConfigRead(std::io::Error),

    /// Configuration file could not be parsed.
    #[error("failed to parse RBAC config: {0}")]
    ConfigParse(toml::de::Error),

    /// Invalid resource pattern syntax.
    #[error("invalid resource pattern: {0}")]
    InvalidPattern(String),

    /// Unknown role referenced.
    #[error("unknown role: {0}")]
    UnknownRole(String),

    /// Circular role inheritance detected.
    #[error("circular role inheritance detected: {0}")]
    CircularInheritance(String),
}

impl From<std::io::Error> for RbacError {
    fn from(e: std::io::Error) -> Self {
        RbacError::ConfigRead(e)
    }
}

impl From<toml::de::Error> for RbacError {
    fn from(e: toml::de::Error) -> Self {
        RbacError::ConfigParse(e)
    }
}
