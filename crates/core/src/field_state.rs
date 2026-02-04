//! FieldState — enum wrapping all CRDT types for a single field.

use crate::clock::HlcTimestamp;
use crate::crdt::{
    CrdtMerge, GrowOnlySet, LwwRegister, MaxRegister, MinRegister, ObservedRemoveSet,
    ReviewRegister,
};
use crate::field::{ConflictState, FieldValue, MergeStrategy};
use crate::identity::ActorId;
use serde::{Deserialize, Serialize};

/// The internal CRDT state for a single field.
///
/// Each variant wraps the corresponding CRDT primitive, dispatching merge
/// by the field's declared merge strategy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldState {
    Lww(LwwRegister<FieldValue>),
    Max(MaxRegister),
    Min(MinRegister),
    GrowSet(GrowOnlySet<String>),
    OrSet(ObservedRemoveSet<String>),
    Review(ReviewRegister),
}

impl FieldState {
    /// Creates a new `FieldState` from a value, timestamp, actor, and merge strategy.
    pub fn new(
        value: FieldValue,
        timestamp: HlcTimestamp,
        actor: ActorId,
        strategy: MergeStrategy,
    ) -> Self {
        match strategy {
            MergeStrategy::Lww => FieldState::Lww(LwwRegister::new(value, timestamp, actor)),
            MergeStrategy::Max => FieldState::Max(MaxRegister::new(value, timestamp, actor)),
            MergeStrategy::Min => FieldState::Min(MinRegister::new(value, timestamp, actor)),
            MergeStrategy::GrowSet => {
                let mut set = GrowOnlySet::new();
                if let FieldValue::List(items) = &value {
                    for item in items {
                        if let FieldValue::String(s) = item {
                            set.insert(s.clone());
                        }
                    }
                } else if let FieldValue::String(s) = &value {
                    set.insert(s.clone());
                }
                FieldState::GrowSet(set)
            }
            MergeStrategy::OrSet => {
                let mut set = ObservedRemoveSet::new();
                if let FieldValue::List(items) = &value {
                    for item in items {
                        if let FieldValue::String(s) = item {
                            set.add(s.clone());
                        }
                    }
                } else if let FieldValue::String(s) = &value {
                    set.add(s.clone());
                }
                FieldState::OrSet(set)
            }
            MergeStrategy::Review => {
                FieldState::Review(ReviewRegister::new(value, timestamp, actor))
            }
        }
    }

    /// Merges this field state with another.
    ///
    /// Both states must be the same variant (same merge strategy).
    /// Panics if variants differ — this should be prevented by schema validation.
    pub fn merge(&self, other: &Self) -> Self {
        match (self, other) {
            (FieldState::Lww(a), FieldState::Lww(b)) => FieldState::Lww(a.merge(b)),
            (FieldState::Max(a), FieldState::Max(b)) => FieldState::Max(a.merge(b)),
            (FieldState::Min(a), FieldState::Min(b)) => FieldState::Min(a.merge(b)),
            (FieldState::GrowSet(a), FieldState::GrowSet(b)) => FieldState::GrowSet(a.merge(b)),
            (FieldState::OrSet(a), FieldState::OrSet(b)) => FieldState::OrSet(a.merge(b)),
            (FieldState::Review(a), FieldState::Review(b)) => FieldState::Review(a.merge(b)),
            _ => panic!(
                "cannot merge different FieldState variants: {:?} vs {:?}",
                std::mem::discriminant(self),
                std::mem::discriminant(other)
            ),
        }
    }

    /// Extracts the current resolved value from this field state.
    pub fn value(&self) -> FieldValue {
        match self {
            FieldState::Lww(reg) => reg.value.clone(),
            FieldState::Max(reg) => reg.value.clone(),
            FieldState::Min(reg) => reg.value.clone(),
            FieldState::GrowSet(set) => FieldValue::List(
                set.elements
                    .iter()
                    .map(|s| FieldValue::String(s.clone()))
                    .collect(),
            ),
            FieldState::OrSet(set) => FieldValue::List(
                set.to_vec()
                    .into_iter()
                    .map(|s| FieldValue::String(s.clone()))
                    .collect(),
            ),
            FieldState::Review(reg) => reg.value.clone(),
        }
    }

    /// Returns the conflict state for this field, if applicable.
    pub fn conflict_state(&self) -> ConflictState {
        match self {
            FieldState::Review(reg) => reg.conflict.clone(),
            _ => ConflictState::Resolved,
        }
    }

    /// Returns the merge strategy this field state represents.
    pub fn strategy(&self) -> MergeStrategy {
        match self {
            FieldState::Lww(_) => MergeStrategy::Lww,
            FieldState::Max(_) => MergeStrategy::Max,
            FieldState::Min(_) => MergeStrategy::Min,
            FieldState::GrowSet(_) => MergeStrategy::GrowSet,
            FieldState::OrSet(_) => MergeStrategy::OrSet,
            FieldState::Review(_) => MergeStrategy::Review,
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
    fn field_state_lww_merge() {
        let clock = Clock::new();
        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();
        let actor = make_actor("alice");

        let s1 = FieldState::new(FieldValue::Int(10), ts1, actor.clone(), MergeStrategy::Lww);
        let s2 = FieldState::new(FieldValue::Int(20), ts2, actor, MergeStrategy::Lww);

        let merged = s1.merge(&s2);
        assert_eq!(merged.value(), FieldValue::Int(20));
    }

    #[test]
    fn field_state_max_merge() {
        let clock = Clock::new();
        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();
        let actor = make_actor("alice");

        let s1 = FieldState::new(FieldValue::Int(100), ts1, actor.clone(), MergeStrategy::Max);
        let s2 = FieldState::new(FieldValue::Int(20), ts2, actor, MergeStrategy::Max);

        let merged = s1.merge(&s2);
        assert_eq!(merged.value(), FieldValue::Int(100));
    }

    #[test]
    fn field_state_grow_set_merge() {
        let clock = Clock::new();
        let ts = clock.new_timestamp();
        let actor = make_actor("alice");

        let s1 = FieldState::new(
            FieldValue::String("a".into()),
            ts,
            actor.clone(),
            MergeStrategy::GrowSet,
        );
        let s2 = FieldState::new(
            FieldValue::String("b".into()),
            ts,
            actor,
            MergeStrategy::GrowSet,
        );

        let merged = s1.merge(&s2);
        if let FieldValue::List(items) = merged.value() {
            let strings: Vec<String> = items
                .into_iter()
                .map(|v| match v {
                    FieldValue::String(s) => s,
                    _ => panic!("expected string"),
                })
                .collect();
            assert!(strings.contains(&"a".to_string()));
            assert!(strings.contains(&"b".to_string()));
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn field_state_conflict_state_resolved_for_lww() {
        let clock = Clock::new();
        let ts = clock.new_timestamp();
        let actor = make_actor("alice");
        let s = FieldState::new(FieldValue::Int(10), ts, actor, MergeStrategy::Lww);
        assert_eq!(s.conflict_state(), ConflictState::Resolved);
    }

    #[test]
    fn field_state_strategy() {
        let clock = Clock::new();
        let ts = clock.new_timestamp();
        let actor = make_actor("alice");

        let s = FieldState::new(FieldValue::Int(10), ts, actor, MergeStrategy::Max);
        assert_eq!(s.strategy(), MergeStrategy::Max);
    }
}
