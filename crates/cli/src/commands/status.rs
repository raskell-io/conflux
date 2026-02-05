//! Show document status.

use crate::config::Config;
use crate::output::OutputFormat;
use anyhow::{Context, Result};
use clap::Args;
use conflux_core::{Clock, Document};
use conflux_schema::Schema;
use conflux_store::SqliteStore;
use std::path::Path;

/// Show document status.
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Output format (plain, json, yaml).
    #[arg(long, short, default_value = "plain")]
    pub output: OutputFormat,
}

/// Runs the status command.
pub fn run(args: StatusArgs, config: &Config, config_dir: &Path) -> Result<()> {
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

    // Load existing state from store
    let stored_ops = store.query_operations(
        &conflux_store::OperationQuery::new(&config.document_id).limit(10000),
    )?;
    let schema_info = schema.as_schema_info();
    for stored in stored_ops {
        let _ = document.apply(&stored.operation, &schema_info, &clock);
    }

    // Gather statistics
    let total_operations = store.operation_count(&config.document_id)?;
    let entity_count = document.entities.len();
    let active_entities = document
        .entities
        .values()
        .filter(|e| !e.tombstone.value)
        .count();
    let deleted_entities = entity_count - active_entities;

    let milestones = store.list_milestones(&config.document_id)?;
    let milestone_count = milestones.len();
    let latest_milestone = milestones.first();

    let ops_since_milestone = store.operations_since_milestone(&config.document_id)?;

    // Count entities by type
    let mut entity_types: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for entity in document.entities.values() {
        if !entity.tombstone.value {
            *entity_types.entry(entity.entity_type.clone()).or_default() += 1;
        }
    }

    // Count total fields
    let mut total_fields = 0;
    for entity in document.entities.values() {
        total_fields += entity.fields.len();
    }

    match args.output {
        OutputFormat::Plain => {
            println!("Document: {}", config.document_id);
            println!("Schema:   {} v{}", schema.name, schema.version);
            println!();
            println!(
                "Entities: {} ({} active, {} deleted)",
                entity_count, active_entities, deleted_entities
            );
            if !entity_types.is_empty() {
                let mut types: Vec<_> = entity_types.iter().collect();
                types.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
                for (type_name, count) in types {
                    println!("  {}: {}", type_name, count);
                }
            }
            println!();
            println!("Operations: {}", total_operations);
            println!("Fields: {}", total_fields);
            println!("Milestones: {}", milestone_count);
            if let Some(m) = latest_milestone {
                println!(
                    "  Latest: {} ({} ops since)",
                    m.id.to_string().get(..8).unwrap_or(&m.id.to_string()),
                    ops_since_milestone
                );
                if let Some(ref msg) = m.message {
                    println!("  Message: {}", msg);
                }
            }
        }
        OutputFormat::Json | OutputFormat::Yaml => {
            let value = serde_json::json!({
                "document_id": config.document_id,
                "schema": {
                    "name": schema.name,
                    "version": schema.version,
                },
                "entities": {
                    "total": entity_count,
                    "active": active_entities,
                    "deleted": deleted_entities,
                    "by_type": entity_types,
                },
                "operations": {
                    "total": total_operations,
                    "since_milestone": ops_since_milestone,
                },
                "fields": total_fields,
                "milestones": {
                    "count": milestone_count,
                    "latest": latest_milestone.map(|m| serde_json::json!({
                        "id": m.id.to_string(),
                        "message": m.message,
                        "git_commit": m.git_commit,
                        "created_at": m.created_at,
                    })),
                },
            });

            match args.output {
                OutputFormat::Json | OutputFormat::Plain => {
                    println!("{}", serde_json::to_string_pretty(&value)?);
                }
                OutputFormat::Yaml => {
                    print!("{}", serde_yaml::to_string(&value)?);
                }
            }
        }
    }

    Ok(())
}
