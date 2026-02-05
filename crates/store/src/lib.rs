//! Persistent state storage for Conflux.
//!
//! Provides an append-only operation log, periodic document snapshots,
//! and milestone metadata with pluggable storage backends.
//!
//! # Built-in Backends
//!
//! - [`SqliteStore`] — SQLite backend (default, production)
//! - [`MemoryStore`] — In-memory backend (testing, embedded)
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

pub mod error;
pub mod memory;
pub mod models;
pub mod query;
pub mod sqlite;
mod traits;

pub use error::StoreError;
pub use memory::MemoryStore;
pub use models::{StoredMilestone, StoredOperation, StoredSnapshot};
pub use query::OperationQuery;
pub use sqlite::SqliteStore;
pub use traits::Store;

/// Type alias for a boxed store (owned, single-threaded).
pub type BoxStore = Box<dyn Store>;

/// Type alias for a shared store (thread-safe, reference-counted).
pub type ArcStore = std::sync::Arc<dyn Store>;
