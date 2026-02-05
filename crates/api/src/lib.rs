//! HTTP and gRPC API server for Conflux.
//!
//! Exposes the Conflux engine over HTTP (REST) and gRPC for machine actors.
//! Handles actor identity, intent tracking, and operation dispatch.
//!
//! # Overview
//!
//! The API crate provides:
//! - **HTTP REST API** (port 9400) — For human tooling, dashboards, simple integrations
//! - **Actor authentication** — Requires `X-Conflux-Actor` and `X-Conflux-Actor-Class` headers
//! - **Operation submission** — Submit typed operations with full attribution
//! - **State queries** — Get resolved document and entity state
//! - **Milestone projection** — Commit resolved state to git
//! - **Operation log** — Query operation history
//!
//! # Example
//!
//! ```rust,no_run
//! use conflux_api::{AppState, ServerConfig, run_server};
//! use conflux_schema::Schema;
//! use conflux_store::SqliteStore;
//!
//! #[tokio::main]
//! async fn main() {
//!     let schema = Schema {
//!         name: "example".into(),
//!         version: "1.0".into(),
//!         entities: Default::default(),
//!         environments: None,
//!     };
//!     let store = SqliteStore::open_in_memory().unwrap();
//!     let state = AppState::new(schema, store, "my-document");
//!
//!     let config = ServerConfig::new().with_http_port(9400);
//!     run_server(config, state).await.unwrap();
//! }
//! ```
//!
//! # HTTP Endpoints
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | GET | `/health` | Health check |
//! | POST | `/v1/ops` | Submit operation |
//! | POST | `/v1/ops/batch` | Submit multiple operations |
//! | GET | `/v1/state` | Get full document state |
//! | GET | `/v1/state/{entity}` | Get entity state |
//! | POST | `/v1/milestones` | Create milestone |
//! | GET | `/v1/milestones` | List milestones |
//! | GET | `/v1/milestones/{id}` | Get milestone details |
//! | GET | `/v1/log` | Operation log |
//! | GET | `/v1/log/{entity}` | Entity operation log |
//! | POST | `/v1/webhooks` | Register webhook |
//! | GET | `/v1/webhooks` | List webhooks |
//! | GET | `/v1/webhooks/{id}` | Get webhook |
//! | PUT | `/v1/webhooks/{id}` | Update webhook |
//! | DELETE | `/v1/webhooks/{id}` | Delete webhook |

pub mod auth;
pub mod dto;
pub mod error;
pub mod grpc;
pub mod handlers;
pub mod server;
pub mod state;
pub mod webhook;

pub use auth::Actor;
pub use error::ApiError;
pub use grpc::{pb, ConfluxGrpcService, StateChangeEvent};
pub use server::{build_router, run_server, ServerConfig, DEFAULT_HTTP_PORT};
pub use state::AppState;
pub use webhook::{
    ConflictPayload, GitOpsInvolvedObject, GitOpsMetadata, GitOpsPayload, GitOpsSource,
    Webhook, WebhookConfig, WebhookEventFilter, WebhookFormat, WebhookManager, WebhookPayload,
};
