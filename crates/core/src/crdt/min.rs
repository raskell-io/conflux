//! Min Register — lowest numeric value wins.
//!
//! Falls back to LWW semantics on numeric tie or non-numeric type mismatch.

use crate::clock::HlcTimestamp;
use crate::crdt::CrdtMerge;
use crate::field::FieldValue;
use crate::identity::ActorId;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// A register where the lowest numeric value wins on merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinRegister {
    pub value: FieldValue,
    pub timestamp: HlcTimestamp,
    pub actor: ActorId,
}

impl MinRegister {
    pub fn new(value: FieldValue, timestamp: HlcTimestamp, actor: ActorId) -> Self {
        Self {
            value,
            timestamp,
            actor,
        }
    }

    fn lww_tiebreak(&self, other: &Self) -> Self {
        match self.timestamp.cmp(&other.timestamp) {
            Ordering::Greater => self.clone(),
            Ordering::Less => other.clone(),
            Ordering::Equal => {
                if self.actor >= other.actor {
                    self.clone()
                } else {
                    other.clone()
                }
            }
        }
    }
}

impl CrdtMerge for MinRegister {
    fn merge(&self, other: &Self) -> Self {
        match self.value.numeric_cmp(&other.value) {
            Some(Ordering::Less) => self.clone(),
            Some(Ordering::Greater) => other.clone(),
            Some(Ordering::Equal) => self.lww_tiebreak(other),
            None => self.lww_tiebreak(other),
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
    fn min_selects_lowest_value() {
        let clock = Clock::new();
        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();
        let actor = make_actor("alice");

        let r1 = MinRegister::new(FieldValue::Int(10), ts1, actor.clone());
        let r2 = MinRegister::new(FieldValue::Int(20), ts2, actor);

        let merged = r1.merge(&r2);
        assert_eq!(merged.value, FieldValue::Int(10));
    }

    #[test]
    fn min_selects_lowest_even_with_older_timestamp() {
        let clock = Clock::new();
        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();
        let actor = make_actor("alice");

        let r1 = MinRegister::new(FieldValue::Int(5), ts1, actor.clone());
        let r2 = MinRegister::new(FieldValue::Int(20), ts2, actor);

        let merged = r1.merge(&r2);
        assert_eq!(merged.value, FieldValue::Int(5));
    }

    #[test]
    fn min_merge_is_commutative() {
        let clock = Clock::new();
        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();
        let alice = make_actor("alice");
        let bob = make_actor("bob");

        let r1 = MinRegister::new(FieldValue::Int(10), ts1, alice);
        let r2 = MinRegister::new(FieldValue::Int(20), ts2, bob);

        assert_eq!(r1.merge(&r2), r2.merge(&r1));
    }

    #[test]
    fn min_merge_is_idempotent() {
        let clock = Clock::new();
        let ts = clock.new_timestamp();
        let actor = make_actor("alice");
        let r = MinRegister::new(FieldValue::Int(42), ts, actor);

        assert_eq!(r.merge(&r), r);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::clock::Clock;
    use crate::identity::ActorClass;
    use proptest::prelude::*;

    fn arb_min_register() -> impl Strategy<Value = MinRegister> {
        (any::<i64>(), "[a-z]{1,5}").prop_map(|(value, actor_id)| {
            let clock = Clock::new();
            let ts = clock.new_timestamp();
            let actor = ActorId::new(actor_id, ActorClass::Human);
            MinRegister::new(FieldValue::Int(value), ts, actor)
        })
    }

    proptest! {
        #[test]
        fn min_merge_is_commutative(a in arb_min_register(), b in arb_min_register()) {
            prop_assert_eq!(a.merge(&b), b.merge(&a));
        }

        #[test]
        fn min_merge_is_idempotent(a in arb_min_register()) {
            prop_assert_eq!(a.merge(&a), a.clone());
        }

        #[test]
        fn min_merge_is_associative(
            a in arb_min_register(),
            b in arb_min_register(),
            c in arb_min_register()
        ) {
            let ab_c = a.merge(&b).merge(&c);
            let a_bc = a.merge(&b.merge(&c));
            prop_assert_eq!(ab_c, a_bc);
        }
    }
}
