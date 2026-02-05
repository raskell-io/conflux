//! Get entity or field values.

use crate::config::Config;
use crate::output::{OutputFormat, Table};
use anyhow::{Context, Result};
use clap::Args;
use conflux_core::{Clock, Document, EntityId};
use conflux_schema::Schema;
use conflux_store::SqliteStore;
use std::path::Path;

/// Get entity or field values.
#[derive(Debug, Args)]
pub struct GetArgs {
    /// Entity ID to get (e.g., "service.api"). Omit to list all entities.
    pub entity_id: Option<String>,

    /// Specific field to get. If omitted, shows all fields.
    #[arg(long, short)]
    pub field: Option<String>,

    /// Output format (plain, json, yaml).
    #[arg(long, short, default_value = "plain")]
    pub output: OutputFormat,

    /// Show all entities in the document.
    #[arg(long)]
    pub all: bool,
}

/// Runs the get command.
pub fn run(args: GetArgs, config: &Config, config_dir: &Path) -> Result<()> {
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

    if args.all || args.entity_id.is_none() {
        // List all entities
        return list_all_entities(&document, args.output);
    }

    let entity_id_str = args.entity_id.unwrap();
    let entity_id = EntityId::new(&entity_id_str);

    let entity = document
        .get_entity(&entity_id)
        .ok_or_else(|| anyhow::anyhow!("entity '{}' not found", entity_id_str))?;

    if let Some(field_name) = args.field {
        // Get specific field
        let field_state = entity.fields.get(&field_name).ok_or_else(|| {
            anyhow::anyhow!(
                "field '{}' not found on entity '{}'",
                field_name,
                entity_id_str
            )
        })?;

        let value = field_state.value();
        match args.output {
            OutputFormat::Plain => println!("{value:?}"),
            OutputFormat::Json => {
                let json = field_value_to_json(&value);
                println!("{}", serde_json::to_string_pretty(&json)?);
            }
            OutputFormat::Yaml => {
                let json = field_value_to_json(&value);
                print!("{}", serde_yaml::to_string(&json)?);
            }
        }
    } else {
        // Get all fields
        match args.output {
            OutputFormat::Plain => {
                println!("Entity: {} (type: {})", entity_id_str, entity.entity_type);
                if entity.tombstone.value {
                    println!("  [DELETED]");
                }
                if let Some(parent) = &entity.parent_id {
                    println!("  Parent: {}", parent);
                }
                if !entity.children.is_empty() {
                    println!(
                        "  Children: {}",
                        entity
                            .children
                            .values()
                            .map(|c| c.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                println!("  Fields:");
                for (name, state) in &entity.fields {
                    println!("    {}: {:?}", name, state.value());
                }
            }
            OutputFormat::Json | OutputFormat::Yaml => {
                let mut obj = serde_json::Map::new();
                obj.insert(
                    "entity_id".to_string(),
                    serde_json::Value::String(entity_id_str),
                );
                obj.insert(
                    "entity_type".to_string(),
                    serde_json::Value::String(entity.entity_type.clone()),
                );
                obj.insert(
                    "tombstoned".to_string(),
                    serde_json::Value::Bool(entity.tombstone.value),
                );

                let mut fields = serde_json::Map::new();
                for (name, state) in &entity.fields {
                    fields.insert(name.clone(), field_value_to_json(&state.value()));
                }
                obj.insert("fields".to_string(), serde_json::Value::Object(fields));

                let value = serde_json::Value::Object(obj);
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
    }

    Ok(())
}

fn list_all_entities(document: &Document, format: OutputFormat) -> Result<()> {
    let entities: Vec<_> = document.entities.iter().collect();

    if entities.is_empty() {
        println!("No entities in document.");
        return Ok(());
    }

    match format {
        OutputFormat::Plain => {
            let mut table = Table::new(vec!["Entity ID", "Type", "Status", "Fields"]);
            for (entity_id, entity) in &entities {
                let status = if entity.tombstone.value {
                    "deleted"
                } else {
                    "active"
                };
                let field_count = entity.fields.len().to_string();
                table.add_row(vec![
                    entity_id.to_string(),
                    entity.entity_type.clone(),
                    status.to_string(),
                    field_count,
                ]);
            }
            table.print();
        }
        OutputFormat::Json | OutputFormat::Yaml => {
            let list: Vec<serde_json::Value> = entities
                .iter()
                .map(|(id, entity)| {
                    let mut obj = serde_json::Map::new();
                    obj.insert(
                        "entity_id".to_string(),
                        serde_json::Value::String(id.to_string()),
                    );
                    obj.insert(
                        "entity_type".to_string(),
                        serde_json::Value::String(entity.entity_type.clone()),
                    );
                    obj.insert(
                        "tombstoned".to_string(),
                        serde_json::Value::Bool(entity.tombstone.value),
                    );

                    let mut fields = serde_json::Map::new();
                    for (name, state) in &entity.fields {
                        fields.insert(name.clone(), field_value_to_json(&state.value()));
                    }
                    obj.insert("fields".to_string(), serde_json::Value::Object(fields));
                    serde_json::Value::Object(obj)
                })
                .collect();

            let value = serde_json::Value::Array(list);
            match format {
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

fn field_value_to_json(value: &conflux_core::FieldValue) -> serde_json::Value {
    use conflux_core::FieldValue;
    match value {
        FieldValue::String(s) => serde_json::Value::String(s.clone()),
        FieldValue::Int(i) => serde_json::json!(i),
        FieldValue::Float(f) => serde_json::json!(f),
        FieldValue::Bool(b) => serde_json::Value::Bool(*b),
        FieldValue::Duration(d) => serde_json::Value::String(d.to_string()),
        FieldValue::Ref(r) => serde_json::Value::String(r.to_string()),
        FieldValue::List(items) => {
            serde_json::Value::Array(items.iter().map(field_value_to_json).collect())
        }
        FieldValue::Map(map) => {
            let obj: serde_json::Map<_, _> = map
                .iter()
                .map(|(k, v)| (k.clone(), field_value_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
        FieldValue::Null => serde_json::Value::Null,
    }
}
