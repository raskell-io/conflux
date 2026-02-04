//! Field value types, merge strategies, and conflict state.

use crate::identity::EntityId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// A dynamically-typed field value in the document model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FieldValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    /// Duration in milliseconds.
    Duration(i64),
    /// Reference to another entity.
    Ref(EntityId),
    List(Vec<FieldValue>),
    Map(BTreeMap<String, FieldValue>),
    Null,
}

impl Eq for FieldValue {}

impl FieldValue {
    /// Returns the type name as a string for error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            FieldValue::String(_) => "string",
            FieldValue::Int(_) => "int",
            FieldValue::Float(_) => "float",
            FieldValue::Bool(_) => "bool",
            FieldValue::Duration(_) => "duration",
            FieldValue::Ref(_) => "ref",
            FieldValue::List(_) => "list",
            FieldValue::Map(_) => "map",
            FieldValue::Null => "null",
        }
    }

    /// Compares two field values numerically.
    ///
    /// Returns `Some(Ordering)` if both values are numeric (Int or Float),
    /// `None` otherwise.
    pub fn numeric_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (FieldValue::Int(a), FieldValue::Int(b)) => Some(a.cmp(b)),
            (FieldValue::Float(a), FieldValue::Float(b)) => a.partial_cmp(b),
            (FieldValue::Int(a), FieldValue::Float(b)) => (*a as f64).partial_cmp(b),
            (FieldValue::Float(a), FieldValue::Int(b)) => a.partial_cmp(&(*b as f64)),
            _ => None,
        }
    }
}

impl fmt::Display for FieldValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldValue::String(s) => write!(f, "\"{s}\""),
            FieldValue::Int(n) => write!(f, "{n}"),
            FieldValue::Float(n) => write!(f, "{n}"),
            FieldValue::Bool(b) => write!(f, "{b}"),
            FieldValue::Duration(ms) => write!(f, "{ms}ms"),
            FieldValue::Ref(id) => write!(f, "ref({id})"),
            FieldValue::List(items) => write!(f, "[{} items]", items.len()),
            FieldValue::Map(map) => write!(f, "{{{} entries}}", map.len()),
            FieldValue::Null => write!(f, "null"),
        }
    }
}

/// Merge strategy for a field, declared in the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MergeStrategy {
    /// Last-writer-wins by HLC timestamp, actor tiebreak.
    Lww,
    /// Highest numeric value wins.
    Max,
    /// Lowest numeric value wins.
    Min,
    /// Grow-only set (union, no removes).
    GrowSet,
    /// Observed-remove set (add-wins on concurrent add/remove).
    OrSet,
    /// LWW with concurrent write detection; flags for human review.
    Review,
}

impl fmt::Display for MergeStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MergeStrategy::Lww => write!(f, "lww"),
            MergeStrategy::Max => write!(f, "max"),
            MergeStrategy::Min => write!(f, "min"),
            MergeStrategy::GrowSet => write!(f, "grow_set"),
            MergeStrategy::OrSet => write!(f, "or_set"),
            MergeStrategy::Review => write!(f, "review"),
        }
    }
}

/// Conflict state for a field after merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictState {
    /// The merge resolved cleanly.
    Resolved,
    /// Concurrent writes were detected and need human review.
    NeedsReview {
        /// The contending values from concurrent writers.
        contending_values: Vec<FieldValue>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_value_type_name() {
        assert_eq!(FieldValue::Int(42).type_name(), "int");
        assert_eq!(FieldValue::String("hi".into()).type_name(), "string");
        assert_eq!(FieldValue::Null.type_name(), "null");
    }

    #[test]
    fn numeric_cmp_int_int() {
        let a = FieldValue::Int(10);
        let b = FieldValue::Int(20);
        assert_eq!(a.numeric_cmp(&b), Some(std::cmp::Ordering::Less));
    }

    #[test]
    fn numeric_cmp_float_float() {
        let a = FieldValue::Float(1.5);
        let b = FieldValue::Float(1.5);
        assert_eq!(a.numeric_cmp(&b), Some(std::cmp::Ordering::Equal));
    }

    #[test]
    fn numeric_cmp_mixed() {
        let a = FieldValue::Int(10);
        let b = FieldValue::Float(9.5);
        assert_eq!(a.numeric_cmp(&b), Some(std::cmp::Ordering::Greater));
    }

    #[test]
    fn numeric_cmp_non_numeric_returns_none() {
        let a = FieldValue::String("hi".into());
        let b = FieldValue::Int(1);
        assert_eq!(a.numeric_cmp(&b), None);
    }

    #[test]
    fn field_value_display() {
        assert_eq!(FieldValue::Int(42).to_string(), "42");
        assert_eq!(FieldValue::Null.to_string(), "null");
    }

    #[test]
    fn merge_strategy_display() {
        assert_eq!(MergeStrategy::Lww.to_string(), "lww");
        assert_eq!(MergeStrategy::OrSet.to_string(), "or_set");
    }
}
