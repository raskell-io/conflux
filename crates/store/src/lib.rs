//! Persistent state storage for Conflux.
//!
//! Provides an append-only operation log, periodic document snapshots,
//! and milestone metadata with pluggable storage backends.
//!
//! # Synchronous Backends
//!
//! - [`SqliteStore`] — SQLite backend (default, production)
//! - [`MemoryStore`] — In-memory backend (testing, embedded)
//!
//! # Async Backends (feature-gated)
//!
//! - [`PostgresStore`] — PostgreSQL backend (feature: `postgres`)
//! - [`DynamoStore`] — DynamoDB backend (feature: `dynamodb`)
//!
//! # Using the Store Trait
//!
//! ```ignore
//! use conflux_store::{Store, SqliteStore, MemoryStore};
//!
//! // Use concrete type
//! let store = SqliteStore::open("data.db")?;
//! store.append_operation("doc", &op)?;
//!
//! // Or use trait object for backend flexibility
//! let store: Box<dyn Store> = Box::new(MemoryStore::new());
//! store.append_operation("doc", &op)?;
//! ```
//!
//! # Using the AsyncStore Trait
//!
//! ```ignore
//! use conflux_store::{AsyncStore, PostgresStore};
//!
//! let store = PostgresStore::connect("postgres://localhost/conflux").await?;
//! store.append_operation("doc", &op).await?;
//! ```

pub mod error;
pub mod memory;
pub mod models;
pub mod query;
pub mod sqlite;
mod traits;

// Feature-gated async backends
#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(feature = "dynamodb")]
pub mod dynamo;

pub use error::StoreError;
pub use memory::MemoryStore;
pub use models::{StoredMilestone, StoredOperation, StoredSnapshot};
pub use query::OperationQuery;
pub use sqlite::SqliteStore;
pub use traits::Store;

// Export AsyncStore trait when any async backend is enabled
#[cfg(any(feature = "postgres", feature = "dynamodb"))]
pub use traits::AsyncStore;

// Export async backend implementations
#[cfg(feature = "postgres")]
pub use postgres::PostgresStore;

#[cfg(feature = "dynamodb")]
pub use dynamo::DynamoStore;

/// Type alias for a boxed store (owned, single-threaded).
pub type BoxStore = Box<dyn Store>;

/// Type alias for a shared store (thread-safe, reference-counted).
pub type ArcStore = std::sync::Arc<dyn Store>;

/// Type alias for a boxed async store (owned).
#[cfg(any(feature = "postgres", feature = "dynamodb"))]
pub type BoxAsyncStore = Box<dyn AsyncStore>;

/// Type alias for a shared async store (thread-safe, reference-counted).
#[cfg(any(feature = "postgres", feature = "dynamodb"))]
pub type ArcAsyncStore = std::sync::Arc<dyn AsyncStore>;
