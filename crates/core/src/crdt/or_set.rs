//! Observed-Remove Set — add/remove with add-wins semantics.
//!
//! Each element addition is tagged with a unique identifier. Removes
//! tombstone specific tags. Concurrent add and remove of the same element
//! resolves to the element being present (add-wins).

use crate::crdt::CrdtMerge;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// A unique tag identifying a specific add operation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UniqueTag(Uuid);

impl UniqueTag {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for UniqueTag {
    fn default() -> Self {
        Self::new()
    }
}

/// An observed-remove set with add-wins semantics.
///
/// Elements are tracked by unique tags. Removing an element tombstones
/// its current tags. A concurrent add (with a new tag) survives the remove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedRemoveSet<T: Ord> {
    /// Map from element to the set of active tags for that element.
    pub elements: BTreeMap<T, BTreeSet<UniqueTag>>,
    /// Set of tombstoned tags.
    pub tombstones: BTreeSet<UniqueTag>,
}

impl<T: Ord> ObservedRemoveSet<T> {
    pub fn new() -> Self {
        Self {
            elements: BTreeMap::new(),
            tombstones: BTreeSet::new(),
        }
    }

    /// Adds an element, returning the unique tag for this add operation.
    pub fn add(&mut self, element: T) -> UniqueTag {
        let tag = UniqueTag::new();
        self.elements
            .entry(element)
            .or_default()
            .insert(tag.clone());
        tag
    }

    /// Removes an element by tombstoning all its current tags.
    pub fn remove(&mut self, element: &T) {
        if let Some(tags) = self.elements.get(element) {
            self.tombstones.extend(tags.iter().cloned());
        }
    }

    /// Returns whether the element is currently present (has non-tombstoned tags).
    pub fn contains(&self, element: &T) -> bool {
        if let Some(tags) = self.elements.get(element) {
            tags.iter().any(|tag| !self.tombstones.contains(tag))
        } else {
            false
        }
    }

    /// Returns all currently-present elements.
    pub fn to_vec(&self) -> Vec<&T> {
        self.elements
            .iter()
            .filter(|(_, tags)| tags.iter().any(|tag| !self.tombstones.contains(tag)))
            .map(|(element, _)| element)
            .collect()
    }

    /// Returns the number of currently-present elements.
    pub fn len(&self) -> usize {
        self.to_vec().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T: Ord> Default for ObservedRemoveSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Ord + Clone> CrdtMerge for ObservedRemoveSet<T> {
    fn merge(&self, other: &Self) -> Self {
        // Union all elements and their tags
        let mut elements: BTreeMap<T, BTreeSet<UniqueTag>> = self.elements.clone();
        for (element, tags) in &other.elements {
            elements
                .entry(element.clone())
                .or_default()
                .extend(tags.iter().cloned());
        }

        // Union all tombstones
        let tombstones: BTreeSet<UniqueTag> =
            self.tombstones.union(&other.tombstones).cloned().collect();

        // Clean up: remove elements whose tags are all tombstoned
        elements.retain(|_, tags| {
            tags.retain(|tag| !tombstones.contains(tag));
            !tags.is_empty()
        });

        Self {
            elements,
            tombstones,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn or_set_add_and_contains() {
        let mut set = ObservedRemoveSet::new();
        set.add("x".to_string());
        assert!(set.contains(&"x".to_string()));
        assert!(!set.contains(&"y".to_string()));
    }

    #[test]
    fn or_set_remove_tombstones_element() {
        let mut set = ObservedRemoveSet::new();
        set.add("x".to_string());
        set.remove(&"x".to_string());
        assert!(!set.contains(&"x".to_string()));
    }

    #[test]
    fn or_set_concurrent_add_wins_over_remove() {
        let mut set_a = ObservedRemoveSet::new();
        let mut set_b = ObservedRemoveSet::new();

        // Both start with "x"
        let tag = UniqueTag::new();
        set_a
            .elements
            .entry("x".to_string())
            .or_default()
            .insert(tag.clone());
        set_b
            .elements
            .entry("x".to_string())
            .or_default()
            .insert(tag.clone());

        // A removes "x"
        set_a.remove(&"x".to_string());

        // B concurrently adds "x" again (new tag)
        set_b.add("x".to_string());

        // Merge: add-wins, "x" should be present
        let merged = set_a.merge(&set_b);
        assert!(merged.contains(&"x".to_string()));
    }

    #[test]
    fn or_set_merge_is_commutative() {
        let mut a = ObservedRemoveSet::new();
        a.add("x".to_string());

        let mut b = ObservedRemoveSet::new();
        b.add("y".to_string());

        let ab = a.merge(&b);
        let ba = b.merge(&a);
        assert_eq!(ab.to_vec(), ba.to_vec());
    }

    #[test]
    fn or_set_merge_is_idempotent() {
        let mut a = ObservedRemoveSet::new();
        a.add("x".to_string());
        a.add("y".to_string());

        let merged = a.merge(&a);
        assert_eq!(merged.to_vec(), a.to_vec());
    }
}
