//! Grow-Only Set — union-only set, no removes.

use crate::crdt::CrdtMerge;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A set that only grows via union. Elements can never be removed.
///
/// Uses `BTreeSet` for deterministic ordering across replicas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrowOnlySet<T: Ord> {
    pub elements: BTreeSet<T>,
}

impl<T: Ord> GrowOnlySet<T> {
    pub fn new() -> Self {
        Self {
            elements: BTreeSet::new(),
        }
    }

    pub fn insert(&mut self, element: T) {
        self.elements.insert(element);
    }

    pub fn contains(&self, element: &T) -> bool {
        self.elements.contains(element)
    }

    pub fn len(&self) -> usize {
        self.elements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
}

impl<T: Ord> Default for GrowOnlySet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Ord + Clone> CrdtMerge for GrowOnlySet<T> {
    fn merge(&self, other: &Self) -> Self {
        let elements: BTreeSet<T> = self.elements.union(&other.elements).cloned().collect();
        Self { elements }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grow_set_union_merges_elements() {
        let mut a = GrowOnlySet::new();
        a.insert("x".to_string());
        a.insert("y".to_string());

        let mut b = GrowOnlySet::new();
        b.insert("y".to_string());
        b.insert("z".to_string());

        let merged = a.merge(&b);
        assert_eq!(merged.len(), 3);
        assert!(merged.contains(&"x".to_string()));
        assert!(merged.contains(&"y".to_string()));
        assert!(merged.contains(&"z".to_string()));
    }

    #[test]
    fn grow_set_merge_is_commutative() {
        let mut a = GrowOnlySet::new();
        a.insert(1);
        a.insert(2);

        let mut b = GrowOnlySet::new();
        b.insert(2);
        b.insert(3);

        assert_eq!(a.merge(&b), b.merge(&a));
    }

    #[test]
    fn grow_set_merge_is_idempotent() {
        let mut a = GrowOnlySet::new();
        a.insert(1);
        a.insert(2);

        assert_eq!(a.merge(&a), a);
    }

    #[test]
    fn grow_set_merge_is_associative() {
        let mut a = GrowOnlySet::new();
        a.insert(1);
        let mut b = GrowOnlySet::new();
        b.insert(2);
        let mut c = GrowOnlySet::new();
        c.insert(3);

        let ab_c = a.merge(&b).merge(&c);
        let a_bc = a.merge(&b.merge(&c));
        assert_eq!(ab_c, a_bc);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::collection::btree_set;
    use proptest::prelude::*;

    fn arb_grow_set() -> impl Strategy<Value = GrowOnlySet<i32>> {
        btree_set(any::<i32>(), 0..10).prop_map(|elements| GrowOnlySet { elements })
    }

    proptest! {
        #[test]
        fn grow_set_merge_is_commutative(a in arb_grow_set(), b in arb_grow_set()) {
            prop_assert_eq!(a.merge(&b), b.merge(&a));
        }

        #[test]
        fn grow_set_merge_is_idempotent(a in arb_grow_set()) {
            prop_assert_eq!(a.merge(&a), a.clone());
        }

        #[test]
        fn grow_set_merge_is_associative(
            a in arb_grow_set(),
            b in arb_grow_set(),
            c in arb_grow_set()
        ) {
            let ab_c = a.merge(&b).merge(&c);
            let a_bc = a.merge(&b.merge(&c));
            prop_assert_eq!(ab_c, a_bc);
        }
    }
}
