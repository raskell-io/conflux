//! List and display unresolved conflicts.

use crate::config::Config;
use crate::output::{OutputFormat, Table};
use anyhow::{Context, Result};
use clap::Args;
use conflux_core::{Clock, Document};
use conflux_schema::Schema;
use conflux_store::SqliteStore;
use std::path::Path;

/// List unresolved conflicts in the document.
#[derive(Debug, Args)]
pub struct ConflictsArgs {
    /// Filter by entity ID (partial match).
    #[arg(long, short)]
    pub entity: Option<String>,

    /// Filter by field name.
    #[arg(long, short)]
    pub field: Option<String>,

    /// Output format.
    #[arg(long, short, default_value = "plain")]
    pub output: OutputFormat,
}

/// Runs the conflicts command.
pub fn run(args: ConflictsArgs, config: &Config, config_dir: &Path) -> Result<()> {
    // Load schema
    let schema_path = config_dir.join(&config.schema);
    let schema = Schema::from_file(&schema_path)
        .with_context(|| format!("loading schema from {}", schema_path.display()))?;

    // Open store
    let db_path = config_dir.join(&config.database);
    if !db_path.exists() {
        println!("No conflicts (database not initialized).");
        return Ok(());
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

    // Get all conflicts
    let mut conflicts = document.list_conflicts();

    // Apply filters
    if let Some(ref entity_filter) = args.entity {
        conflicts.retain(|c| c.entity_id.to_string().contains(entity_filter));
    }
    if let Some(ref field_filter) = args.field {
        conflicts.retain(|c| c.field.contains(field_filter));
    }

    if conflicts.is_empty() {
        println!("No unresolved conflicts.");
        return Ok(());
    }

    match args.output {
        OutputFormat::Plain => {
            let mut table = Table::new(vec!["Entity", "Field", "Environment", "Contending Values"]);
            for conflict in &conflicts {
                let env = conflict.environment.as_deref().unwrap_or("(base)");
                let values: Vec<String> = conflict
                    .contending_values
                    .iter()
                    .map(|v| format!("{:?}", v))
                    .collect();
                table.add_row(vec![
                    conflict.entity_id.to_string(),
                    conflict.field.clone(),
                    env.to_string(),
                    values.join(", "),
                ]);
            }
            table.print();
            println!();
            println!("{} unresolved conflict(s).", conflicts.len());
        }
        OutputFormat::Json | OutputFormat::Yaml => {
            let list: Vec<serde_json::Value> = conflicts
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "entity_id": c.entity_id.to_string(),
                        "field": c.field,
                        "environment": c.environment,
                        "contending_values": c.contending_values.iter()
                            .map(|v| format!("{:?}", v))
                            .collect::<Vec<_>>(),
                    })
                })
                .collect();

            let value = serde_json::json!({
                "conflicts": list,
                "total": conflicts.len(),
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
