//! Promote configuration between environments.
//!
//! Copies field values from a source environment to a target environment
//! by creating SetOverride operations.

use crate::actor::resolve_actor;
use crate::config::Config;
use anyhow::{Context, Result};
use clap::Args;
use conflux_core::schema_info::SchemaInfo;
use conflux_core::{Clock, Document, EntityId, Operation};
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

    /// Dry run - show what would be promoted without making changes.
    #[arg(long)]
    pub dry_run: bool,

    /// Actor identity for the operation.
    #[arg(long)]
    pub actor: Option<String>,

    /// Actor class (human, pipeline, operator, system).
    #[arg(long)]
    pub actor_class: Option<String>,

    /// Intent message explaining why this promotion is being made.
    #[arg(long, short)]
    pub intent: Option<String>,
}

/// A field that can be promoted.
#[derive(Debug)]
struct PromotableField {
    entity_id: EntityId,
    field: String,
    source_value: conflux_core::FieldValue,
    target_value: Option<conflux_core::FieldValue>,
}

/// Runs the promote command.
pub fn run(args: PromoteArgs, config: &Config, config_dir: &Path) -> Result<()> {
    // Load schema
    let schema_path = config_dir.join(&config.schema);
    let schema = Schema::from_file(&schema_path)
        .with_context(|| format!("loading schema from {}", schema_path.display()))?;

    // Validate environments exist in schema
    let envs = schema.environments.as_ref().ok_or_else(|| {
        anyhow::anyhow!("no environments defined in schema")
    })?;

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

    // Validate inheritance direction (can only promote up the chain)
    if !can_promote(&args.from, &args.to, &schema) {
        anyhow::bail!(
            "cannot promote from '{}' to '{}': can only promote to ancestor environments",
            args.from,
            args.to
        );
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

    // Find promotable fields
    let promotable = find_promotable_fields(
        &document,
        &schema_info,
        &schema,
        &args.from,
        &args.to,
        args.entity.as_deref(),
        args.field.as_deref(),
    )?;

    if promotable.is_empty() {
        println!("No differences found between '{}' and '{}'.", args.from, args.to);
        return Ok(());
    }

    // Display what will be promoted
    println!(
        "Found {} field(s) to promote from '{}' to '{}':",
        promotable.len(),
        args.from,
        args.to
    );
    println!();

    for field in &promotable {
        let target_display = field
            .target_value
            .as_ref()
            .map(|v| format!("{:?}", v))
            .unwrap_or_else(|| "(inherited)".to_string());
        println!(
            "  {}.{}: {:?} -> {:?}",
            field.entity_id, field.field, target_display, field.source_value
        );
    }
    println!();

    if args.dry_run {
        println!("Dry run - no changes made.");
        return Ok(());
    }

    // Resolve actor
    let actor = resolve_actor(args.actor.as_deref(), args.actor_class.as_deref())?;

    // Build intent
    let intent = args.intent.unwrap_or_else(|| {
        format!("promote {} -> {}", args.from, args.to)
    });

    // Create and apply SetOverride operations
    let mut applied_count = 0;
    for field in &promotable {
        let op = Operation::set_override(
            field.entity_id.clone(),
            field.field.clone(),
            args.to.clone(),
            field.source_value.clone(),
            &actor,
            clock.new_timestamp(),
        )
        .with_intent(&intent);

        match document.apply(&op, &schema_info, &clock) {
            Ok(conflux_core::ApplyResult::Applied) => {
                store.append_operation(&config.document_id, &op)?;
                applied_count += 1;
            }
            Ok(conflux_core::ApplyResult::Conflict(info)) => {
                println!(
                    "  Warning: conflict on {}.{}: {:?}",
                    field.entity_id, field.field, info.contending_values
                );
                store.append_operation(&config.document_id, &op)?;
                applied_count += 1;
            }
            Err(e) => {
                println!(
                    "  Error promoting {}.{}: {}",
                    field.entity_id, field.field, e
                );
            }
        }
    }

    println!(
        "Promoted {} field(s) from '{}' to '{}'.",
        applied_count, args.from, args.to
    );

    Ok(())
}

/// Checks if promotion from `from` to `to` is valid.
/// Can only promote to ancestor environments (up the inheritance chain).
fn can_promote(from: &str, to: &str, schema: &Schema) -> bool {
    let schema_info = schema.as_schema_info();

    // Walk up from `from` and see if we hit `to`
    let mut current = Some(from);
    while let Some(env) = current {
        if env == to {
            return true;
        }
        current = schema_info.environment_parent(env);
    }

    // Also allow promoting from any env to the base
    if let Some(ref envs) = schema.environments {
        if to == envs.base {
            return true;
        }
    }

    false
}

/// Finds all fields that differ between source and target environments.
fn find_promotable_fields(
    document: &Document,
    schema_info: &dyn SchemaInfo,
    schema: &Schema,
    from: &str,
    to: &str,
    entity_filter: Option<&str>,
    field_filter: Option<&str>,
) -> Result<Vec<PromotableField>> {
    let mut promotable = Vec::new();

    for (entity_id, entity) in &document.entities {
        // Apply entity filter
        if let Some(filter) = entity_filter {
            if !entity_id.to_string().contains(filter) {
                continue;
            }
        }

        // Skip tombstoned entities
        if entity.is_tombstoned() {
            continue;
        }

        // Get field definitions for this entity type
        let entity_def = schema.entities.get(&entity.entity_type);
        if entity_def.is_none() {
            continue;
        }

        let field_names = schema_info.entity_fields(&entity.entity_type);
        if field_names.is_none() {
            continue;
        }

        for field_name in field_names.unwrap() {
            // Apply field filter
            if let Some(filter) = field_filter {
                if field_name != filter {
                    continue;
                }
            }

            // Get values in both environments
            let source_value = document.get_field_for_env(entity_id, field_name, from, schema_info);
            let target_value = document.get_field_for_env(entity_id, field_name, to, schema_info);

            // Check if source has a value that differs from target
            if let Some(src) = source_value {
                if target_value.as_ref() != Some(&src) {
                    promotable.push(PromotableField {
                        entity_id: entity_id.clone(),
                        field: field_name.to_string(),
                        source_value: src,
                        target_value,
                    });
                }
            }
        }
    }

    Ok(promotable)
}
