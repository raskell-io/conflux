//! Promote configuration between environments.
//!
//! Note: Environment overrides are an advanced feature that requires core support
//! for per-environment field values. This command provides a placeholder implementation
//! that shows the intended UX but will need core support to be fully functional.

use crate::config::Config;
use anyhow::{Context, Result};
use clap::Args;
use conflux_core::{Clock, Document};
use conflux_schema::Schema;
use conflux_store::SqliteStore;
use std::path::Path;

/// Promote configuration from one environment to another.
#[derive(Debug, Args)]
pub struct PromoteArgs {
    /// Source environment.
    pub from: String,

    /// Target environment.
    pub to: String,

    /// Specific entity to promote (omit for all).
    #[arg(long, short)]
    pub entity: Option<String>,

    /// Specific field to promote (requires --entity).
    #[arg(long, short)]
    pub field: Option<String>,

    /// Dry run - show what would be promoted.
    #[arg(long)]
    pub dry_run: bool,
}

/// Runs the promote command.
pub fn run(args: PromoteArgs, config: &Config, config_dir: &Path) -> Result<()> {
    // Load schema
    let schema_path = config_dir.join(&config.schema);
    let schema = Schema::from_file(&schema_path)
        .with_context(|| format!("loading schema from {}", schema_path.display()))?;

    // Validate environments exist in schema
    if let Some(ref envs) = schema.environments {
        let all_envs: Vec<&str> = std::iter::once(envs.base.as_str())
            .chain(envs.overlays.iter().map(|o| o.name.as_str()))
            .collect();

        if !all_envs.contains(&args.from.as_str()) {
            anyhow::bail!(
                "source environment '{}' not found in schema. Available: {}",
                args.from,
                all_envs.join(", ")
            );
        }
        if !all_envs.contains(&args.to.as_str()) {
            anyhow::bail!(
                "target environment '{}' not found in schema. Available: {}",
                args.to,
                all_envs.join(", ")
            );
        }
    } else {
        anyhow::bail!("no environments defined in schema");
    }

    // Open store
    let db_path = config_dir.join(&config.database);
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

    // Count eligible entities/fields
    let mut eligible_count = 0;
    for (entity_id, entity) in &document.entities {
        if let Some(ref entity_filter) = args.entity {
            if !entity_id.to_string().contains(entity_filter) {
                continue;
            }
        }

        for field_name in entity.fields.keys() {
            if let Some(ref field_filter) = args.field {
                if field_name != field_filter {
                    continue;
                }
            }
            eligible_count += 1;
        }
    }

    if eligible_count == 0 {
        println!("No fields found matching the criteria.");
        return Ok(());
    }

    if args.dry_run {
        println!("Dry run - would promote {} fields:", eligible_count);
        println!("  From: {}", args.from);
        println!("  To: {}", args.to);
        if let Some(ref e) = args.entity {
            println!("  Entity filter: {}", e);
        }
        if let Some(ref f) = args.field {
            println!("  Field filter: {}", f);
        }
        println!();
        println!(
            "Note: Environment overrides require core support for SetOverride operations."
        );
        println!("This feature is planned but not yet fully implemented in conflux-core.");
        return Ok(());
    }

    // Environment promotion requires SetOverride operation support in core
    println!(
        "Promote from '{}' to '{}' is planned but requires SetOverride support in core.",
        args.from, args.to
    );
    println!("Eligible entities/fields: {}", eligible_count);
    println!();
    println!("Use `conflux set` with environment-specific values as a workaround.");

    Ok(())
}
