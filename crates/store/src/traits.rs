//! Pluggable storage backend trait.

use crate::error::StoreError;
use crate::models::{StoredMilestone, StoredOperation, StoredSnapshot};
use crate::query::OperationQuery;
use conflux_core::{Document, HlcTimestamp, Operation};
use uuid::Uuid;

/// A pluggable storage backend for Conflux.
///
/// Implementors must be `Send + Sync` for thread-safe usage.
///
/// # Built-in Implementations
///
/// - [`SqliteStore`](crate::SqliteStore) — SQLite backend (default, production)
/// - [`MemoryStore`](crate::MemoryStore) — In-memory backend (testing, embedded)
pub trait Store: Send + Sync {
    // --- Operations (6 methods) ---

    /// Appends an operation to the log for a given document.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Serialization` if the operation can't be serialized,
    /// or a backend-specific error on insert failure.
    fn append_operation(&self, document_id: &str, operation: &Operation) -> Result<(), StoreError>;

    /// Retrieves a single operation by ID.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::NotFound` if the operation doesn't exist.
    fn get_operation(&self, operation_id: &Uuid) -> Result<StoredOperation, StoreError>;

    /// Queries operations using a fluent query builder.
    ///
    /// Results are ordered by HLC timestamp ascending.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error on query failure or
    /// `StoreError::Serialization` if stored payloads are corrupt.
    fn query_operations(&self, query: &OperationQuery) -> Result<Vec<StoredOperation>, StoreError>;

    /// Returns the number of operations for a document.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error on query failure.
    fn operation_count(&self, document_id: &str) -> Result<u64, StoreError>;

    /// Checks if an operation with the given ID already exists.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error on query failure.
    fn operation_exists(&self, operation_id: &Uuid) -> Result<bool, StoreError>;

    /// Queries operations by actor (node) since a given timestamp.
    ///
    /// This is useful for anti-entropy sync to find operations from a specific node
    /// that another node might be missing.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error on query failure.
    fn get_operations_by_actor_since(
        &self,
        document_id: &str,
        actor_id: &str,
        since: Option<&HlcTimestamp>,
    ) -> Result<Vec<StoredOperation>, StoreError>;

    // --- Snapshots (3 methods) ---

    /// Saves a document snapshot.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Serialization` if the document can't be serialized,
    /// or a backend-specific error on insert failure.
    fn save_snapshot(
        &self,
        document_id: &str,
        hlc_timestamp: &HlcTimestamp,
        document: &Document,
    ) -> Result<Uuid, StoreError>;

    /// Returns the latest snapshot for a document.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::NotFound` if no snapshots exist for the document.
    fn latest_snapshot(&self, document_id: &str) -> Result<StoredSnapshot, StoreError>;

    /// Returns the number of operations after the latest snapshot for a document.
    ///
    /// If no snapshots exist, returns the total operation count.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error on query failure.
    fn operations_since_snapshot(&self, document_id: &str) -> Result<u64, StoreError>;

    // --- Milestones (4 methods) ---

    /// Records a milestone for a document.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error on insert failure.
    fn record_milestone(
        &self,
        document_id: &str,
        git_commit: Option<&str>,
        hlc_range_start: &HlcTimestamp,
        hlc_range_end: &HlcTimestamp,
        message: Option<&str>,
    ) -> Result<Uuid, StoreError>;

    /// Returns the latest milestone for a document.
    ///
    /// Returns `None` if no milestones exist.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error on query failure.
    fn latest_milestone(&self, document_id: &str) -> Result<Option<StoredMilestone>, StoreError>;

    /// Lists all milestones for a document, newest first.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error on query failure.
    fn list_milestones(&self, document_id: &str) -> Result<Vec<StoredMilestone>, StoreError>;

    /// Returns the number of operations after the latest milestone for a document.
    ///
    /// If no milestones exist, returns the total operation count.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error on query failure.
    fn operations_since_milestone(&self, document_id: &str) -> Result<u64, StoreError>;

    // --- Version Vectors (2 methods) ---

    /// Saves a version vector for a document.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error on upsert failure.
    fn save_version_vector(
        &self,
        document_id: &str,
        version_vector_json: &str,
    ) -> Result<(), StoreError>;

    /// Loads a version vector for a document.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::NotFound` if no version vector exists for the document.
    fn load_version_vector(&self, document_id: &str) -> Result<String, StoreError>;
}
