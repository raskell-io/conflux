//! Show who changed what (per-field attribution).

use crate::config::Config;
use crate::output::{OutputFormat, Table};
use anyhow::Result;
use clap::Args;
use conflux_core::OperationKind;
use conflux_store::SqliteStore;
use std::collections::HashMap;
use std::path::Path;

/// Show who changed what (per-field attribution).
#[derive(Debug, Args)]
pub struct BlameArgs {
    /// Entity ID to blame.
    pub entity_id: String,

    /// Specific field to blame (omit for all fields).
    #[arg(long, short)]
    pub field: Option<String>,

    /// Output format (plain, json, yaml).
    #[arg(long, short, default_value = "plain")]
    pub output: OutputFormat,
}

/// Field blame information.
struct FieldBlame {
    field: String,
    value: String,
    actor: String,
    timestamp: String,
    intent: Option<String>,
}

/// Runs the blame command.
pub fn run(args: BlameArgs, config: &Config, config_dir: &Path) -> Result<()> {
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

    // Query operations for this entity
    let stored_ops = store.query_operations(
        &conflux_store::OperationQuery::new(&config.document_id)
            .for_entity(&args.entity_id)
            .limit(10000),
    )?;

    if stored_ops.is_empty() {
        println!("No operations found for entity '{}'", args.entity_id);
        return Ok(());
    }

    // Track the last operation that set each field
    let mut field_blame: HashMap<String, FieldBlame> = HashMap::new();
    let mut entity_creator: Option<FieldBlame> = None;

    for stored in &stored_ops {
        let op = &stored.operation;

        match &op.kind {
            OperationKind::InsertEntity { entity_id, .. } => {
                if entity_id.to_string() == args.entity_id {
                    entity_creator = Some(FieldBlame {
                        field: "(created)".to_string(),
                        value: "entity".to_string(),
                        actor: op.actor.id.clone(),
                        timestamp: op.timestamp.to_string(),
                        intent: op.intent.clone(),
                    });
                }
            }
            OperationKind::SetField {
                entity_id,
                field,
                value,
            } => {
                if entity_id.to_string() == args.entity_id
                    && (args.field.is_none() || args.field.as_ref() == Some(field))
                {
                    field_blame.insert(
                        field.clone(),
                        FieldBlame {
                            field: field.clone(),
                            value: format!("{:?}", value),
                            actor: op.actor.id.clone(),
                            timestamp: op.timestamp.to_string(),
                            intent: op.intent.clone(),
                        },
                    );
                }
            }
            OperationKind::RemoveEntity { entity_id } => {
                if entity_id.to_string() == args.entity_id {
                    field_blame.insert(
                        "(deleted)".to_string(),
                        FieldBlame {
                            field: "(deleted)".to_string(),
                            value: "true".to_string(),
                            actor: op.actor.id.clone(),
                            timestamp: op.timestamp.to_string(),
                            intent: op.intent.clone(),
                        },
                    );
                }
            }
            _ => {}
        }
    }

    // Collect all blame entries
    let mut entries: Vec<FieldBlame> = Vec::new();
    if let Some(creator) = entity_creator {
        entries.push(creator);
    }
    for (_, blame) in field_blame {
        entries.push(blame);
    }

    // Sort by field name
    entries.sort_by(|a, b| a.field.cmp(&b.field));

    if entries.is_empty() {
        println!("No blame information found for entity '{}'", args.entity_id);
        return Ok(());
    }

    match args.output {
        OutputFormat::Plain => {
            println!("Blame for entity: {}", args.entity_id);
            println!();

            let mut table = Table::new(vec!["Field", "Value", "Actor", "Intent"]);
            for entry in &entries {
                table.add_row(vec![
                    entry.field.clone(),
                    truncate(&entry.value, 30),
                    entry.actor.clone(),
                    entry.intent.clone().unwrap_or_default(),
                ]);
            }
            table.print();
        }
        OutputFormat::Json | OutputFormat::Yaml => {
            let blame_list: Vec<serde_json::Value> = entries
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "field": e.field,
                        "value": e.value,
                        "actor": e.actor,
                        "timestamp": e.timestamp,
                        "intent": e.intent,
                    })
                })
                .collect();

            let value = serde_json::json!({
                "entity_id": args.entity_id,
                "blame": blame_list,
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

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
