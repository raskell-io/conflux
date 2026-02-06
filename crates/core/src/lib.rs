//! Core CRDT document model for Conflux.
//!
//! This crate provides the fundamental data structures for schema-aware
//! config state coordination: typed operations, per-field merge semantics,
//! causal ordering via hybrid logical clocks, and deterministic convergence.
//!
//! # Architecture
//!
//! - **Document** — flat `HashMap<EntityId, Entity>` with O(1) lookup
//! - **Entity** — typed node with CRDT-backed fields, parent/child tree links
//! - **FieldState** — enum dispatching merge to the appropriate CRDT primitive
//! - **Operation** — typed mutation carrying actor identity and HLC timestamp
//!
//! # Merge Model
//!
//! Each field declares a merge strategy in the schema:
//! - `Lww` — last-writer-wins by HLC timestamp, actor tiebreak
//! - `Max` / `Min` — highest/lowest numeric value wins
//! - `GrowSet` — grow-only set (union, no removes)
//! - `OrSet` — observed-remove set (add-wins on concurrent add/remove)
//! - `Review` — LWW with concurrent write detection, flags for human review

pub mod clock;
pub mod crdt;
pub mod document;
pub mod entity;
pub mod error;
pub mod field;
pub mod field_state;
pub mod identity;
pub mod operation;
pub mod schema_info;
pub mod signing;

// Re-exports for convenient access
pub use clock::{Clock, HlcTimestamp};
pub use document::Document;
pub use entity::{ConflictInfo, Entity};
pub use error::{ClockError, ConfluxError, MergeError, OperationError, ValidationError};
pub use field::{ConflictState, FieldValue, MergeStrategy};
pub use field_state::FieldState;
pub use identity::{ActorClass, ActorId, EntityId};
pub use operation::{ApplyResult, Operation, OperationKind};
pub use schema_info::{FieldSchema, SchemaInfo, SimpleSchemaInfo};
pub use signing::{
    OperationSignature, PublicKeyRegistry, SignatureScheme, SigningError, SigningKey,
    VerificationKey,
};
