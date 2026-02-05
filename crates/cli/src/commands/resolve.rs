//! Resolve a conflict by choosing a value.

use crate::actor::resolve_actor;
use crate::config::Config;
use anyhow::{Context, Result};
use clap::Args;
use conflux_core::{Clock, Document, FieldValue, Operation};
use conflux_schema::Schema;
use conflux_store::SqliteStore;
use std::path::Path;

/// Resolve a conflict by choosing a value.
#[derive(Debug, Args)]
pub struct ResolveArgs {
    /// Entity ID with the conflict.
    pub entity_id: String,

    /// Field name with the conflict.
    pub field: String,

    /// The value to set (JSON format).
    pub value: String,

    /// Environment if resolving an override conflict.
    #[arg(long, short)]
    pub env: Option<String>,

    /// Actor identity for the operation.
    #[arg(long)]
    pub actor: Option<String>,

    /// Actor class (human, pipeline, operator, system).
    #[arg(long)]
    pub actor_class: Option<String>,

    /// Intent message explaining the resolution.
    #[arg(long, short)]
    pub intent: Option<String>,

    /// Show conflict details before resolving (interactive mode).
    #[arg(long)]
    pub show: bool,
}

/// Runs the resolve command.
pub fn run(args: ResolveArgs, config: &Config, config_dir: &Path) -> Result<()> {
    // Load schema
    let schema_path = config_dir.join(&config.schema);
    let schema = Schema::from_file(&schema_path)
        .with_context(|| format!("loading schema from {}", schema_path.display()))?;

    // Open store
    let db_path = config_dir.join(&config.database);
    if !db_path.exists() {
        anyhow::bail!(
            "database not found at {}. Run `conflux init` first.",
            db_path.display()
        );
    }
    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid database path"))?;
    let store = SqliteStore::open(db_path_str)?;

    let clock = Clock::new();
    let mut document = Document::new();

    // Load document state
    let stored_ops = store.query_operations(
        &conflux_store::OperationQuery::new(&config.document_id).limit(10000),
    )?;
    let schema_info = schema.as_schema_info();
    for stored in stored_ops {
        let _ = document.apply(&stored.operation, &schema_info, &clock);
    }

    // Find the conflict
    let entity_id = conflux_core::EntityId::new(&args.entity_id);
    let conflicts = document.list_conflicts();
    let conflict = conflicts.iter().find(|c| {
        c.entity_id == entity_id && c.field == args.field && c.environment == args.env
    });

    if args.show {
        match conflict {
            Some(c) => {
                println!("Conflict on {}.{}:", c.entity_id, c.field);
                if let Some(ref env) = c.environment {
                    println!("  Environment: {}", env);
                }
                println!("  Contending values:");
                for (i, value) in c.contending_values.iter().enumerate() {
                    println!("    [{}] {:?}", i + 1, value);
                }
                println!();
            }
            None => {
                anyhow::bail!(
                    "no conflict found on {}.{}{}",
                    args.entity_id,
                    args.field,
                    args.env.as_ref().map(|e| format!("[{}]", e)).unwrap_or_default()
                );
            }
        }
    }

    // Parse the chosen value
    let chosen_value = parse_value(&args.value)?;

    // Resolve actor
    let actor = resolve_actor(args.actor.as_deref(), args.actor_class.as_deref())?;

    // Create the resolve operation
    let mut op = Operation::resolve_conflict(
        entity_id.clone(),
        args.field.clone(),
        args.env.clone(),
        chosen_value.clone(),
        &actor,
        clock.new_timestamp(),
    );

    if let Some(intent) = args.intent {
        op = op.with_intent(intent);
    }

    // Apply and store
    match document.apply(&op, &schema_info, &clock) {
        Ok(_) => {
            store.append_operation(&config.document_id, &op)?;
            println!(
                "Resolved conflict on {}.{}: {:?}",
                args.entity_id, args.field, chosen_value
            );
        }
        Err(e) => {
            anyhow::bail!("failed to resolve conflict: {}", e);
        }
    }

    Ok(())
}

/// Parses a value string into a FieldValue.
fn parse_value(s: &str) -> Result<FieldValue> {
    // Try JSON first
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(s) {
        return Ok(json_to_field_value(&json));
    }

    // Try as integer
    if let Ok(i) = s.parse::<i64>() {
        return Ok(FieldValue::Int(i));
    }

    // Try as float
    if let Ok(f) = s.parse::<f64>() {
        return Ok(FieldValue::Float(f));
    }

    // Try as boolean
    if s == "true" {
        return Ok(FieldValue::Bool(true));
    }
    if s == "false" {
        return Ok(FieldValue::Bool(false));
    }

    // Try as null
    if s == "null" {
        return Ok(FieldValue::Null);
    }

    // Default to string
    Ok(FieldValue::String(s.to_string()))
}

fn json_to_field_value(json: &serde_json::Value) -> FieldValue {
    match json {
        serde_json::Value::Null => FieldValue::Null,
        serde_json::Value::Bool(b) => FieldValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                FieldValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                FieldValue::Float(f)
            } else {
                FieldValue::Null
            }
        }
        serde_json::Value::String(s) => FieldValue::String(s.clone()),
        serde_json::Value::Array(arr) => {
            FieldValue::List(arr.iter().map(json_to_field_value).collect())
        }
        serde_json::Value::Object(obj) => {
            let map: std::collections::BTreeMap<String, FieldValue> = obj
                .iter()
                .map(|(k, v)| (k.clone(), json_to_field_value(v)))
                .collect();
            FieldValue::Map(map)
        }
    }
}
