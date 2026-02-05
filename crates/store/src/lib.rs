//! Persistent state storage for Conflux.
//!
//! Provides an append-only operation log, periodic document snapshots,
//! and milestone metadata backed by SQLite.

pub mod error;
pub mod models;
pub mod query;
pub mod sqlite;

pub use error::StoreError;
pub use models::{StoredMilestone, StoredOperation, StoredSnapshot};
pub use query::OperationQuery;
pub use sqlite::SqliteStore;
