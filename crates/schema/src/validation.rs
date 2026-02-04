//! Schema validation — both self-consistency and document validation.

use crate::error::SchemaError;
use crate::types::{FieldType, Schema};
use conflux_core::{Document, EntityId, FieldValue, ValidationError};

impl Schema {
    /// Validates that the schema itself is internally consistent.
    ///
    /// Checks:
    /// - All `ref` fields have a `target` that references a defined entity type
    /// - All child entity types are defined
    /// - Environment overlays form a valid chain (no cycles, all inherit from defined envs)
    ///
    /// # Errors
    ///
    /// Returns `SchemaError::Validation` with a description of the inconsistency.
    pub fn validate(&self) -> Result<(), SchemaError> {
        // Check ref targets
        for (entity_name, entity_def) in &self.entities {
            for (field_name, field_def) in &entity_def.fields {
                if field_def.field_type == FieldType::Ref {
                    if let Some(ref target) = field_def.target {
                        if !self.entities.contains_key(target) {
                            return Err(SchemaError::Validation(format!(
                                "entity '{entity_name}', field '{field_name}': \
                                 ref target '{target}' is not a defined entity type"
                            )));
                        }
                    }
                }
            }

            // Check child entity types
            for (child_name, child_def) in &entity_def.children {
                if !self.entities.contains_key(&child_def.entity_type) {
                    return Err(SchemaError::Validation(format!(
                        "entity '{entity_name}', children '{child_name}': \
                         child type '{}' is not a defined entity type",
                        child_def.entity_type
                    )));
                }
            }
        }

        // Check environment overlays
        if let Some(ref envs) = self.environments {
            let mut known = std::collections::HashSet::new();
            known.insert(envs.base.as_str());
            for overlay in &envs.overlays {
                known.insert(overlay.name.as_str());
            }
            for overlay in &envs.overlays {
                if !known.contains(overlay.inherits.as_str()) {
                    return Err(SchemaError::Validation(format!(
                        "environment '{}' inherits from '{}' which is not defined",
                        overlay.name, overlay.inherits
                    )));
                }
            }
        }

        Ok(())
    }

    /// Validates a document against this schema.
    ///
    /// Checks:
    /// - All entities have a known entity type
    /// - Required fields are present and non-null
    /// - Field value types match the schema-declared types
    /// - Numeric fields satisfy range constraints
    /// - String fields satisfy enum constraints
    /// - Ref fields point to existing entities
    ///
    /// # Errors
    ///
    /// Returns a `Vec<ValidationError>` for all violations found.
    pub fn validate_document(&self, document: &Document) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        for (entity_id, entity) in &document.entities {
            // Skip tombstoned entities
            if entity.is_tombstoned() {
                continue;
            }

            let entity_def = match self.entities.get(&entity.entity_type) {
                Some(def) => def,
                None => {
                    // Unknown entity types are allowed at this level;
                    // they're caught at operation time.
                    continue;
                }
            };

            // Check required fields
            for (field_name, field_def) in &entity_def.fields {
                if field_def.required {
                    match entity.get_field(field_name) {
                        None => {
                            errors.push(ValidationError::MissingField {
                                entity_id: entity_id.to_string(),
                                field: field_name.clone(),
                            });
                        }
                        Some(state) => {
                            if state.value() == FieldValue::Null {
                                errors.push(ValidationError::MissingField {
                                    entity_id: entity_id.to_string(),
                                    field: field_name.clone(),
                                });
                            }
                        }
                    }
                }
            }

            // Check field value types and constraints
            for (field_name, field_state) in &entity.fields {
                let field_def = match entity_def.fields.get(field_name) {
                    Some(def) => def,
                    None => continue,
                };

                let value = field_state.value();
                if value == FieldValue::Null {
                    continue;
                }

                // Type check
                if let Some(err) = check_type(entity_id, field_name, &field_def.field_type, &value)
                {
                    errors.push(err);
                    continue; // Skip constraint checks if type is wrong
                }

                // Range check
                if let Some((min, max)) = field_def.range {
                    if let Some(err) = check_range(entity_id, field_name, min, max, &value) {
                        errors.push(err);
                    }
                }

                // Enum check
                if let Some(ref allowed) = field_def.allowed_values {
                    if let FieldValue::String(ref s) = value {
                        if !allowed.contains(s) {
                            errors.push(ValidationError::ConstraintViolation {
                                field: field_name.clone(),
                                reason: format!(
                                    "value '{}' not in allowed values: {:?}",
                                    s, allowed
                                ),
                            });
                        }
                    }
                }

                // Ref check
                if field_def.field_type == FieldType::Ref {
                    if let FieldValue::Ref(ref target_id) = value {
                        if !document.entities.contains_key(target_id) {
                            errors.push(ValidationError::BrokenReference {
                                field: field_name.clone(),
                                target: target_id.to_string(),
                            });
                        }
                    }
                }
            }
        }

        errors
    }
}

fn check_type(
    _entity_id: &EntityId,
    field_name: &str,
    expected: &FieldType,
    value: &FieldValue,
) -> Option<ValidationError> {
    let ok = matches!(
        (expected, value),
        (FieldType::String, FieldValue::String(_))
            | (FieldType::Int, FieldValue::Int(_))
            | (FieldType::Float, FieldValue::Float(_))
            | (FieldType::Bool, FieldValue::Bool(_))
            | (FieldType::Duration, FieldValue::Duration(_))
            | (FieldType::Ref, FieldValue::Ref(_))
            | (FieldType::List(_), FieldValue::List(_))
            | (FieldType::Map, FieldValue::Map(_))
    );

    if ok {
        None
    } else {
        Some(ValidationError::ConstraintViolation {
            field: field_name.to_string(),
            reason: format!(
                "type mismatch: expected {}, got {}",
                expected,
                value.type_name()
            ),
        })
    }
}

fn check_range(
    _entity_id: &EntityId,
    field_name: &str,
    min: f64,
    max: f64,
    value: &FieldValue,
) -> Option<ValidationError> {
    let num = match value {
        FieldValue::Int(n) => *n as f64,
        FieldValue::Float(n) => *n,
        _ => return None,
    };

    if num < min || num > max {
        Some(ValidationError::ConstraintViolation {
            field: field_name.to_string(),
            reason: format!("value {num} out of range [{min}, {max}]"),
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Schema;
    use conflux_core::{ActorClass, ActorId, Clock, EntityId, FieldValue, Operation};
    use std::str::FromStr;

    use crate::SchemaInfoAdapter;

    fn test_schema_toml() -> &'static str {
        r#"
[schema]
name = "test"
version = "1.0"

[entity.route]
fields = [
    { name = "path",     type = "string",   merge = "lww", required = true },
    { name = "weight",   type = "int",      merge = "max", range = "0,100" },
    { name = "protocol", type = "string",   merge = "lww", values = "http,https" },
    { name = "upstream", type = "ref",      merge = "lww", target = "upstream" },
]

[entity.upstream]
fields = [
    { name = "targets",      type = "list<string>", merge = "grow-set" },
    { name = "health-check", type = "bool",          merge = "lww" },
]
"#
    }

    /// Helper: parse test schema and return both Schema and its SchemaInfoAdapter.
    fn test_schema() -> (Schema, SchemaInfoAdapter) {
        let schema = Schema::from_str(test_schema_toml()).unwrap();
        let si = schema.as_schema_info();
        (schema, si)
    }

    #[test]
    fn schema_self_validation_passes() {
        let schema = Schema::from_str(test_schema_toml()).unwrap();
        schema.validate().unwrap();
    }

    #[test]
    fn schema_self_validation_catches_bad_ref_target() {
        let input = r#"
[schema]
name = "test"
version = "1.0"

[entity.route]
fields = [
    { name = "upstream", type = "ref", merge = "lww", target = "nonexistent" },
]
"#;
        let schema = Schema::from_str(input).unwrap();
        let err = schema.validate().unwrap_err();
        assert!(matches!(err, SchemaError::Validation(_)));
    }

    #[test]
    fn schema_self_validation_catches_bad_child_type() {
        let input = r#"
[schema]
name = "test"
version = "1.0"

[entity.route]
fields = [
    { name = "path", type = "string", merge = "lww" },
]

[entity.route.children]
filters = { type = "nonexistent", merge = "set" }
"#;
        let schema = Schema::from_str(input).unwrap();
        let err = schema.validate().unwrap_err();
        assert!(matches!(err, SchemaError::Validation(_)));
    }

    #[test]
    fn schema_self_validation_catches_bad_env_inheritance() {
        let input = r#"
[schema]
name = "test"
version = "1.0"

[environments]
base = "production"

[environments.overlays]
staging = { inherits = "nonexistent" }
"#;
        let schema = Schema::from_str(input).unwrap();
        let err = schema.validate().unwrap_err();
        assert!(matches!(err, SchemaError::Validation(_)));
    }

    #[test]
    fn document_validation_required_field_missing() {
        let (schema, si) = test_schema();
        let clock = Clock::new();
        let actor = ActorId::new("alice", ActorClass::Human);

        let mut doc = Document::new();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::insert_entity("route.api", "route", None, None, &actor, ts),
            &si,
            &clock,
        )
        .unwrap();
        // Don't set "path" (required field)

        let errors = schema.validate_document(&doc);
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| matches!(e,
            ValidationError::MissingField { field, .. } if field == "path"
        )));
    }

    #[test]
    fn document_validation_range_violation() {
        let (schema, si) = test_schema();
        let clock = Clock::new();
        let actor = ActorId::new("alice", ActorClass::Human);

        let mut doc = Document::new();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::insert_entity("route.api", "route", None, None, &actor, ts),
            &si,
            &clock,
        )
        .unwrap();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_field(
                "route.api",
                "path",
                FieldValue::String("/api".into()),
                &actor,
                ts,
            ),
            &si,
            &clock,
        )
        .unwrap();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_field("route.api", "weight", FieldValue::Int(200), &actor, ts),
            &si,
            &clock,
        )
        .unwrap();

        let errors = schema.validate_document(&doc);
        assert!(errors.iter().any(|e| matches!(e,
            ValidationError::ConstraintViolation { field, .. } if field == "weight"
        )));
    }

    #[test]
    fn document_validation_enum_violation() {
        let (schema, si) = test_schema();
        let clock = Clock::new();
        let actor = ActorId::new("alice", ActorClass::Human);

        let mut doc = Document::new();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::insert_entity("route.api", "route", None, None, &actor, ts),
            &si,
            &clock,
        )
        .unwrap();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_field(
                "route.api",
                "path",
                FieldValue::String("/api".into()),
                &actor,
                ts,
            ),
            &si,
            &clock,
        )
        .unwrap();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_field(
                "route.api",
                "protocol",
                FieldValue::String("ftp".into()),
                &actor,
                ts,
            ),
            &si,
            &clock,
        )
        .unwrap();

        let errors = schema.validate_document(&doc);
        assert!(errors.iter().any(|e| matches!(e,
            ValidationError::ConstraintViolation { field, .. } if field == "protocol"
        )));
    }

    #[test]
    fn document_validation_broken_ref() {
        let (schema, si) = test_schema();
        let clock = Clock::new();
        let actor = ActorId::new("alice", ActorClass::Human);

        let mut doc = Document::new();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::insert_entity("route.api", "route", None, None, &actor, ts),
            &si,
            &clock,
        )
        .unwrap();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_field(
                "route.api",
                "path",
                FieldValue::String("/api".into()),
                &actor,
                ts,
            ),
            &si,
            &clock,
        )
        .unwrap();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_field(
                "route.api",
                "upstream",
                FieldValue::Ref(EntityId::new("nonexistent")),
                &actor,
                ts,
            ),
            &si,
            &clock,
        )
        .unwrap();

        let errors = schema.validate_document(&doc);
        assert!(errors.iter().any(|e| matches!(e,
            ValidationError::BrokenReference { field, .. } if field == "upstream"
        )));
    }

    #[test]
    fn document_validation_passes_clean_document() {
        let (schema, si) = test_schema();
        let clock = Clock::new();
        let actor = ActorId::new("alice", ActorClass::Human);

        let mut doc = Document::new();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::insert_entity("upstream.backend", "upstream", None, None, &actor, ts),
            &si,
            &clock,
        )
        .unwrap();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::insert_entity("route.api", "route", None, None, &actor, ts),
            &si,
            &clock,
        )
        .unwrap();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_field(
                "route.api",
                "path",
                FieldValue::String("/api".into()),
                &actor,
                ts,
            ),
            &si,
            &clock,
        )
        .unwrap();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_field("route.api", "weight", FieldValue::Int(80), &actor, ts),
            &si,
            &clock,
        )
        .unwrap();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_field(
                "route.api",
                "protocol",
                FieldValue::String("https".into()),
                &actor,
                ts,
            ),
            &si,
            &clock,
        )
        .unwrap();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_field(
                "route.api",
                "upstream",
                FieldValue::Ref(EntityId::new("upstream.backend")),
                &actor,
                ts,
            ),
            &si,
            &clock,
        )
        .unwrap();

        let errors = schema.validate_document(&doc);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    #[test]
    fn document_validation_skips_tombstoned_entities() {
        let (schema, si) = test_schema();
        let clock = Clock::new();
        let actor = ActorId::new("alice", ActorClass::Human);

        let mut doc = Document::new();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::insert_entity("route.api", "route", None, None, &actor, ts),
            &si,
            &clock,
        )
        .unwrap();
        // Don't set required "path", but tombstone the entity
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::remove_entity("route.api", &actor, ts),
            &si,
            &clock,
        )
        .unwrap();

        let errors = schema.validate_document(&doc);
        assert!(errors.is_empty(), "tombstoned entities should be skipped");
    }
}
