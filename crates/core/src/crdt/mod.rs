//! CRDT primitives for per-field merge semantics.
//!
//! Each primitive implements the `CrdtMerge` trait and is designed to integrate
//! with Conflux's schema-aware merge model.

mod grow_set;
mod lww;
mod max;
mod min;
mod or_set;
mod review;

pub use grow_set::GrowOnlySet;
pub use lww::LwwRegister;
pub use max::MaxRegister;
pub use min::MinRegister;
pub use or_set::{ObservedRemoveSet, UniqueTag};
pub use review::ReviewRegister;

/// Trait for CRDT merge operations.
///
/// Implementations must satisfy:
/// - **Commutativity**: `a.merge(&b) == b.merge(&a)`
/// - **Associativity**: `a.merge(&b).merge(&c) == a.merge(&b.merge(&c))`
/// - **Idempotency**: `a.merge(&a) == a`
pub trait CrdtMerge {
    /// Merges two CRDT states, producing a new state that incorporates both.
    fn merge(&self, other: &Self) -> Self;
}
