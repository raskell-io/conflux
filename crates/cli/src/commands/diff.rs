//! Show differences between states.

use crate::config::Config;
use anyhow::{Context, Result};
use clap::Args;
use conflux_core::schema_info::SchemaInfo;
use conflux_core::{Clock, Document};
use conflux_schema::Schema;
use conflux_store::SqliteStore;
use std::path::Path;

/// Show differences between document states.
#[derive(Debug, Args)]
pub struct DiffArgs {
    /// Compare with a specific milestone (by ID or "latest").
    #[arg(long)]
    pub milestone: Option<String>,

    /// Specific entity to diff (omit for all entities).
    #[arg(long, short)]
    pub entity: Option<String>,

    /// Show only summary counts.
    #[arg(long)]
    pub summary: bool,

    /// Compare two environments (source environment).
    #[arg(long, requires = "to_env")]
    pub from_env: Option<String>,

    /// Compare two environments (target environment).
    #[arg(long, requires = "from_env")]
    pub to_env: Option<String>,

    /// Show diff for a specific environment (resolved values).
    #[arg(long, conflicts_with_all = ["from_env", "to_env"])]
    pub env: Option<String>,
}

/// Runs the diff command.
pub fn run(args: DiffArgs, config: &Config, config_dir: &Path) -> Result<()> {
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

    // Check if this is an environment comparison
    if args.from_env.is_some() && args.to_env.is_some() {
        return run_env_diff(&args, config, &schema, &store);
    }

    let clock = Clock::new();
    let schema_info = schema.as_schema_info();

    // Determine the base state (from milestone or empty)
    let base_doc = if let Some(ref milestone_ref) = args.milestone {
        let milestones = store.list_milestones(&config.document_id)?;

        let milestone = if milestone_ref == "latest" {
            milestones.first()
        } else {
            milestones
                .iter()
                .find(|m| m.id.to_string().starts_with(milestone_ref))
        }
        .ok_or_else(|| anyhow::anyhow!("milestone '{}' not found", milestone_ref))?;

        // Rebuild document up to milestone
        let mut doc = Document::new();
        let stored_ops = store.query_operations(
            &conflux_store::OperationQuery::new(&config.document_id)
                .until(&milestone.hlc_range_end)
                .limit(10000),
        )?;
        for stored in stored_ops {
            let _ = doc.apply(&stored.operation, &schema_info, &clock);
        }
        doc
    } else {
        // Compare against the latest milestone or empty
        if let Some(milestone) = store.latest_milestone(&config.document_id)? {
            let mut doc = Document::new();
            let stored_ops = store.query_operations(
                &conflux_store::OperationQuery::new(&config.document_id)
                    .until(&milestone.hlc_range_end)
                    .limit(10000),
            )?;
            for stored in stored_ops {
                let _ = doc.apply(&stored.operation, &schema_info, &clock);
            }
            doc
        } else {
            Document::new()
        }
    };

    // Build current document
    let mut current_doc = Document::new();
    let stored_ops = store.query_operations(
        &conflux_store::OperationQuery::new(&config.document_id).limit(10000),
    )?;
    for stored in stored_ops {
        let _ = current_doc.apply(&stored.operation, &schema_info, &clock);
    }

    // If --env is specified, show environment-resolved values
    if let Some(ref env) = args.env {
        return run_env_resolved_diff(&args, &base_doc, &current_doc, &schema_info, env);
    }

    // Calculate diff (standard milestone comparison)
    run_milestone_diff(&args, &base_doc, &current_doc)
}

/// Compares two environments and shows the differences.
fn run_env_diff(
    args: &DiffArgs,
    config: &Config,
    schema: &Schema,
    store: &SqliteStore,
) -> Result<()> {
    let from_env = args.from_env.as_ref().unwrap();
    let to_env = args.to_env.as_ref().unwrap();

    let schema_info = schema.as_schema_info();

    // Validate environments exist
    if !schema_info.has_environment(from_env) {
        anyhow::bail!("environment '{}' not found in schema", from_env);
    }
    if !schema_info.has_environment(to_env) {
        anyhow::bail!("environment '{}' not found in schema", to_env);
    }

    // Load current document
    let clock = Clock::new();
    let mut document = Document::new();
    let stored_ops = store.query_operations(
        &conflux_store::OperationQuery::new(&config.document_id).limit(10000),
    )?;
    for stored in stored_ops {
        let _ = document.apply(&stored.operation, &schema_info, &clock);
    }

    println!("Comparing environments: {} -> {}", from_env, to_env);
    println!();

    let mut diff_count = 0;
    let mut changes: Vec<String> = Vec::new();

    for (entity_id, entity) in &document.entities {
        // Apply entity filter
        if let Some(ref filter) = args.entity {
            if !entity_id.to_string().contains(filter) {
                continue;
            }
        }

        // Skip tombstoned entities
        if entity.is_tombstoned() {
            continue;
        }

        // Get field names for this entity type
        let field_names = match schema_info.entity_fields(&entity.entity_type) {
            Some(names) => names,
            None => continue,
        };

        for field_name in field_names {
            let from_value = document.get_field_for_env(entity_id, field_name, from_env, &schema_info);
            let to_value = document.get_field_for_env(entity_id, field_name, to_env, &schema_info);

            if from_value != to_value {
                diff_count += 1;
                if !args.summary {
                    let from_display = from_value
                        .as_ref()
                        .map(|v| format!("{:?}", v))
                        .unwrap_or_else(|| "(none)".to_string());
                    let to_display = to_value
                        .as_ref()
                        .map(|v| format!("{:?}", v))
                        .unwrap_or_else(|| "(none)".to_string());
                    changes.push(format!(
                        "  {}.{}: {} -> {}",
                        entity_id, field_name, from_display, to_display
                    ));
                }
            }
        }
    }

    if diff_count == 0 {
        println!("No differences between '{}' and '{}'.", from_env, to_env);
    } else {
        if !args.summary {
            for change in &changes {
                println!("{change}");
            }
            println!();
        }
        println!("{} field(s) differ between '{}' and '{}'.", diff_count, from_env, to_env);
    }

    Ok(())
}

/// Shows diff with environment-resolved values.
fn run_env_resolved_diff(
    args: &DiffArgs,
    base_doc: &Document,
    current_doc: &Document,
    schema_info: &dyn SchemaInfo,
    env: &str,
) -> Result<()> {
    // Validate environment exists
    if !schema_info.has_environment(env) {
        anyhow::bail!("environment '{}' not found in schema", env);
    }

    println!("Diff for environment: {}", env);
    println!();

    let mut added = 0;
    let mut modified = 0;
    let mut removed = 0;
    let mut changes: Vec<String> = Vec::new();

    // Check for added/modified entities
    for (entity_id, entity) in &current_doc.entities {
        if let Some(ref filter) = args.entity {
            if !entity_id.to_string().contains(filter) {
                continue;
            }
        }

        match base_doc.get_entity(entity_id) {
            None => {
                added += 1;
                if !args.summary {
                    changes.push(format!("+ {} (type: {})", entity_id, entity.entity_type));
                }
            }
            Some(_base_entity) => {
                // Compare environment-resolved field values
                let field_names = match schema_info.entity_fields(&entity.entity_type) {
                    Some(names) => names,
                    None => continue,
                };

                for field_name in field_names {
                    let current_value = current_doc.get_field_for_env(entity_id, field_name, env, schema_info);
                    let base_value = base_doc.get_field_for_env(entity_id, field_name, env, schema_info);

                    if current_value != base_value {
                        modified += 1;
                        if !args.summary {
                            let base_display = base_value
                                .as_ref()
                                .map(|v| format!("{:?}", v))
                                .unwrap_or_else(|| "(none)".to_string());
                            let current_display = current_value
                                .as_ref()
                                .map(|v| format!("{:?}", v))
                                .unwrap_or_else(|| "(none)".to_string());
                            changes.push(format!(
                                "~ {}.{}: {} -> {}",
                                entity_id, field_name, base_display, current_display
                            ));
                        }
                    }
                }

                // Check tombstone changes
                if entity.tombstone.value != _base_entity.tombstone.value {
                    modified += 1;
                    if !args.summary {
                        if entity.tombstone.value {
                            changes.push(format!("x {} (deleted)", entity_id));
                        } else {
                            changes.push(format!("o {} (restored)", entity_id));
                        }
                    }
                }
            }
        }
    }

    // Check for removed entities
    for entity_id in base_doc.entities.keys() {
        if let Some(ref filter) = args.entity {
            if !entity_id.to_string().contains(filter) {
                continue;
            }
        }

        if current_doc.get_entity(entity_id).is_none() {
            removed += 1;
            if !args.summary {
                changes.push(format!("- {} (removed)", entity_id));
            }
        }
    }

    // Print results
    if added == 0 && modified == 0 && removed == 0 {
        println!("No changes.");
    } else {
        if !args.summary {
            for change in &changes {
                println!("{change}");
            }
            println!();
        }
        println!(
            "Summary: {} added, {} modified, {} removed",
            added, modified, removed
        );
    }

    Ok(())
}

/// Standard milestone-based diff (original behavior).
fn run_milestone_diff(
    args: &DiffArgs,
    base_doc: &Document,
    current_doc: &Document,
) -> Result<()> {
    let mut added = 0;
    let mut modified = 0;
    let mut removed = 0;
    let mut changes: Vec<String> = Vec::new();

    // Check for added/modified entities
    for (entity_id, entity) in &current_doc.entities {
        if let Some(ref filter) = args.entity {
            if !entity_id.to_string().contains(filter) {
                continue;
            }
        }

        match base_doc.get_entity(entity_id) {
            None => {
                added += 1;
                if !args.summary {
                    changes.push(format!("+ {} (type: {})", entity_id, entity.entity_type));
                }
            }
            Some(base_entity) => {
                // Check for field changes
                for (field_name, current_state) in &entity.fields {
                    let current_value = current_state.value();
                    let base_value = base_entity
                        .fields
                        .get(field_name)
                        .map(|s| s.value())
                        .unwrap_or(conflux_core::FieldValue::Null);

                    if current_value != base_value {
                        modified += 1;
                        if !args.summary {
                            changes.push(format!(
                                "~ {}.{}: {:?} -> {:?}",
                                entity_id, field_name, base_value, current_value
                            ));
                        }
                    }
                }

                // Check for removed fields
                for field_name in base_entity.fields.keys() {
                    if !entity.fields.contains_key(field_name) {
                        modified += 1;
                        if !args.summary {
                            let base_value = base_entity.fields.get(field_name).unwrap().value();
                            changes.push(format!(
                                "- {}.{}: {:?}",
                                entity_id, field_name, base_value
                            ));
                        }
                    }
                }

                // Check tombstone changes
                if entity.tombstone.value != base_entity.tombstone.value {
                    modified += 1;
                    if !args.summary {
                        if entity.tombstone.value {
                            changes.push(format!("x {} (deleted)", entity_id));
                        } else {
                            changes.push(format!("o {} (restored)", entity_id));
                        }
                    }
                }
            }
        }
    }

    // Check for removed entities
    for entity_id in base_doc.entities.keys() {
        if let Some(ref filter) = args.entity {
            if !entity_id.to_string().contains(filter) {
                continue;
            }
        }

        if current_doc.get_entity(entity_id).is_none() {
            removed += 1;
            if !args.summary {
                changes.push(format!("- {} (removed)", entity_id));
            }
        }
    }

    // Print results
    if added == 0 && modified == 0 && removed == 0 {
        println!("No changes.");
    } else {
        if !args.summary {
            for change in &changes {
                println!("{change}");
            }
            println!();
        }
        println!(
            "Summary: {} added, {} modified, {} removed",
            added, modified, removed
        );
    }

    Ok(())
}
