//! Row types mapping between SQLite and core types.

use conflux_core::{Document, HlcTimestamp, Operation};
use uuid::Uuid;

/// An operation stored in the operation log, with storage metadata.
#[derive(Debug, Clone)]
pub struct StoredOperation {
    /// The core operation.
    pub operation: Operation,
    /// The document this operation belongs to.
    pub document_id: String,
    /// Wall-clock time the operation was stored.
    pub created_at: String,
}

/// A materialized document snapshot for fast reads.
#[derive(Debug, Clone)]
pub struct StoredSnapshot {
    /// Unique snapshot ID.
    pub id: Uuid,
    /// The document this snapshot belongs to.
    pub document_id: String,
    /// HLC timestamp at snapshot time.
    pub hlc_timestamp: HlcTimestamp,
    /// The materialized document state.
    pub document: Document,
    /// Wall-clock time the snapshot was created.
    pub created_at: String,
}

/// A milestone recording a git projection point.
#[derive(Debug, Clone)]
pub struct StoredMilestone {
    /// Unique milestone ID.
    pub id: Uuid,
    /// The document this milestone belongs to.
    pub document_id: String,
    /// Git commit SHA, if projected.
    pub git_commit: Option<String>,
    /// First operation HLC timestamp in this milestone's range.
    pub hlc_range_start: HlcTimestamp,
    /// Last operation HLC timestamp in this milestone's range.
    pub hlc_range_end: HlcTimestamp,
    /// Optional human-readable milestone message.
    pub message: Option<String>,
    /// Wall-clock time the milestone was recorded.
    pub created_at: String,
}
