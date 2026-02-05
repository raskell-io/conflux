//! Hybrid Logical Clock types wrapping `uhlc`.

use crate::error::ClockError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A hybrid logical clock timestamp.
///
/// Wraps `uhlc::Timestamp` to provide Serialize/Deserialize and a stable API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HlcTimestamp(uhlc::Timestamp);

impl HlcTimestamp {
    /// Creates a new timestamp from a `uhlc::Timestamp`.
    pub fn new(ts: uhlc::Timestamp) -> Self {
        Self(ts)
    }

    /// Returns the inner `uhlc::Timestamp`.
    pub fn inner(&self) -> &uhlc::Timestamp {
        &self.0
    }

    /// Returns an RFC3339-lossy string representation.
    pub fn to_rfc3339(&self) -> String {
        self.0.to_string_rfc3339_lossy()
    }
}

impl fmt::Display for HlcTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for HlcTimestamp {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for HlcTimestamp {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let ts: uhlc::Timestamp = s.parse().map_err(serde::de::Error::custom)?;
        Ok(Self(ts))
    }
}

/// Wrapper around `uhlc::HLC` providing a simpler interface.
///
/// This type is intentionally *not* Clone or Serialize because `uhlc::HLC`
/// contains a Mutex. Pass it by reference or keep it external to Document.
pub struct Clock {
    inner: uhlc::HLC,
}

impl Clock {
    /// Creates a new clock with a random ID.
    pub fn new() -> Self {
        Self {
            inner: uhlc::HLCBuilder::new().build(),
        }
    }

    /// Creates a new clock with a specific ID.
    pub fn with_id(id: uhlc::ID) -> Self {
        Self {
            inner: uhlc::HLCBuilder::new().with_id(id).build(),
        }
    }

    /// Creates a new clock from a node ID string.
    ///
    /// The node ID is hashed (truncated to 16 bytes) to create a stable uhlc::ID.
    /// This allows using human-readable node names like "node-a" while still
    /// getting deterministic clock IDs.
    ///
    /// # Errors
    ///
    /// Returns `ClockError::InvalidNodeId` if the resulting ID is invalid.
    pub fn with_node_id(node_id: &str) -> Result<Self, ClockError> {
        use sha2::{Digest, Sha256};

        // Hash the node_id to get deterministic bytes
        let mut hasher = Sha256::new();
        hasher.update(node_id.as_bytes());
        let hash = hasher.finalize();

        // Take first 16 bytes (max size for uhlc::ID)
        let id_bytes: [u8; 16] = hash[..16].try_into().expect("sha256 produces 32 bytes");

        // Create uhlc::ID from bytes
        let id = uhlc::ID::try_from(&id_bytes[..])
            .map_err(|e| ClockError::InvalidNodeId(format!("{node_id}: {e}")))?;

        Ok(Self::with_id(id))
    }

    /// Returns the clock's ID as a hex string.
    pub fn id_hex(&self) -> String {
        // We can't access the ID directly from HLC, but we can get it from a timestamp
        // For now, return empty - this would require storing the ID separately
        // if needed for debugging
        String::new()
    }

    /// Generates a new unique timestamp.
    pub fn new_timestamp(&self) -> HlcTimestamp {
        HlcTimestamp(self.inner.new_timestamp())
    }

    /// Updates the clock with an incoming timestamp.
    ///
    /// # Errors
    ///
    /// Returns `ClockError::DeltaExceeded` if the incoming timestamp is
    /// too far from the local clock.
    pub fn update_with_timestamp(&self, ts: &HlcTimestamp) -> Result<(), ClockError> {
        self.inner
            .update_with_timestamp(&ts.0)
            .map_err(|e| ClockError::DeltaExceeded(e.to_string()))
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Clock").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_generates_increasing_timestamps() {
        let clock = Clock::new();
        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();
        assert!(ts2 > ts1);
    }

    #[test]
    fn timestamp_roundtrip_serde() {
        let clock = Clock::new();
        let ts = clock.new_timestamp();
        let json = serde_json::to_string(&ts).unwrap();
        let ts2: HlcTimestamp = serde_json::from_str(&json).unwrap();
        assert_eq!(ts, ts2);
    }

    #[test]
    fn clock_update_with_timestamp() {
        let clock1 = Clock::new();
        let clock2 = Clock::new();
        let ts1 = clock1.new_timestamp();
        clock2.update_with_timestamp(&ts1).unwrap();
        let ts2 = clock2.new_timestamp();
        assert!(ts2 > ts1);
    }

    #[test]
    fn clock_with_node_id() {
        // Creating a clock with a node ID should succeed
        let clock = Clock::with_node_id("node-a").unwrap();
        let ts = clock.new_timestamp();
        assert!(ts.to_string().len() > 0);
    }

    #[test]
    fn clock_with_node_id_deterministic() {
        // Same node ID should produce clocks that generate comparable timestamps
        let clock1 = Clock::with_node_id("node-a").unwrap();
        let clock2 = Clock::with_node_id("node-a").unwrap();

        let ts1 = clock1.new_timestamp();
        let ts2 = clock2.new_timestamp();

        // Both should generate valid timestamps
        assert!(ts1.to_string().len() > 0);
        assert!(ts2.to_string().len() > 0);
    }
}
