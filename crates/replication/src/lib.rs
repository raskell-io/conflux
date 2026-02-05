//! Multi-node replication for Conflux.
//!
//! This crate provides:
//! - Operation broadcast between nodes
//! - Causal anti-entropy for gap detection and sync
//! - Leader election for milestone projection
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐     gRPC      ┌─────────────────┐
//! │   Node A        │◄────────────►│   Node B        │
//! │  (Leader)       │               │  (Follower)     │
//! │                 │               │                 │
//! │ ┌─────────────┐ │               │ ┌─────────────┐ │
//! │ │ Document    │ │   Replicate   │ │ Document    │ │
//! │ │ (CRDT)      │◄───Operations──►│ │ (CRDT)      │ │
//! │ └─────────────┘ │               │ └─────────────┘ │
//! └─────────────────┘               └─────────────────┘
//! ```
//!
//! CRDTs guarantee convergence. Operations can arrive in any order and still
//! produce the same final state.

pub mod config;
pub mod error;
pub mod node;
pub mod version_vector;

// gRPC service modules
pub mod client;
pub mod service;

// Anti-entropy and leader election
pub mod anti_entropy;
pub mod leader;

// Re-exports
pub use client::{PeerManager, ReconnectConfig, ReconnectTask};
pub use config::{ClusterConfig, PeerConfig};
pub use error::ReplicationError;
pub use node::{ClusterRole, ClusterState, NodeId};
pub use version_vector::VersionVector;

// Generated protobuf types
pub mod proto {
    tonic::include_proto!("conflux.replication.v1");
}
