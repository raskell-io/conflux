//! Watch for state changes in real-time.

use crate::config::Config;
use anyhow::{Context, Result};
use clap::Args;
use conflux_core::{Clock, Document};
use conflux_schema::Schema;
use conflux_store::SqliteStore;
use std::path::Path;
use tokio::sync::broadcast;

/// Watch for state changes in real-time.
#[derive(Debug, Args)]
pub struct WatchArgs {
    /// Filter by entity ID prefix.
    #[arg(long, short)]
    pub entity: Option<String>,

    /// Filter by field name.
    #[arg(long, short)]
    pub field: Option<String>,

    /// Output format (plain, json).
    #[arg(long, short, default_value = "plain")]
    pub output: WatchOutput,

    /// Polling interval in milliseconds (for file-based watching).
    #[arg(long, default_value = "1000")]
    pub interval: u64,
}

/// Watch output format.
#[derive(Debug, Clone, Copy, Default)]
pub enum WatchOutput {
    #[default]
    Plain,
    Json,
}

impl std::str::FromStr for WatchOutput {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "plain" => Ok(Self::Plain),
            "json" => Ok(Self::Json),
            _ => Err(format!("unknown output format: {}", s)),
        }
    }
}

/// Runs the watch command.
pub async fn run(args: WatchArgs, config: &Config, config_dir: &Path) -> Result<()> {
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
    let schema_info = schema.as_schema_info();

    // Track operation count to detect changes
    let mut last_count = store.operation_count(&config.document_id)?;
    let mut document = Document::new();

    // Load initial state
    let stored_ops = store.query_operations(
        &conflux_store::OperationQuery::new(&config.document_id).limit(10000),
    )?;
    for stored in stored_ops {
        let _ = document.apply(&stored.operation, &schema_info, &clock);
    }

    println!("Watching for changes... (press Ctrl+C to stop)");
    if let Some(ref entity) = args.entity {
        println!("  Entity filter: {}", entity);
    }
    if let Some(ref field) = args.field {
        println!("  Field filter: {}", field);
    }
    println!();

    // Create channel for graceful shutdown
    let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);

    // Handle Ctrl+C
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        let _ = shutdown_tx_clone.send(());
    });

    let interval = std::time::Duration::from_millis(args.interval);

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                println!("\nStopping watch...");
                break;
            }
            _ = tokio::time::sleep(interval) => {
                // Check for new operations
                let current_count = store.operation_count(&config.document_id)?;
                if current_count > last_count {
                    // Fetch new operations
                    let new_ops = store.query_operations(
                        &conflux_store::OperationQuery::new(&config.document_id)
                            .limit((current_count - last_count) as u32),
                    )?;

                    // Process in chronological order (query returns newest first)
                    for stored in new_ops.into_iter().rev() {
                        let op = &stored.operation;

                        // Apply filters
                        let entity_id = get_entity_id(&op.kind);
                        if let Some(ref filter) = args.entity {
                            if !entity_id.starts_with(filter) {
                                let _ = document.apply(op, &schema_info, &clock);
                                continue;
                            }
                        }
                        if let Some(ref filter) = args.field {
                            let field = get_field(&op.kind);
                            if field.as_ref().map(|f| f != filter).unwrap_or(true) {
                                let _ = document.apply(op, &schema_info, &clock);
                                continue;
                            }
                        }

                        // Print the change
                        match args.output {
                            WatchOutput::Plain => {
                                print_plain_change(op);
                            }
                            WatchOutput::Json => {
                                print_json_change(op);
                            }
                        }

                        let _ = document.apply(op, &schema_info, &clock);
                    }

                    last_count = current_count;
                }
            }
        }
    }

    Ok(())
}

fn get_entity_id(kind: &conflux_core::OperationKind) -> String {
    use conflux_core::OperationKind;
    match kind {
        OperationKind::SetField { entity_id, .. } => entity_id.to_string(),
        OperationKind::InsertEntity { entity_id, .. } => entity_id.to_string(),
        OperationKind::RemoveEntity { entity_id } => entity_id.to_string(),
        OperationKind::MoveEntity { entity_id, .. } => entity_id.to_string(),
        OperationKind::SetOverride { entity_id, .. } => entity_id.to_string(),
        OperationKind::ResolveConflict { entity_id, .. } => entity_id.to_string(),
    }
}

fn get_field(kind: &conflux_core::OperationKind) -> Option<String> {
    use conflux_core::OperationKind;
    match kind {
        OperationKind::SetField { field, .. } => Some(field.clone()),
        OperationKind::SetOverride { field, .. } => Some(field.clone()),
        OperationKind::ResolveConflict { field, .. } => Some(field.clone()),
        _ => None,
    }
}

fn print_plain_change(op: &conflux_core::Operation) {
    use conflux_core::OperationKind;

    let timestamp = format_timestamp(&op.timestamp);
    let actor = &op.actor.id;

    match &op.kind {
        OperationKind::SetField {
            entity_id,
            field,
            value,
        } => {
            println!(
                "[{}] {} set {}.{} = {:?}",
                timestamp, actor, entity_id, field, value
            );
        }
        OperationKind::InsertEntity {
            entity_id,
            entity_type,
            parent_id,
            ..
        } => {
            if let Some(parent) = parent_id {
                println!(
                    "[{}] {} inserted {} ({}) under {}",
                    timestamp, actor, entity_id, entity_type, parent
                );
            } else {
                println!(
                    "[{}] {} inserted {} ({})",
                    timestamp, actor, entity_id, entity_type
                );
            }
        }
        OperationKind::RemoveEntity { entity_id } => {
            println!("[{}] {} removed {}", timestamp, actor, entity_id);
        }
        OperationKind::MoveEntity {
            entity_id,
            new_parent_id,
            ..
        } => {
            println!(
                "[{}] {} moved {} to {}",
                timestamp, actor, entity_id, new_parent_id
            );
        }
        OperationKind::SetOverride {
            entity_id,
            field,
            environment,
            value,
        } => {
            println!(
                "[{}] {} set override {}.{}[{}] = {:?}",
                timestamp, actor, entity_id, field, environment, value
            );
        }
        OperationKind::ResolveConflict {
            entity_id,
            field,
            environment,
            chosen_value,
        } => {
            if let Some(env) = environment {
                println!(
                    "[{}] {} resolved conflict {}.{}[{}] = {:?}",
                    timestamp, actor, entity_id, field, env, chosen_value
                );
            } else {
                println!(
                    "[{}] {} resolved conflict {}.{} = {:?}",
                    timestamp, actor, entity_id, field, chosen_value
                );
            }
        }
    }

    if let Some(ref intent) = op.intent {
        println!("         intent: {}", intent);
    }
}

fn print_json_change(op: &conflux_core::Operation) {
    use conflux_core::OperationKind;

    let (op_type, entity_id, field, value) = match &op.kind {
        OperationKind::SetField {
            entity_id,
            field,
            value,
        } => (
            "set_field",
            entity_id.to_string(),
            Some(field.clone()),
            Some(format!("{:?}", value)),
        ),
        OperationKind::InsertEntity {
            entity_id,
            entity_type,
            ..
        } => (
            "insert_entity",
            entity_id.to_string(),
            None,
            Some(entity_type.clone()),
        ),
        OperationKind::RemoveEntity { entity_id } => {
            ("remove_entity", entity_id.to_string(), None, None)
        }
        OperationKind::MoveEntity { entity_id, .. } => {
            ("move_entity", entity_id.to_string(), None, None)
        }
        OperationKind::SetOverride {
            entity_id,
            field,
            value,
            ..
        } => (
            "set_override",
            entity_id.to_string(),
            Some(field.clone()),
            Some(format!("{:?}", value)),
        ),
        OperationKind::ResolveConflict {
            entity_id,
            field,
            chosen_value,
            ..
        } => (
            "resolve_conflict",
            entity_id.to_string(),
            Some(field.clone()),
            Some(format!("{:?}", chosen_value)),
        ),
    };

    let json = serde_json::json!({
        "timestamp": op.timestamp.to_string(),
        "actor_id": op.actor.id,
        "actor_class": op.actor.class.to_string(),
        "operation_type": op_type,
        "entity_id": entity_id,
        "field": field,
        "value": value,
        "intent": op.intent,
        "operation_id": op.id.to_string(),
    });

    println!("{}", serde_json::to_string(&json).unwrap_or_default());
}

fn format_timestamp(ts: &conflux_core::HlcTimestamp) -> String {
    let s = ts.to_string();
    if s.len() > 20 {
        s[..20].to_string()
    } else {
        s
    }
}
