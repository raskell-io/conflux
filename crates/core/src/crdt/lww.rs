//! Last-Writer-Wins Register.
//!
//! The value with the highest HLC timestamp wins. On timestamp tie,
//! the lexicographically greater ActorId wins for determinism.

use crate::clock::HlcTimestamp;
use crate::crdt::CrdtMerge;
use crate::identity::ActorId;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// A last-writer-wins register parameterized over value type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LwwRegister<T> {
    pub value: T,
    pub timestamp: HlcTimestamp,
    pub actor: ActorId,
}

impl<T> LwwRegister<T> {
    pub fn new(value: T, timestamp: HlcTimestamp, actor: ActorId) -> Self {
        Self {
            value,
            timestamp,
            actor,
        }
    }
}

impl<T: Clone + PartialEq + Eq> CrdtMerge for LwwRegister<T> {
    fn merge(&self, other: &Self) -> Self {
        match self.timestamp.cmp(&other.timestamp) {
            Ordering::Greater => self.clone(),
            Ordering::Less => other.clone(),
            Ordering::Equal => {
                // Tiebreak by actor ID for determinism
                if self.actor >= other.actor {
                    self.clone()
                } else {
                    other.clone()
                }
            }
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
    fn lww_prefers_later_hlc_timestamp() {
        let clock = Clock::new();
        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();
        let actor = make_actor("alice");

        let r1 = LwwRegister::new(10, ts1, actor.clone());
        let r2 = LwwRegister::new(20, ts2, actor);

        let merged = r1.merge(&r2);
        assert_eq!(merged.value, 20);
    }

    #[test]
    fn lww_tiebreaks_by_actor_id() {
        let clock = Clock::new();
        let ts = clock.new_timestamp();
        let alice = make_actor("alice");
        let bob = make_actor("bob");

        let r1 = LwwRegister::new(10, ts, alice);
        let r2 = LwwRegister::new(20, ts, bob.clone());

        let merged = r1.merge(&r2);
        // "bob" > "alice" lexicographically
        assert_eq!(merged.value, 20);
        assert_eq!(merged.actor, bob);
    }

    #[test]
    fn lww_merge_is_commutative() {
        let clock = Clock::new();
        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();
        let alice = make_actor("alice");
        let bob = make_actor("bob");

        let r1 = LwwRegister::new(10, ts1, alice);
        let r2 = LwwRegister::new(20, ts2, bob);

        assert_eq!(r1.merge(&r2), r2.merge(&r1));
    }

    #[test]
    fn lww_merge_is_idempotent() {
        let clock = Clock::new();
        let ts = clock.new_timestamp();
        let actor = make_actor("alice");
        let r = LwwRegister::new(42, ts, actor);

        assert_eq!(r.merge(&r), r);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::clock::Clock;
    use crate::identity::ActorClass;
    use proptest::prelude::*;

    fn arb_lww_register() -> impl Strategy<Value = LwwRegister<i64>> {
        (any::<i64>(), "[a-z]{1,5}").prop_map(|(value, actor_id)| {
            let clock = Clock::new();
            let ts = clock.new_timestamp();
            let actor = ActorId::new(actor_id, ActorClass::Human);
            LwwRegister::new(value, ts, actor)
        })
    }

    proptest! {
        #[test]
        fn lww_merge_is_commutative(a in arb_lww_register(), b in arb_lww_register()) {
            prop_assert_eq!(a.merge(&b), b.merge(&a));
        }

        #[test]
        fn lww_merge_is_idempotent(a in arb_lww_register()) {
            prop_assert_eq!(a.merge(&a), a.clone());
        }

        #[test]
        fn lww_merge_is_associative(
            a in arb_lww_register(),
            b in arb_lww_register(),
            c in arb_lww_register()
        ) {
            let ab_c = a.merge(&b).merge(&c);
            let a_bc = a.merge(&b.merge(&c));
            prop_assert_eq!(ab_c, a_bc);
        }
    }
}
