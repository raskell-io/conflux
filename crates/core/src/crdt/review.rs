//! Review Register — LWW with concurrent write detection.
//!
//! When two actors concurrently write to the same field, the register
//! picks a winner via LWW but flags the field as `NeedsReview` so a
//! human can verify the resolution.

use crate::clock::HlcTimestamp;
use crate::crdt::CrdtMerge;
use crate::field::{ConflictState, FieldValue};
use crate::identity::ActorId;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// A register that detects concurrent writes and flags them for review.
///
/// The winner is selected via LWW, but if both values were written without
/// having seen each other (neither timestamp dominates), the field is marked
/// `NeedsReview`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRegister {
    pub value: FieldValue,
    pub timestamp: HlcTimestamp,
    pub actor: ActorId,
    pub conflict: ConflictState,
    /// Pending writes that have been observed (for concurrency detection).
    pub pending_writes: Vec<(FieldValue, HlcTimestamp, ActorId)>,
}

impl ReviewRegister {
    pub fn new(value: FieldValue, timestamp: HlcTimestamp, actor: ActorId) -> Self {
        Self {
            value,
            timestamp,
            actor,
            conflict: ConflictState::Resolved,
            pending_writes: Vec::new(),
        }
    }

    fn lww_winner<'a>(a: &'a Self, b: &'a Self) -> &'a Self {
        match a.timestamp.cmp(&b.timestamp) {
            Ordering::Greater => a,
            Ordering::Less => b,
            Ordering::Equal => {
                if a.actor >= b.actor {
                    a
                } else {
                    b
                }
            }
        }
    }

    fn is_concurrent(&self, other: &Self) -> bool {
        // Two writes are concurrent if they are from different actors
        // and neither has seen the other's timestamp. We approximate this
        // by checking that the actors differ and the values differ.
        self.actor != other.actor && self.value != other.value
    }

    /// Resolves a conflict by setting the chosen value and marking as Resolved.
    ///
    /// This is used when a human explicitly chooses which value should win.
    pub fn resolve(
        &self,
        chosen_value: FieldValue,
        timestamp: HlcTimestamp,
        actor: ActorId,
    ) -> Self {
        Self {
            value: chosen_value,
            timestamp,
            actor,
            conflict: ConflictState::Resolved,
            pending_writes: Vec::new(),
        }
    }
}

impl CrdtMerge for ReviewRegister {
    fn merge(&self, other: &Self) -> Self {
        // If they're the same, no conflict
        if self == other {
            return self.clone();
        }

        let winner = Self::lww_winner(self, other);
        let conflict = if self.is_concurrent(other) {
            // Collect all unique contending values
            let mut contending = vec![self.value.clone(), other.value.clone()];

            // Add any existing contending values from both sides
            if let ConflictState::NeedsReview {
                contending_values: ref existing,
            } = self.conflict
            {
                contending.extend(existing.iter().cloned());
            }
            if let ConflictState::NeedsReview {
                contending_values: ref existing,
            } = other.conflict
            {
                contending.extend(existing.iter().cloned());
            }

            // Deduplicate
            contending.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
            contending.dedup();

            ConflictState::NeedsReview {
                contending_values: contending,
            }
        } else {
            // Merge existing conflict states
            match (&self.conflict, &other.conflict) {
                (ConflictState::NeedsReview { .. }, _) => self.conflict.clone(),
                (_, ConflictState::NeedsReview { .. }) => other.conflict.clone(),
                _ => ConflictState::Resolved,
            }
        };

        Self {
            value: winner.value.clone(),
            timestamp: winner.timestamp,
            actor: winner.actor.clone(),
            conflict,
            pending_writes: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Clock;
    use crate::identity::ActorClass;

    fn make_actor(id: &str) -> ActorId {
        ActorId::new(id, ActorClass::Human)
    }

    #[test]
    fn review_register_no_conflict_same_actor() {
        let clock = Clock::new();
        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();
        let actor = make_actor("alice");

        let r1 = ReviewRegister::new(FieldValue::Int(10), ts1, actor.clone());
        let r2 = ReviewRegister::new(FieldValue::Int(20), ts2, actor);

        let merged = r1.merge(&r2);
        assert_eq!(merged.value, FieldValue::Int(20));
        assert_eq!(merged.conflict, ConflictState::Resolved);
    }

    #[test]
    fn review_register_flags_concurrent_writes() {
        let clock = Clock::new();
        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();
        let alice = make_actor("alice");
        let bob = make_actor("bob");

        let r1 = ReviewRegister::new(FieldValue::Int(10), ts1, alice);
        let r2 = ReviewRegister::new(FieldValue::Int(20), ts2, bob);

        let merged = r1.merge(&r2);
        // LWW picks the later timestamp
        assert_eq!(merged.value, FieldValue::Int(20));
        // But flags for review
        match &merged.conflict {
            ConflictState::NeedsReview { contending_values } => {
                assert!(contending_values.contains(&FieldValue::Int(10)));
                assert!(contending_values.contains(&FieldValue::Int(20)));
            }
            _ => panic!("expected NeedsReview"),
        }
    }

    #[test]
    fn review_register_no_conflict_same_value_different_actors() {
        let clock = Clock::new();
        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();
        let alice = make_actor("alice");
        let bob = make_actor("bob");

        let r1 = ReviewRegister::new(FieldValue::Int(42), ts1, alice);
        let r2 = ReviewRegister::new(FieldValue::Int(42), ts2, bob);

        let merged = r1.merge(&r2);
        assert_eq!(merged.value, FieldValue::Int(42));
        // Same value means no real conflict
        assert_eq!(merged.conflict, ConflictState::Resolved);
    }

    #[test]
    fn review_register_merge_is_commutative() {
        let clock = Clock::new();
        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();
        let alice = make_actor("alice");
        let bob = make_actor("bob");

        let r1 = ReviewRegister::new(FieldValue::Int(10), ts1, alice);
        let r2 = ReviewRegister::new(FieldValue::Int(20), ts2, bob);

        let ab = r1.merge(&r2);
        let ba = r2.merge(&r1);
        assert_eq!(ab.value, ba.value);
        assert_eq!(ab.timestamp, ba.timestamp);
    }

    #[test]
    fn review_register_merge_is_idempotent() {
        let clock = Clock::new();
        let ts = clock.new_timestamp();
        let actor = make_actor("alice");
        let r = ReviewRegister::new(FieldValue::Int(42), ts, actor);

        assert_eq!(r.merge(&r), r);
    }
}
