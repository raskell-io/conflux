//! PostgreSQL-backed async storage for Conflux.
//!
//! This module provides a PostgreSQL implementation of the [`AsyncStore`](crate::AsyncStore) trait
//! using connection pooling via sqlx for production cluster deployments.
//!
//! # Example
//!
//! ```ignore
//! use conflux_store::PostgresStore;
//!
//! let store = PostgresStore::connect("postgres://user:pass@host/db").await?;
//! store.append_operation("doc-1", &op).await?;
//! ```

mod store;

pub use store::PostgresStore;
