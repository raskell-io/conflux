//! TOML schema parser.
//!
//! Parses a TOML schema file into a [`Schema`] struct.

use crate::error::SchemaError;
use crate::types::{
    ChildDef, ConflictMode, EntityDef, EnvironmentsDef, FieldDef, FieldType, OverlayDef, Schema,
};
use conflux_core::MergeStrategy;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

/// Raw TOML structure for deserialization.
#[derive(Debug, Deserialize)]
struct RawSchema {
    schema: RawSchemaHeader,
    #[serde(default)]
    entity: HashMap<String, RawEntityDef>,
    environments: Option<RawEnvironments>,
}

#[derive(Debug, Deserialize)]
struct RawSchemaHeader {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct RawEntityDef {
    #[serde(default)]
    fields: Vec<RawFieldDef>,
    #[serde(default)]
    children: Option<HashMap<String, RawChildDef>>,
}

#[derive(Debug, Deserialize)]
struct RawFieldDef {
    name: String,
    #[serde(rename = "type")]
    field_type: String,
    merge: String,
    #[serde(default)]
    conflict: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    required: Option<bool>,
    #[serde(default)]
    range: Option<String>,
    #[serde(default)]
    values: Option<String>,
    #[serde(default)]
    pattern: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawChildDef {
    #[serde(rename = "type")]
    entity_type: String,
    merge: String,
}

#[derive(Debug, Deserialize)]
struct RawEnvironments {
    base: String,
    #[serde(default)]
    overlays: Option<HashMap<String, RawOverlayDef>>,
}

#[derive(Debug, Deserialize)]
struct RawOverlayDef {
    inherits: String,
}

impl std::str::FromStr for Schema {
    type Err = SchemaError;

    fn from_str(input: &str) -> Result<Self, SchemaError> {
        let raw: RawSchema = toml::from_str(input)?;
        Self::from_raw(raw)
    }
}

impl Schema {
    /// Parses a schema from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns `SchemaError` if the file cannot be read or the content is invalid.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, SchemaError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_str(&content)
    }

    fn from_raw(raw: RawSchema) -> Result<Self, SchemaError> {
        let mut entities = HashMap::new();

        for (entity_name, raw_entity) in raw.entity {
            let entity_def = Self::parse_entity(&entity_name, raw_entity)?;
            entities.insert(entity_name, entity_def);
        }

        let environments = raw.environments.map(Self::parse_environments).transpose()?;

        Ok(Schema {
            name: raw.schema.name,
            version: raw.schema.version,
            entities,
            environments,
        })
    }

    fn parse_entity(entity_name: &str, raw: RawEntityDef) -> Result<EntityDef, SchemaError> {
        let mut fields = HashMap::new();

        for raw_field in raw.fields {
            let field_def = Self::parse_field(entity_name, &raw_field)?;
            fields.insert(raw_field.name.clone(), field_def);
        }

        let mut children = HashMap::new();
        if let Some(raw_children) = raw.children {
            for (child_name, raw_child) in raw_children {
                children.insert(
                    child_name,
                    ChildDef {
                        entity_type: raw_child.entity_type,
                        merge: raw_child.merge,
                    },
                );
            }
        }

        Ok(EntityDef {
            name: entity_name.to_string(),
            fields,
            children,
        })
    }

    fn parse_field(entity_name: &str, raw: &RawFieldDef) -> Result<FieldDef, SchemaError> {
        let field_type =
            FieldType::parse(&raw.field_type).ok_or_else(|| SchemaError::UnknownFieldType {
                entity_type: entity_name.to_string(),
                field: raw.name.clone(),
                field_type: raw.field_type.clone(),
            })?;

        let merge_strategy =
            parse_merge_strategy(&raw.merge).ok_or_else(|| SchemaError::UnknownMergeStrategy {
                entity_type: entity_name.to_string(),
                field: raw.name.clone(),
                strategy: raw.merge.clone(),
            })?;

        let conflict = match &raw.conflict {
            Some(mode) => {
                ConflictMode::parse(mode).ok_or_else(|| SchemaError::UnknownConflictMode {
                    entity_type: entity_name.to_string(),
                    field: raw.name.clone(),
                    mode: mode.clone(),
                })?
            }
            None => ConflictMode::default(),
        };

        let range = raw.range.as_deref().map(parse_range).transpose()?;

        let allowed_values = raw
            .values
            .as_ref()
            .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());

        Ok(FieldDef {
            name: raw.name.clone(),
            field_type,
            merge_strategy,
            conflict,
            target: raw.target.clone(),
            required: raw.required.unwrap_or(false),
            range,
            allowed_values,
            pattern: raw.pattern.clone(),
        })
    }

    fn parse_environments(raw: RawEnvironments) -> Result<EnvironmentsDef, SchemaError> {
        let overlays = match raw.overlays {
            Some(map) => map
                .into_iter()
                .map(|(name, overlay)| OverlayDef {
                    name,
                    inherits: overlay.inherits,
                })
                .collect(),
            None => Vec::new(),
        };

        Ok(EnvironmentsDef {
            base: raw.base,
            overlays,
        })
    }
}

fn parse_merge_strategy(s: &str) -> Option<MergeStrategy> {
    match s {
        "lww" => Some(MergeStrategy::Lww),
        "max" => Some(MergeStrategy::Max),
        "min" => Some(MergeStrategy::Min),
        "grow-set" => Some(MergeStrategy::GrowSet),
        "or-set" => Some(MergeStrategy::OrSet),
        "review" => Some(MergeStrategy::Review),
        _ => None,
    }
}

fn parse_range(s: &str) -> Result<(f64, f64), SchemaError> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 2 {
        return Err(SchemaError::InvalidValue(format!(
            "range must be 'min,max', got '{s}'"
        )));
    }
    let min: f64 = parts[0]
        .trim()
        .parse()
        .map_err(|_| SchemaError::InvalidValue(format!("invalid range min: '{}'", parts[0])))?;
    let max: f64 = parts[1]
        .trim()
        .parse()
        .map_err(|_| SchemaError::InvalidValue(format!("invalid range max: '{}'", parts[1])))?;
    Ok((min, max))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASIC_SCHEMA: &str = r#"
[schema]
name = "test-config"
version = "1.0"

[entity.route]
fields = [
    { name = "path",    type = "string",   merge = "lww" },
    { name = "weight",  type = "int",      merge = "max", range = "0,100" },
    { name = "timeout", type = "duration", merge = "lww" },
]

[entity.upstream]
fields = [
    { name = "targets",      type = "list<string>", merge = "grow-set" },
    { name = "health-check", type = "bool",          merge = "lww" },
]
"#;

    #[test]
    fn parse_basic_schema() {
        let schema = Schema::from_str(BASIC_SCHEMA).unwrap();
        assert_eq!(schema.name, "test-config");
        assert_eq!(schema.version, "1.0");
        assert_eq!(schema.entities.len(), 2);
        assert!(schema.entities.contains_key("route"));
        assert!(schema.entities.contains_key("upstream"));
    }

    #[test]
    fn parse_entity_fields() {
        let schema = Schema::from_str(BASIC_SCHEMA).unwrap();
        let route = &schema.entities["route"];
        assert_eq!(route.fields.len(), 3);

        let weight = &route.fields["weight"];
        assert_eq!(weight.field_type, FieldType::Int);
        assert_eq!(weight.merge_strategy, MergeStrategy::Max);
        assert_eq!(weight.range, Some((0.0, 100.0)));
    }

    #[test]
    fn parse_list_field_type() {
        let schema = Schema::from_str(BASIC_SCHEMA).unwrap();
        let upstream = &schema.entities["upstream"];
        let targets = &upstream.fields["targets"];
        assert_eq!(
            targets.field_type,
            FieldType::List(Some(Box::new(FieldType::String)))
        );
        assert_eq!(targets.merge_strategy, MergeStrategy::GrowSet);
    }

    #[test]
    fn parse_conflict_mode() {
        let input = r#"
[schema]
name = "test"
version = "1.0"

[entity.route]
fields = [
    { name = "path", type = "string", merge = "lww", conflict = "review" },
]
"#;
        let schema = Schema::from_str(input).unwrap();
        let path = &schema.entities["route"].fields["path"];
        assert_eq!(path.conflict, ConflictMode::Review);
    }

    #[test]
    fn parse_required_field() {
        let input = r#"
[schema]
name = "test"
version = "1.0"

[entity.route]
fields = [
    { name = "path", type = "string", merge = "lww", required = true },
]
"#;
        let schema = Schema::from_str(input).unwrap();
        let path = &schema.entities["route"].fields["path"];
        assert!(path.required);
    }

    #[test]
    fn parse_allowed_values() {
        let input = r#"
[schema]
name = "test"
version = "1.0"

[entity.listener]
fields = [
    { name = "protocol", type = "string", merge = "lww", values = "http,https" },
]
"#;
        let schema = Schema::from_str(input).unwrap();
        let protocol = &schema.entities["listener"].fields["protocol"];
        assert_eq!(
            protocol.allowed_values,
            Some(vec!["http".to_string(), "https".to_string()])
        );
    }

    #[test]
    fn parse_ref_field_with_target() {
        let input = r#"
[schema]
name = "test"
version = "1.0"

[entity.route]
fields = [
    { name = "upstream", type = "ref", target = "upstream", merge = "lww" },
]
"#;
        let schema = Schema::from_str(input).unwrap();
        let upstream = &schema.entities["route"].fields["upstream"];
        assert_eq!(upstream.field_type, FieldType::Ref);
        assert_eq!(upstream.target, Some("upstream".to_string()));
    }

    #[test]
    fn parse_environments() {
        let input = r#"
[schema]
name = "test"
version = "1.0"

[environments]
base = "production"

[environments.overlays]
staging = { inherits = "production" }
development = { inherits = "staging" }
"#;
        let schema = Schema::from_str(input).unwrap();
        let envs = schema.environments.unwrap();
        assert_eq!(envs.base, "production");
        assert_eq!(envs.overlays.len(), 2);
    }

    #[test]
    fn parse_children() {
        let input = r#"
[schema]
name = "test"
version = "1.0"

[entity.route]
fields = [
    { name = "path", type = "string", merge = "lww" },
]

[entity.route.children]
filters = { type = "filter", merge = "set" }
"#;
        let schema = Schema::from_str(input).unwrap();
        let route = &schema.entities["route"];
        assert_eq!(route.children.len(), 1);
        let filters = &route.children["filters"];
        assert_eq!(filters.entity_type, "filter");
        assert_eq!(filters.merge, "set");
    }

    #[test]
    fn parse_unknown_field_type_errors() {
        let input = r#"
[schema]
name = "test"
version = "1.0"

[entity.route]
fields = [
    { name = "path", type = "foobar", merge = "lww" },
]
"#;
        let err = Schema::from_str(input).unwrap_err();
        assert!(matches!(err, SchemaError::UnknownFieldType { .. }));
    }

    #[test]
    fn parse_unknown_merge_strategy_errors() {
        let input = r#"
[schema]
name = "test"
version = "1.0"

[entity.route]
fields = [
    { name = "path", type = "string", merge = "yolo" },
]
"#;
        let err = Schema::from_str(input).unwrap_err();
        assert!(matches!(err, SchemaError::UnknownMergeStrategy { .. }));
    }

    #[test]
    fn parse_invalid_range_errors() {
        let input = r#"
[schema]
name = "test"
version = "1.0"

[entity.route]
fields = [
    { name = "weight", type = "int", merge = "max", range = "abc,def" },
]
"#;
        let err = Schema::from_str(input).unwrap_err();
        assert!(matches!(err, SchemaError::InvalidValue(_)));
    }

    #[test]
    fn parse_pattern_field() {
        let input = r#"
[schema]
name = "test"
version = "1.0"

[entity.route]
fields = [
    { name = "path", type = "string", merge = "lww", pattern = "^/[a-z]+" },
]
"#;
        let schema = Schema::from_str(input).unwrap();
        let path = &schema.entities["route"].fields["path"];
        assert_eq!(path.pattern, Some("^/[a-z]+".to_string()));
    }

    #[test]
    fn parse_schema_without_environments() {
        let schema = Schema::from_str(BASIC_SCHEMA).unwrap();
        assert!(schema.environments.is_none());
    }
}
