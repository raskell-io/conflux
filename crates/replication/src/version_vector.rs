//! Version vector for causal anti-entropy.

use conflux_core::HlcTimestamp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::node::NodeId;

/// A version vector tracking the highest timestamp seen from each node.
///
/// Used for anti-entropy sync to identify which operations a peer is missing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionVector {
    /// Maps node_id to highest HLC timestamp seen from that node.
    entries: BTreeMap<NodeId, HlcTimestamp>,
}

impl VersionVector {
    /// Creates a new empty version vector.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Updates the timestamp for a node, keeping the maximum.
    pub fn update(&mut self, node_id: &NodeId, timestamp: HlcTimestamp) {
        self.entries
            .entry(node_id.clone())
            .and_modify(|existing| {
                if timestamp > *existing {
                    *existing = timestamp;
                }
            })
            .or_insert(timestamp);
    }

    /// Returns the highest timestamp seen from a node.
    pub fn get(&self, node_id: &NodeId) -> Option<&HlcTimestamp> {
        self.entries.get(node_id)
    }

    /// Returns true if this vector dominates or equals another.
    ///
    /// A dominates B if for every node in B, A has a >= timestamp.
    pub fn dominates(&self, other: &VersionVector) -> bool {
        for (node_id, other_ts) in &other.entries {
            match self.entries.get(node_id) {
                Some(self_ts) if self_ts >= other_ts => continue,
                _ => return false,
            }
        }
        true
    }

    /// Merges another version vector into this one, taking the maximum timestamp
    /// for each node.
    pub fn merge(&mut self, other: &VersionVector) {
        for (node_id, ts) in &other.entries {
            self.update(node_id, *ts);
        }
    }

    /// Returns the entries as a slice for iteration.
    pub fn entries(&self) -> &BTreeMap<NodeId, HlcTimestamp> {
        &self.entries
    }

    /// Returns true if the vector is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the number of nodes tracked.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Computes the difference: nodes/timestamps in self that are newer than in other.
    ///
    /// Returns a list of (node_id, other's timestamp or None) for nodes where
    /// self has a newer timestamp. This tells us what operations the other node
    /// might be missing.
    pub fn diff(&self, other: &VersionVector) -> Vec<(NodeId, Option<HlcTimestamp>)> {
        let mut diff = Vec::new();

        for (node_id, self_ts) in &self.entries {
            match other.entries.get(node_id) {
                Some(other_ts) if self_ts > other_ts => {
                    diff.push((node_id.clone(), Some(*other_ts)));
                }
                None => {
                    diff.push((node_id.clone(), None));
                }
                _ => {}
            }
        }

        diff
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conflux_core::Clock;

    fn make_timestamp(clock: &Clock) -> HlcTimestamp {
        clock.new_timestamp()
    }

    #[test]
    fn version_vector_update() {
        let clock = Clock::new();
        let mut vv = VersionVector::new();

        let node_a = NodeId::new("node-a");
        let ts1 = make_timestamp(&clock);
        let ts2 = make_timestamp(&clock);

        vv.update(&node_a, ts1);
        assert_eq!(vv.get(&node_a), Some(&ts1));

        // Update with higher timestamp
        vv.update(&node_a, ts2);
        assert_eq!(vv.get(&node_a), Some(&ts2));

        // Update with lower timestamp should not change
        vv.update(&node_a, ts1);
        assert_eq!(vv.get(&node_a), Some(&ts2));
    }

    #[test]
    fn version_vector_dominates() {
        let clock = Clock::new();
        let node_a = NodeId::new("node-a");
        let node_b = NodeId::new("node-b");

        let ts1 = make_timestamp(&clock);
        let ts2 = make_timestamp(&clock);
        let ts3 = make_timestamp(&clock);

        // vv1: a=ts2, b=ts1
        let mut vv1 = VersionVector::new();
        vv1.update(&node_a, ts2);
        vv1.update(&node_b, ts1);

        // vv2: a=ts1
        let mut vv2 = VersionVector::new();
        vv2.update(&node_a, ts1);

        // vv1 dominates vv2 (has higher ts for a, and b is extra)
        assert!(vv1.dominates(&vv2));

        // vv2 does not dominate vv1 (missing b, and a is lower)
        assert!(!vv2.dominates(&vv1));

        // vv3: a=ts3 (higher than vv1)
        let mut vv3 = VersionVector::new();
        vv3.update(&node_a, ts3);

        // vv1 does not dominate vv3 (ts2 < ts3)
        assert!(!vv1.dominates(&vv3));
    }

    #[test]
    fn version_vector_merge() {
        let clock = Clock::new();
        let node_a = NodeId::new("node-a");
        let node_b = NodeId::new("node-b");
        let node_c = NodeId::new("node-c");

        let ts1 = make_timestamp(&clock);
        let ts2 = make_timestamp(&clock);
        let ts3 = make_timestamp(&clock);

        let mut vv1 = VersionVector::new();
        vv1.update(&node_a, ts1);
        vv1.update(&node_b, ts3);

        let mut vv2 = VersionVector::new();
        vv2.update(&node_a, ts2); // Higher than vv1
        vv2.update(&node_c, ts1); // New node

        vv1.merge(&vv2);

        // a should be ts2 (higher)
        assert_eq!(vv1.get(&node_a), Some(&ts2));
        // b should be ts3 (unchanged)
        assert_eq!(vv1.get(&node_b), Some(&ts3));
        // c should be ts1 (new)
        assert_eq!(vv1.get(&node_c), Some(&ts1));
    }

    #[test]
    fn version_vector_diff() {
        let clock = Clock::new();
        let node_a = NodeId::new("node-a");
        let node_b = NodeId::new("node-b");
        let node_c = NodeId::new("node-c");

        let ts1 = make_timestamp(&clock);
        let ts2 = make_timestamp(&clock);
        let ts3 = make_timestamp(&clock);

        // self: a=ts3, b=ts2, c=ts1
        let mut self_vv = VersionVector::new();
        self_vv.update(&node_a, ts3);
        self_vv.update(&node_b, ts2);
        self_vv.update(&node_c, ts1);

        // other: a=ts1, b=ts2 (missing c)
        let mut other_vv = VersionVector::new();
        other_vv.update(&node_a, ts1);
        other_vv.update(&node_b, ts2);

        let diff = self_vv.diff(&other_vv);

        // Should have diffs for a (ts3 > ts1) and c (missing)
        assert_eq!(diff.len(), 2);

        let diff_map: std::collections::HashMap<_, _> = diff.into_iter().collect();
        assert_eq!(diff_map.get(&node_a), Some(&Some(ts1)));
        assert_eq!(diff_map.get(&node_c), Some(&None));
        assert!(!diff_map.contains_key(&node_b)); // b is same
    }

    #[test]
    fn version_vector_serde_roundtrip() {
        let clock = Clock::new();
        let mut vv = VersionVector::new();
        vv.update(&NodeId::new("node-a"), clock.new_timestamp());
        vv.update(&NodeId::new("node-b"), clock.new_timestamp());

        let json = serde_json::to_string(&vv).unwrap();
        let vv2: VersionVector = serde_json::from_str(&json).unwrap();

        assert_eq!(vv, vv2);
    }
}
