//! Schema information trait and test helpers.
//!
//! The `SchemaInfo` trait defines what the core crate needs from a schema
//! without depending on the schema crate. The schema crate implements this
//! trait on its `Schema` type.

use crate::field::MergeStrategy;
use std::collections::HashMap;

/// Describes a single field's schema properties.
#[derive(Debug, Clone)]
pub struct FieldSchema {
    /// The field name.
    pub name: String,
    /// The expected type (e.g., "string", "int", "float", "bool", "duration", "ref", "list", "map").
    pub field_type: String,
    /// The merge strategy for this field.
    pub merge_strategy: MergeStrategy,
    /// Whether this field is required.
    pub required: bool,
}

/// Trait providing schema information to the core document model.
///
/// This is the boundary between `conflux-core` and `conflux-schema`.
/// Core only depends on this trait; the schema crate provides the implementation.
pub trait SchemaInfo {
    /// Returns the schema for a specific field on an entity type.
    fn field_schema(&self, entity_type: &str, field_name: &str) -> Option<&FieldSchema>;

    /// Returns all field names defined for an entity type.
    fn entity_fields(&self, entity_type: &str) -> Option<Vec<&str>>;

    /// Returns whether an entity type is defined in the schema.
    fn has_entity_type(&self, entity_type: &str) -> bool;

    /// Returns the base environment name (the root of the inheritance chain).
    fn base_environment(&self) -> Option<&str> {
        None
    }

    /// Returns the parent environment for inheritance lookup.
    /// Returns None if the environment is the base or doesn't exist.
    fn environment_parent(&self, env: &str) -> Option<&str>;

    /// Returns whether an environment is defined in the schema.
    fn has_environment(&self, env: &str) -> bool {
        self.base_environment() == Some(env) || self.environment_parent(env).is_some()
    }
}

/// A simple HashMap-backed schema info for testing.
///
/// Maps `(entity_type, field_name)` to `FieldSchema`.
#[derive(Debug, Clone, Default)]
pub struct SimpleSchemaInfo {
    fields: HashMap<String, HashMap<String, FieldSchema>>,
    /// Base environment name.
    base_env: Option<String>,
    /// Environment inheritance: maps child env -> parent env.
    env_parents: HashMap<String, String>,
}

impl SimpleSchemaInfo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the base environment.
    pub fn set_base_environment(&mut self, env: impl Into<String>) -> &mut Self {
        self.base_env = Some(env.into());
        self
    }

    /// Adds an environment overlay with inheritance.
    pub fn add_environment(&mut self, env: impl Into<String>, inherits: impl Into<String>) -> &mut Self {
        self.env_parents.insert(env.into(), inherits.into());
        self
    }

    /// Adds a field definition for an entity type.
    pub fn add_field(
        &mut self,
        entity_type: impl Into<String>,
        name: impl Into<String>,
        field_type: impl Into<String>,
        merge_strategy: MergeStrategy,
    ) -> &mut Self {
        let entity_type = entity_type.into();
        let name = name.into();
        let schema = FieldSchema {
            name: name.clone(),
            field_type: field_type.into(),
            merge_strategy,
            required: false,
        };
        self.fields
            .entry(entity_type)
            .or_default()
            .insert(name, schema);
        self
    }

    /// Adds a required field definition for an entity type.
    pub fn add_required_field(
        &mut self,
        entity_type: impl Into<String>,
        name: impl Into<String>,
        field_type: impl Into<String>,
        merge_strategy: MergeStrategy,
    ) -> &mut Self {
        let entity_type = entity_type.into();
        let name = name.into();
        let schema = FieldSchema {
            name: name.clone(),
            field_type: field_type.into(),
            merge_strategy,
            required: true,
        };
        self.fields
            .entry(entity_type)
            .or_default()
            .insert(name, schema);
        self
    }
}

impl SchemaInfo for SimpleSchemaInfo {
    fn field_schema(&self, entity_type: &str, field_name: &str) -> Option<&FieldSchema> {
        self.fields.get(entity_type)?.get(field_name)
    }

    fn entity_fields(&self, entity_type: &str) -> Option<Vec<&str>> {
        self.fields
            .get(entity_type)
            .map(|fields| fields.keys().map(|k| k.as_str()).collect())
    }

    fn has_entity_type(&self, entity_type: &str) -> bool {
        self.fields.contains_key(entity_type)
    }

    fn base_environment(&self) -> Option<&str> {
        self.base_env.as_deref()
    }

    fn environment_parent(&self, env: &str) -> Option<&str> {
        self.env_parents.get(env).map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_schema_info_field_lookup() {
        let mut schema = SimpleSchemaInfo::new();
        schema.add_field("route", "weight", "int", MergeStrategy::Max);

        let field = schema.field_schema("route", "weight").unwrap();
        assert_eq!(field.name, "weight");
        assert_eq!(field.merge_strategy, MergeStrategy::Max);
    }

    #[test]
    fn simple_schema_info_missing_field() {
        let schema = SimpleSchemaInfo::new();
        assert!(schema.field_schema("route", "weight").is_none());
    }

    #[test]
    fn simple_schema_info_entity_fields() {
        let mut schema = SimpleSchemaInfo::new();
        schema.add_field("route", "weight", "int", MergeStrategy::Max);
        schema.add_field("route", "path", "string", MergeStrategy::Lww);

        let fields = schema.entity_fields("route").unwrap();
        assert_eq!(fields.len(), 2);
        assert!(fields.contains(&"weight"));
        assert!(fields.contains(&"path"));
    }

    #[test]
    fn simple_schema_info_has_entity_type() {
        let mut schema = SimpleSchemaInfo::new();
        schema.add_field("route", "weight", "int", MergeStrategy::Max);

        assert!(schema.has_entity_type("route"));
        assert!(!schema.has_entity_type("service"));
    }
}
