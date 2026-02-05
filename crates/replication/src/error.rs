//! Error types for the replication crate.

use thiserror::Error;

/// Errors that can occur during replication operations.
#[derive(Debug, Error)]
pub enum ReplicationError {
    /// Failed to connect to a peer.
    #[error("failed to connect to peer '{peer}': {reason}")]
    ConnectionFailed { peer: String, reason: String },

    /// Peer disconnected unexpectedly.
    #[error("peer '{peer}' disconnected")]
    PeerDisconnected { peer: String },

    /// Operation serialization/deserialization failed.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Store error during replication.
    #[error("store error: {0}")]
    Store(#[from] conflux_store::StoreError),

    /// gRPC transport error.
    #[error("transport error: {0}")]
    Transport(#[from] tonic::transport::Error),

    /// gRPC status error.
    #[error("rpc error: {0}")]
    Rpc(#[from] tonic::Status),

    /// Invalid node ID.
    #[error("invalid node ID: {0}")]
    InvalidNodeId(String),

    /// Clock synchronization error.
    #[error("clock sync error: {0}")]
    ClockSync(String),

    /// Anti-entropy sync failed.
    #[error("sync failed with peer '{peer}': {reason}")]
    SyncFailed { peer: String, reason: String },

    /// Operation already exists (idempotent).
    #[error("operation '{operation_id}' already exists")]
    OperationExists { operation_id: String },

    /// No quorum available for leader election.
    #[error("no quorum: {reachable} of {total} peers reachable, need {required}")]
    NoQuorum {
        reachable: usize,
        total: usize,
        required: usize,
    },

    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(String),
}
