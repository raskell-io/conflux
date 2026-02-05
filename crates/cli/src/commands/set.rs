//! Set a field value on an entity.

use crate::actor::resolve_actor;
use crate::config::Config;
use anyhow::{Context, Result};
use clap::Args;
use conflux_core::{Clock, Document, EntityId, FieldValue, Operation};
use conflux_schema::{FieldType, Schema};
use conflux_store::SqliteStore;
use std::path::Path;

/// Set a field value on an entity.
#[derive(Debug, Args)]
pub struct SetArgs {
    /// Entity ID (e.g., "service.api" or "route.frontend").
    pub entity_id: String,

    /// Field name to set.
    pub field: String,

    /// Value to set (interpreted based on field type).
    pub value: String,

    /// Entity type (required if entity doesn't exist).
    #[arg(long, short = 't')]
    pub entity_type: Option<String>,

    /// Actor identity for the operation.
    #[arg(long)]
    pub actor: Option<String>,

    /// Actor class (human, pipeline, operator, system).
    #[arg(long)]
    pub actor_class: Option<String>,

    /// Intent message explaining why this change is being made.
    #[arg(long, short)]
    pub intent: Option<String>,
}

/// Runs the set command.
pub fn run(args: SetArgs, config: &Config, config_dir: &Path) -> Result<()> {
    let actor = resolve_actor(args.actor.as_deref(), args.actor_class.as_deref())?;

    // Load schema
    let schema_path = config_dir.join(&config.schema);
    let schema = Schema::from_file(&schema_path)
        .with_context(|| format!("loading schema from {}", schema_path.display()))?;

    // Open store
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

    let entity_id = EntityId::new(&args.entity_id);

    // Check if entity exists
    let entity = document.get_entity(&entity_id);

    // If entity doesn't exist, we need to create it
    if entity.is_none() {
        let entity_type = args.entity_type.ok_or_else(|| {
            anyhow::anyhow!(
                "entity '{}' does not exist. Use --entity-type to create it.",
                args.entity_id
            )
        })?;

        // Verify entity type exists in schema
        if !schema.entities.contains_key(&entity_type) {
            anyhow::bail!(
                "entity type '{}' not found in schema. Available types: {}",
                entity_type,
                schema.entities.keys().cloned().collect::<Vec<_>>().join(", ")
            );
        }

        let insert_op = Operation::insert_entity(
            args.entity_id.clone(),
            entity_type.clone(),
            None,
            None,
            &actor,
            clock.new_timestamp(),
        );
        let insert_op = if let Some(ref intent) = args.intent {
            insert_op.with_intent(intent)
        } else {
            insert_op
        };

        document.apply(&insert_op, &schema_info, &clock)?;
        store.append_operation(&config.document_id, &insert_op)?;
        println!(
            "Created entity '{}' of type '{}'",
            args.entity_id, entity_type
        );
    }

    // Get entity type for field type lookup
    let entity = document.get_entity(&entity_id).unwrap();
    let entity_type = &entity.entity_type;

    // Look up field type in schema
    let entity_def = schema
        .entities
        .get(entity_type)
        .ok_or_else(|| anyhow::anyhow!("entity type '{}' not found in schema", entity_type))?;

    let field_def = entity_def.fields.get(&args.field);

    // Parse value based on field type
    let value = parse_value(&args.value, field_def)?;

    // Create set operation
    let mut set_op = Operation::set_field(
        args.entity_id.clone(),
        args.field.clone(),
        value.clone(),
        &actor,
        clock.new_timestamp(),
    );

    if let Some(intent) = args.intent {
        set_op = set_op.with_intent(&intent);
    }

    // Apply and store
    let result = document.apply(&set_op, &schema_info, &clock)?;
    store.append_operation(&config.document_id, &set_op)?;

    match result {
        conflux_core::ApplyResult::Applied => {
            println!("Set {}.{} = {:?}", args.entity_id, args.field, value);
        }
        conflux_core::ApplyResult::Conflict(info) => {
            println!(
                "Set {}.{} = {:?} (conflict detected, requires review)",
                args.entity_id, args.field, value
            );
            println!("Contending values: {:?}", info.contending_values);
        }
    }

    Ok(())
}

/// Parses a string value based on optional field definition.
fn parse_value(s: &str, field_def: Option<&conflux_schema::FieldDef>) -> Result<FieldValue> {
    if let Some(def) = field_def {
        match &def.field_type {
            FieldType::Int => {
                let i: i64 = s.parse().with_context(|| format!("parsing '{s}' as int"))?;
                Ok(FieldValue::Int(i))
            }
            FieldType::Float => {
                let f: f64 = s.parse().with_context(|| format!("parsing '{s}' as float"))?;
                Ok(FieldValue::Float(f))
            }
            FieldType::Bool => {
                let b = match s.to_lowercase().as_str() {
                    "true" | "1" | "yes" => true,
                    "false" | "0" | "no" => false,
                    _ => anyhow::bail!("invalid boolean value: '{s}'"),
                };
                Ok(FieldValue::Bool(b))
            }
            FieldType::String | FieldType::Duration | FieldType::Ref => {
                Ok(FieldValue::String(s.to_string()))
            }
            FieldType::List(_) => {
                // Try to parse as JSON array
                if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(s) {
                    Ok(FieldValue::List(
                        arr.iter().map(json_to_field_value).collect(),
                    ))
                } else {
                    // Split by comma
                    Ok(FieldValue::List(
                        s.split(',')
                            .map(|v| FieldValue::String(v.trim().to_string()))
                            .collect(),
                    ))
                }
            }
            FieldType::Map => {
                let obj: serde_json::Value =
                    serde_json::from_str(s).context("parsing JSON object")?;
                if let serde_json::Value::Object(map) = obj {
                    Ok(FieldValue::Map(
                        map.into_iter()
                            .map(|(k, v)| (k, json_to_field_value(&v)))
                            .collect(),
                    ))
                } else {
                    anyhow::bail!("expected JSON object for map field")
                }
            }
        }
    } else {
        // No field definition, try to infer type
        if let Ok(i) = s.parse::<i64>() {
            Ok(FieldValue::Int(i))
        } else if let Ok(f) = s.parse::<f64>() {
            Ok(FieldValue::Float(f))
        } else if s.eq_ignore_ascii_case("true") {
            Ok(FieldValue::Bool(true))
        } else if s.eq_ignore_ascii_case("false") {
            Ok(FieldValue::Bool(false))
        } else {
            Ok(FieldValue::String(s.to_string()))
        }
    }
}

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
