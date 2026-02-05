//! Bulk set a field value across multiple entities.

use crate::actor::resolve_actor;
use crate::config::Config;
use anyhow::{Context, Result};
use clap::Args;
use conflux_core::{Clock, Document, FieldValue, Operation};
use conflux_schema::{FieldType, Schema};
use conflux_store::SqliteStore;
use std::path::Path;

/// Bulk set a field value across multiple entities matching a pattern.
#[derive(Debug, Args)]
pub struct BulkSetArgs {
    /// Pattern to match entity IDs (e.g., "route.*", "service.api-*").
    /// Uses glob-style matching.
    pub pattern: String,

    /// Field name to set.
    pub field: String,

    /// Value to set (interpreted based on field type).
    pub value: String,

    /// Actor identity for the operations.
    #[arg(long)]
    pub actor: Option<String>,

    /// Actor class (human, pipeline, operator, system).
    #[arg(long)]
    pub actor_class: Option<String>,

    /// Intent message explaining why this change is being made.
    #[arg(long, short)]
    pub intent: Option<String>,

    /// Dry run - show what would be changed without making changes.
    #[arg(long)]
    pub dry_run: bool,

    /// Filter by entity type (only update entities of this type).
    #[arg(long, short = 't')]
    pub entity_type: Option<String>,
}

/// Runs the bulk-set command.
pub fn run(args: BulkSetArgs, config: &Config, config_dir: &Path) -> Result<()> {
    let actor = resolve_actor(args.actor.as_deref(), args.actor_class.as_deref())?;

    // Parse the glob pattern
    let pattern = glob::Pattern::new(&args.pattern)
        .with_context(|| format!("invalid pattern: {}", args.pattern))?;

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
        &conflux_store::OperationQuery::new(&config.document_id).limit(100000),
    )?;
    let schema_info = schema.as_schema_info();
    for stored in stored_ops {
        let _ = document.apply(&stored.operation, &schema_info, &clock);
    }

    // Find matching entities
    let matching_entities: Vec<_> = document
        .entities
        .iter()
        .filter(|(id, entity)| {
            // Check pattern match
            if !pattern.matches(id.as_str()) {
                return false;
            }
            // Check entity type filter if specified
            if let Some(ref entity_type) = args.entity_type {
                if &entity.entity_type != entity_type {
                    return false;
                }
            }
            true
        })
        .map(|(id, entity)| (id.clone(), entity.entity_type.clone()))
        .collect();

    if matching_entities.is_empty() {
        println!("No entities match pattern '{}'", args.pattern);
        return Ok(());
    }

    println!(
        "Found {} entities matching '{}'",
        matching_entities.len(),
        args.pattern
    );

    if args.dry_run {
        println!("\nDry run - would update:");
        for (entity_id, entity_type) in &matching_entities {
            // Look up field definition for this entity type
            let field_def = schema
                .entities
                .get(entity_type)
                .and_then(|e| e.fields.get(&args.field));

            // Parse value based on field type
            let value = parse_value(&args.value, field_def)?;

            // Get current value if any
            let current = document
                .get_entity(entity_id)
                .and_then(|e| e.get_field(&args.field))
                .map(|v| format!("{:?}", v))
                .unwrap_or_else(|| "(not set)".to_string());

            println!(
                "  {}.{}: {} -> {:?}",
                entity_id.as_str(),
                args.field,
                current,
                value
            );
        }
        return Ok(());
    }

    let mut success_count = 0;
    let mut conflict_count = 0;

    for (entity_id, entity_type) in &matching_entities {
        // Look up field definition for this entity type
        let entity_def = schema.entities.get(entity_type);
        let field_def = entity_def.and_then(|e| e.fields.get(&args.field));

        // Parse value based on field type
        let value = parse_value(&args.value, field_def)?;

        // Create set operation
        let mut set_op = Operation::set_field(
            entity_id.as_str().to_string(),
            args.field.clone(),
            value.clone(),
            &actor,
            clock.new_timestamp(),
        );

        if let Some(ref intent) = args.intent {
            set_op = set_op.with_intent(intent);
        }

        // Apply and store
        let result = document.apply(&set_op, &schema_info, &clock)?;
        store.append_operation(&config.document_id, &set_op)?;

        match result {
            conflux_core::ApplyResult::Applied => {
                success_count += 1;
            }
            conflux_core::ApplyResult::Conflict(_) => {
                conflict_count += 1;
            }
        }
    }

    println!(
        "\nUpdated {} entities ({} clean, {} conflicts)",
        matching_entities.len(),
        success_count,
        conflict_count
    );

    if conflict_count > 0 {
        println!("Run 'conflux conflicts' to view conflicts requiring review.");
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
