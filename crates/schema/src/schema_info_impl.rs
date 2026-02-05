//! Implementation of `conflux_core::SchemaInfo` for `Schema`.

use crate::types::Schema;
use conflux_core::schema_info::{FieldSchema, SchemaInfo};
use std::collections::HashMap;

/// Adapter providing `SchemaInfo` with pre-built field schema lookups.
///
/// Created from a `Schema` via [`Schema::as_schema_info()`]. Owns the
/// `FieldSchema` values so they can be returned by reference through
/// the `SchemaInfo` trait.
pub struct SchemaInfoAdapter {
    fields: HashMap<String, HashMap<String, FieldSchema>>,
    /// Base environment name.
    base_env: Option<String>,
    /// Environment inheritance: maps child env -> parent env.
    env_parents: HashMap<String, String>,
}

impl SchemaInfoAdapter {
    fn from_schema(schema: &Schema) -> Self {
        let mut fields: HashMap<String, HashMap<String, FieldSchema>> = HashMap::new();

        for (entity_name, entity_def) in &schema.entities {
            let mut entity_fields = HashMap::new();
            for (field_name, field_def) in &entity_def.fields {
                entity_fields.insert(
                    field_name.clone(),
                    FieldSchema {
                        name: field_name.clone(),
                        field_type: field_def.field_type.as_core_type_str().to_string(),
                        merge_strategy: field_def.merge_strategy,
                        required: field_def.required,
                    },
                );
            }
            fields.insert(entity_name.clone(), entity_fields);
        }

        // Extract environment configuration
        let (base_env, env_parents) = if let Some(ref envs) = schema.environments {
            let mut parents = HashMap::new();
            for overlay in &envs.overlays {
                parents.insert(overlay.name.clone(), overlay.inherits.clone());
            }
            (Some(envs.base.clone()), parents)
        } else {
            (None, HashMap::new())
        };

        Self {
            fields,
            base_env,
            env_parents,
        }
    }
}

impl SchemaInfo for SchemaInfoAdapter {
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

impl Schema {
    /// Creates a [`SchemaInfoAdapter`] for use with `conflux-core` document operations.
    ///
    /// The adapter implements `SchemaInfo` and owns the field schema data,
    /// allowing it to return references through the trait.
    pub fn as_schema_info(&self) -> SchemaInfoAdapter {
        SchemaInfoAdapter::from_schema(self)
    }
}
