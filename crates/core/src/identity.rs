//! Identity types: EntityId, ActorId, ActorClass.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for an entity in the document tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityId(String);

impl EntityId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for EntityId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for EntityId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Classification of an actor performing operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ActorClass {
    Human,
    Pipeline,
    Operator,
    System,
}

impl fmt::Display for ActorClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActorClass::Human => write!(f, "human"),
            ActorClass::Pipeline => write!(f, "pipeline"),
            ActorClass::Operator => write!(f, "operator"),
            ActorClass::System => write!(f, "system"),
        }
    }
}

/// Identity of an actor that performs operations.
///
/// Ordering is lexicographic on `id` for deterministic tiebreaking in CRDTs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActorId {
    pub id: String,
    pub class: ActorClass,
    pub display_name: String,
}

impl ActorId {
    pub fn new(id: impl Into<String>, class: ActorClass) -> Self {
        let id = id.into();
        let display_name = id.clone();
        Self {
            id,
            class,
            display_name,
        }
    }

    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = name.into();
        self
    }
}

impl fmt::Display for ActorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.id, self.class)
    }
}

impl PartialOrd for ActorId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ActorId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_id_display() {
        let id = EntityId::new("route.api");
        assert_eq!(id.to_string(), "route.api");
    }

    #[test]
    fn actor_id_ordering_is_by_id_string() {
        let a = ActorId::new("alice", ActorClass::Human);
        let b = ActorId::new("bob", ActorClass::Pipeline);
        assert!(a < b);
    }

    #[test]
    fn actor_id_display() {
        let a = ActorId::new("autoscaler", ActorClass::Operator);
        assert_eq!(a.to_string(), "autoscaler(operator)");
    }
}
