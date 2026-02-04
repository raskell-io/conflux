//! Schema type definitions.

use conflux_core::MergeStrategy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A parsed Conflux schema defining entity types, fields, and environments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    /// Schema metadata (name, version).
    pub name: String,
    pub version: String,
    /// Entity type definitions, keyed by entity type name.
    pub entities: HashMap<String, EntityDef>,
    /// Environment configuration.
    pub environments: Option<EnvironmentsDef>,
}

/// Definition of an entity type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityDef {
    /// The entity type name.
    pub name: String,
    /// Field definitions, keyed by field name.
    pub fields: HashMap<String, FieldDef>,
    /// Child entity definitions.
    pub children: HashMap<String, ChildDef>,
}

/// Definition of a field on an entity type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    /// The field name.
    pub name: String,
    /// The field's data type.
    pub field_type: FieldType,
    /// The merge strategy.
    pub merge_strategy: MergeStrategy,
    /// Conflict handling mode.
    pub conflict: ConflictMode,
    /// For `ref` fields, the target entity type.
    pub target: Option<String>,
    /// Whether this field is required.
    pub required: bool,
    /// Numeric range constraint: (min, max).
    pub range: Option<(f64, f64)>,
    /// Enum constraint: allowed values.
    pub allowed_values: Option<Vec<String>>,
    /// Regex pattern constraint.
    pub pattern: Option<String>,
}

/// Supported field data types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldType {
    String,
    Int,
    Float,
    Bool,
    Duration,
    Ref,
    List(Option<Box<FieldType>>),
    Map,
}

impl FieldType {
    /// Parses a field type string (e.g., "string", "int", "list<address>").
    pub fn parse(s: &str) -> Option<Self> {
        if let Some(inner) = s.strip_prefix("list<").and_then(|s| s.strip_suffix('>')) {
            let inner_type = Self::parse(inner).map(Box::new);
            return Some(FieldType::List(inner_type));
        }
        match s {
            "string" => Some(FieldType::String),
            "int" => Some(FieldType::Int),
            "float" => Some(FieldType::Float),
            "bool" => Some(FieldType::Bool),
            "duration" => Some(FieldType::Duration),
            "ref" => Some(FieldType::Ref),
            "list" => Some(FieldType::List(None)),
            "map" => Some(FieldType::Map),
            _ => None,
        }
    }

    /// Returns the type name string used in core's FieldSchema.
    pub fn as_core_type_str(&self) -> &str {
        match self {
            FieldType::String => "string",
            FieldType::Int => "int",
            FieldType::Float => "float",
            FieldType::Bool => "bool",
            FieldType::Duration => "duration",
            FieldType::Ref => "ref",
            FieldType::List(_) => "list",
            FieldType::Map => "map",
        }
    }
}

impl std::fmt::Display for FieldType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FieldType::List(Some(inner)) => write!(f, "list<{inner}>"),
            FieldType::List(None) => write!(f, "list"),
            other => write!(f, "{}", other.as_core_type_str()),
        }
    }
}

/// Conflict handling mode for a field.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictMode {
    /// Merge strategy decides; no additional flagging.
    #[default]
    Auto,
    /// Flag for human review even if merge strategy could decide.
    Review,
}

impl ConflictMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(ConflictMode::Auto),
            "review" => Some(ConflictMode::Review),
            _ => None,
        }
    }
}

/// Definition of a child relationship on an entity type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildDef {
    /// The child entity type.
    pub entity_type: String,
    /// Merge strategy for the children collection.
    pub merge: String,
}

/// Environment configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentsDef {
    /// The base environment name.
    pub base: String,
    /// Overlay environments with their inheritance.
    pub overlays: Vec<OverlayDef>,
}

/// An environment overlay definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayDef {
    /// The overlay environment name.
    pub name: String,
    /// The environment this overlay inherits from.
    pub inherits: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_type_parse_simple() {
        assert_eq!(FieldType::parse("string"), Some(FieldType::String));
        assert_eq!(FieldType::parse("int"), Some(FieldType::Int));
        assert_eq!(FieldType::parse("bool"), Some(FieldType::Bool));
        assert_eq!(FieldType::parse("ref"), Some(FieldType::Ref));
    }

    #[test]
    fn field_type_parse_list() {
        assert_eq!(FieldType::parse("list"), Some(FieldType::List(None)));
        assert_eq!(
            FieldType::parse("list<string>"),
            Some(FieldType::List(Some(Box::new(FieldType::String))))
        );
    }

    #[test]
    fn field_type_parse_unknown() {
        assert_eq!(FieldType::parse("foobar"), None);
    }

    #[test]
    fn field_type_display() {
        assert_eq!(FieldType::String.to_string(), "string");
        assert_eq!(
            FieldType::List(Some(Box::new(FieldType::String))).to_string(),
            "list<string>"
        );
    }

    #[test]
    fn conflict_mode_parse() {
        assert_eq!(ConflictMode::parse("auto"), Some(ConflictMode::Auto));
        assert_eq!(ConflictMode::parse("review"), Some(ConflictMode::Review));
        assert_eq!(ConflictMode::parse("unknown"), None);
    }
}
