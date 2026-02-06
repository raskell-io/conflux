//! Authentication and authorization for Conflux API.
//!
//! This module provides:
//! - **API key authentication** — Hash-based verification of API keys
//! - **mTLS authentication** — Client certificate verification
//! - **Tower middleware** — Automatic authentication for all requests
//! - **Actor extractors** — Axum extractors for authenticated actors
//!
//! # Authentication Modes
//!
//! The API supports several authentication modes configured in `conflux.toml`:
//!
//! - `none` — No authentication (legacy mode)
//! - `api-key` — API key authentication only
//! - `mtls` — mTLS client certificate authentication only
//! - `both` — Accept either API key or mTLS
//!
//! # Configuration
//!
//! ```toml
//! [auth]
//! mode = "api-key"
//! allow_unauthenticated = false
//!
//! [auth.api_keys]
//! storage = "file"
//! file = ".conflux/api-keys.toml"
//! ```
//!
//! # API Keys File
//!
//! ```toml
//! [[keys]]
//! key_hash = "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
//! actor_id = "ci-pipeline"
//! actor_class = "pipeline"
//! display_name = "GitHub Actions CI"
//! ```
//!
//! # Usage
//!
//! ## Generating API Key Hashes
//!
//! ```rust,no_run
//! use conflux_api::auth::api_key::format_hash;
//!
//! let raw_key = "my-secret-api-key";
//! let hash = format_hash(raw_key);
//! // Use this hash in your api-keys.toml file
//! ```
//!
//! ## Extracting Actor in Handlers
//!
//! ```rust,no_run
//! use conflux_api::auth::{Actor, AuthorizedActor};
//! use axum::Json;
//!
//! // Simple extraction (just the actor ID)
//! async fn my_handler(actor: Actor) {
//!     println!("Request from: {}", actor.0.id);
//! }
//!
//! // Full extraction with auth metadata
//! async fn my_handler_full(auth: AuthorizedActor) {
//!     if auth.is_api_key_auth() {
//!         println!("Authenticated via API key");
//!     }
//! }
//! ```

pub mod api_key;
pub mod config;
pub mod extractor;
pub mod middleware;
pub mod mtls;

pub use api_key::{format_hash, hash_key, ApiKeyAuthenticator, AuthMethod, AuthenticatedActor};
pub use config::{ApiKeyConfig, AuthConfig, AuthMode, MtlsConfig};
pub use extractor::{Actor, AuthorizedActor};
pub use middleware::{AuthLayer, AuthMiddleware, API_KEY_HEADER};
pub use mtls::{ClientCertInfo, MtlsAuthenticator};
