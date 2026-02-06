//! API error types.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use thiserror::Error;

/// Errors that can occur in the API.
#[derive(Debug, Error)]
pub enum ApiError {
    /// Missing required actor identity headers.
    #[error("missing actor identity: {0}")]
    MissingActor(String),

    /// Invalid actor class value.
    #[error("invalid actor class: {0}")]
    InvalidActorClass(String),

    /// Access denied (authorization failed).
    #[error("forbidden: {0}")]
    Forbidden(String),

    /// Entity not found.
    #[error("entity not found: {0}")]
    EntityNotFound(String),

    /// Operation validation failed.
    #[error("validation error: {0}")]
    Validation(String),

    /// Store operation failed.
    #[error("store error: {0}")]
    Store(#[from] conflux_store::StoreError),

    /// Core operation error.
    #[error("operation error: {0}")]
    Operation(#[from] conflux_core::OperationError),

    /// Git projection error.
    #[error("git error: {0}")]
    Git(#[from] conflux_git::GitError),

    /// JSON serialization error.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Internal server error.
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<conflux_rbac::DenialReason> for ApiError {
    fn from(reason: conflux_rbac::DenialReason) -> Self {
        ApiError::Forbidden(reason.message)
    }
}

/// Error response body.
#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
    code: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            ApiError::MissingActor(_) => (StatusCode::UNAUTHORIZED, "missing_actor"),
            ApiError::InvalidActorClass(_) => (StatusCode::BAD_REQUEST, "invalid_actor_class"),
            ApiError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            ApiError::EntityNotFound(_) => (StatusCode::NOT_FOUND, "entity_not_found"),
            ApiError::Validation(_) => (StatusCode::BAD_REQUEST, "validation_error"),
            ApiError::Store(e) => match e {
                conflux_store::StoreError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
                _ => (StatusCode::INTERNAL_SERVER_ERROR, "store_error"),
            },
            ApiError::Operation(_) => (StatusCode::BAD_REQUEST, "operation_error"),
            ApiError::Git(_) => (StatusCode::INTERNAL_SERVER_ERROR, "git_error"),
            ApiError::Serialization(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "serialization_error")
            }
            ApiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };

        let body = ErrorResponse {
            error: self.to_string(),
            code: code.to_string(),
        };

        (status, Json(body)).into_response()
    }
}
