//! Show operation log.

use crate::config::Config;
use crate::output::{OutputFormat, Table};
use anyhow::Result;
use clap::Args;
use conflux_store::SqliteStore;
use std::path::Path;

/// Show the operation log.
#[derive(Debug, Args)]
pub struct LogArgs {
    /// Filter by entity ID.
    #[arg(long, short)]
    pub entity: Option<String>,

    /// Filter by actor ID.
    #[arg(long, short)]
    pub actor: Option<String>,

    /// Maximum number of entries to show.
    #[arg(long, short, default_value = "50")]
    pub limit: u32,

    /// Output format (plain, json, yaml).
    #[arg(long, short, default_value = "plain")]
    pub output: OutputFormat,

    /// Show full operation details.
    #[arg(long)]
    pub verbose: bool,
}

/// Runs the log command.
pub fn run(args: LogArgs, config: &Config, config_dir: &Path) -> Result<()> {
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

    // Build query
    let mut query = conflux_store::OperationQuery::new(&config.document_id).limit(args.limit);

    if let Some(ref entity) = args.entity {
        query = query.for_entity(entity);
    }
    if let Some(ref actor) = args.actor {
        query = query.by_actor(actor);
    }

    let stored_ops = store.query_operations(&query)?;

    if stored_ops.is_empty() {
        println!("No operations found.");
        return Ok(());
    }

    match args.output {
        OutputFormat::Plain => {
            if args.verbose {
                for stored in &stored_ops {
                    let op = &stored.operation;
                    println!("ID:        {}", op.id);
                    println!("Timestamp: {}", op.timestamp);
                    println!("Actor:     {} ({})", op.actor.id, op.actor.class);
                    if let Some(ref intent) = op.intent {
                        println!("Intent:    {}", intent);
                    }
                    println!("Operation: {}", format_operation_kind(&op.kind));
                    println!("---");
                }
            } else {
                let mut table = Table::new(vec!["Timestamp", "Actor", "Operation", "Intent"]);
                for stored in &stored_ops {
                    let op = &stored.operation;
                    table.add_row(vec![
                        format_timestamp(&op.timestamp),
                        op.actor.id.clone(),
                        format_operation_kind_short(&op.kind),
                        op.intent.clone().unwrap_or_default(),
                    ]);
                }
                table.print();
            }
        }
        OutputFormat::Json | OutputFormat::Yaml => {
            let entries: Vec<serde_json::Value> = stored_ops
                .iter()
                .map(|stored| {
                    let op = &stored.operation;
                    serde_json::json!({
                        "id": op.id.to_string(),
                        "timestamp": op.timestamp.to_string(),
                        "actor_id": op.actor.id,
                        "actor_class": op.actor.class.to_string(),
                        "intent": op.intent,
                        "operation": format_operation_kind(&op.kind),
                    })
                })
                .collect();

            let value = serde_json::Value::Array(entries);
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

    let total = store.operation_count(&config.document_id)?;
    if total > stored_ops.len() as u64 {
        println!("\nShowing {} of {} operations", stored_ops.len(), total);
    }

    Ok(())
}

fn format_timestamp(ts: &conflux_core::HlcTimestamp) -> String {
    // Extract just the time portion for display
    let s = ts.to_string();
    if s.len() > 20 {
        s[..20].to_string()
    } else {
        s
    }
}

fn format_operation_kind(kind: &conflux_core::OperationKind) -> String {
    use conflux_core::OperationKind;
    match kind {
        OperationKind::SetField {
            entity_id,
            field,
            value,
        } => {
            format!("set_field({}.{} = {:?})", entity_id, field, value)
        }
        OperationKind::InsertEntity {
            entity_id,
            entity_type,
            parent_id,
            ..
        } => {
            if let Some(parent) = parent_id {
                format!(
                    "insert_entity({} type={} parent={})",
                    entity_id, entity_type, parent
                )
            } else {
                format!("insert_entity({} type={})", entity_id, entity_type)
            }
        }
        OperationKind::RemoveEntity { entity_id } => {
            format!("remove_entity({})", entity_id)
        }
        OperationKind::MoveEntity {
            entity_id,
            new_parent_id,
            new_position,
        } => {
            format!(
                "move_entity({} -> {} at {})",
                entity_id, new_parent_id, new_position
            )
        }
        OperationKind::SetOverride {
            entity_id,
            field,
            environment,
            value,
        } => {
            format!(
                "set_override({}.{}[{}] = {:?})",
                entity_id, field, environment, value
            )
        }
    }
}

fn format_operation_kind_short(kind: &conflux_core::OperationKind) -> String {
    use conflux_core::OperationKind;
    match kind {
        OperationKind::SetField {
            entity_id, field, ..
        } => {
            format!("set {}.{}", entity_id, field)
        }
        OperationKind::InsertEntity {
            entity_id,
            entity_type,
            ..
        } => {
            format!("insert {} ({})", entity_id, entity_type)
        }
        OperationKind::RemoveEntity { entity_id } => {
            format!("remove {}", entity_id)
        }
        OperationKind::MoveEntity { entity_id, .. } => {
            format!("move {}", entity_id)
        }
        OperationKind::SetOverride {
            entity_id,
            field,
            environment,
            ..
        } => {
            format!("override {}.{}[{}]", entity_id, field, environment)
        }
    }
}
