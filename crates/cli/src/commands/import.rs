//! Import existing configuration files.

use crate::actor::resolve_actor;
use crate::config::Config;
use anyhow::{Context, Result};
use clap::Args;
use conflux_core::{Clock, Document, EntityId, FieldValue, Operation};
use conflux_schema::Schema;
use conflux_store::SqliteStore;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Import existing configuration files into Conflux.
#[derive(Debug, Args)]
pub struct ImportArgs {
    /// Path to the file or directory to import.
    pub path: PathBuf,

    /// Entity type to import as.
    #[arg(long)]
    pub entity_type: String,

    /// Environment to import into.
    #[arg(long, short, default_value = "production")]
    pub environment: String,

    /// Actor identity for the import operations.
    #[arg(long)]
    pub actor: Option<String>,

    /// Actor class (human, pipeline, operator, system).
    #[arg(long, default_value = "pipeline")]
    pub actor_class: Option<String>,

    /// Intent message for the operations.
    #[arg(long, default_value = "import")]
    pub intent: String,

    /// Dry run - show what would be imported without making changes.
    #[arg(long)]
    pub dry_run: bool,
}

/// Runs the import command.
pub fn run(args: ImportArgs, config: &Config, config_dir: &Path) -> Result<()> {
    let actor = resolve_actor(args.actor.as_deref(), args.actor_class.as_deref())?;

    // Load schema
    let schema_path = config_dir.join(&config.schema);
    let schema = Schema::from_file(&schema_path)
        .with_context(|| format!("loading schema from {}", schema_path.display()))?;

    // Verify entity type exists in schema
    if !schema.entities.contains_key(&args.entity_type) {
        anyhow::bail!(
            "entity type '{}' not found in schema. Available types: {}",
            args.entity_type,
            schema.entities.keys().cloned().collect::<Vec<_>>().join(", ")
        );
    }

    // Read and parse the input file
    let content = std::fs::read_to_string(&args.path)
        .with_context(|| format!("reading {}", args.path.display()))?;

    let parsed = parse_config_file(&args.path, &content)?;

    if args.dry_run {
        println!("Dry run - would import:");
        for (entity_id, fields) in &parsed {
            println!("  Entity: {entity_id} (type: {})", args.entity_type);
            for (field, value) in fields {
                println!("    {field}: {value:?}");
            }
        }
        return Ok(());
    }

    // Open store and load document
    let db_path = config_dir.join(&config.database);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid database path"))?;
    let store = SqliteStore::open(db_path_str)?;

    let clock = Clock::new();
    let mut document = Document::new();

    // Load existing state from store
    let stored_ops = store.query_operations(
        &conflux_store::OperationQuery::new(&config.document_id).limit(10000),
    )?;
    let schema_info = schema.as_schema_info();
    for stored in stored_ops {
        let _ = document.apply(&stored.operation, &schema_info, &clock);
    }

    let mut operation_count = 0;

    // Create operations for each entity
    for (entity_id, fields) in parsed {
        let entity_id_obj = EntityId::new(&entity_id);

        // Insert entity if it doesn't exist
        if document.get_entity(&entity_id_obj).is_none() {
            let insert_op = Operation::insert_entity(
                entity_id.clone(),
                args.entity_type.clone(),
                None,
                None,
                &actor,
                clock.new_timestamp(),
            )
            .with_intent(&args.intent);

            document.apply(&insert_op, &schema_info, &clock)?;
            store.append_operation(&config.document_id, &insert_op)?;
            operation_count += 1;
        }

        // Set fields
        for (field_name, value) in fields {
            let set_op = Operation::set_field(
                entity_id.clone(),
                field_name,
                value,
                &actor,
                clock.new_timestamp(),
            )
            .with_intent(&args.intent);

            document.apply(&set_op, &schema_info, &clock)?;
            store.append_operation(&config.document_id, &set_op)?;
            operation_count += 1;
        }
    }

    println!(
        "Imported {} operations from {}",
        operation_count,
        args.path.display()
    );

    Ok(())
}

/// Parses a configuration file and returns entity ID to fields mapping.
fn parse_config_file(
    path: &Path,
    content: &str,
) -> Result<HashMap<String, HashMap<String, FieldValue>>> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let value: serde_json::Value = match extension.as_str() {
        "json" => serde_json::from_str(content).context("parsing JSON")?,
        "yaml" | "yml" => serde_yaml::from_str(content).context("parsing YAML")?,
        "toml" => toml::from_str::<toml::Value>(content)
            .map(|v| serde_json::to_value(v).unwrap())
            .context("parsing TOML")?,
        _ => anyhow::bail!("unsupported file format: {extension}. Supported: json, yaml, toml"),
    };

    // Convert JSON to entity map
    json_to_entities(&value)
}

/// Converts a JSON value to entity ID -> fields mapping.
fn json_to_entities(
    value: &serde_json::Value,
) -> Result<HashMap<String, HashMap<String, FieldValue>>> {
    let mut result = HashMap::new();

    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                match val {
                    serde_json::Value::Object(fields) => {
                        let mut field_map = HashMap::new();
                        for (field_name, field_value) in fields {
                            let fv = json_to_field_value(field_value);
                            field_map.insert(field_name.clone(), fv);
                        }
                        result.insert(key.clone(), field_map);
                    }
                    _ => {
                        // Top-level primitive becomes a field on a "default" entity
                        let entry = result.entry("default".to_string()).or_default();
                        entry.insert(key.clone(), json_to_field_value(val));
                    }
                }
            }
        }
        _ => anyhow::bail!("expected object at root of config file"),
    }

    Ok(result)
}

/// Converts a JSON value to a FieldValue.
fn json_to_field_value(value: &serde_json::Value) -> FieldValue {
    match value {
        serde_json::Value::String(s) => FieldValue::String(s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                FieldValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                FieldValue::Float(f)
            } else {
                FieldValue::Null
            }
        }
        serde_json::Value::Bool(b) => FieldValue::Bool(*b),
        serde_json::Value::Array(arr) => {
            FieldValue::List(arr.iter().map(json_to_field_value).collect())
        }
        serde_json::Value::Object(map) => FieldValue::Map(
            map.iter()
                .map(|(k, v)| (k.clone(), json_to_field_value(v)))
                .collect(),
        ),
        serde_json::Value::Null => FieldValue::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_config() {
        let content = r#"{"service1": {"replicas": 3, "image": "nginx"}}"#;
        let path = PathBuf::from("test.json");
        let result = parse_config_file(&path, content).unwrap();

        assert!(result.contains_key("service1"));
        let fields = result.get("service1").unwrap();
        assert_eq!(fields.get("replicas"), Some(&FieldValue::Int(3)));
        assert_eq!(
            fields.get("image"),
            Some(&FieldValue::String("nginx".to_string()))
        );
    }

    #[test]
    fn parse_yaml_config() {
        let content = "service1:\n  replicas: 3\n  image: nginx";
        let path = PathBuf::from("test.yaml");
        let result = parse_config_file(&path, content).unwrap();

        assert!(result.contains_key("service1"));
    }
}
