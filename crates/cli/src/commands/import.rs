//! Import existing configuration files.

use crate::actor::resolve_actor;
use crate::config::Config;
use anyhow::{Context, Result};
use clap::Args;
use conflux_core::{Clock, Document, EntityId, FieldValue, Operation};
use conflux_schema::Schema;
use conflux_store::SqliteStore;
use jrsonnet_evaluator::manifest::JsonFormat;
use jrsonnet_evaluator::trace::PathResolver;
use jrsonnet_evaluator::{FileImportResolver, StateBuilder};
use jrsonnet_stdlib::ContextInitializer;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Import existing configuration files into Conflux.
#[derive(Debug, Args)]
pub struct ImportArgs {
    /// Path to the file or directory to import.
    pub path: PathBuf,

    /// Entity type to import as.
    #[arg(long)]
    pub entity_type: String,

    /// Environment to import into.
    #[arg(long, short, default_value = "production")]
    pub environment: String,

    /// Actor identity for the import operations.
    #[arg(long)]
    pub actor: Option<String>,

    /// Actor class (human, pipeline, operator, system).
    #[arg(long, default_value = "pipeline")]
    pub actor_class: Option<String>,

    /// Intent message for the operations.
    #[arg(long, default_value = "import")]
    pub intent: String,

    /// Dry run - show what would be imported without making changes.
    #[arg(long)]
    pub dry_run: bool,

    /// Recursively import all config files in directory.
    #[arg(long, short)]
    pub recursive: bool,

    /// File extensions to include (comma-separated, e.g., "yaml,json,toml,kdl").
    /// Defaults to yaml,yml,json,toml,kdl,jsonnet,cue.
    #[arg(long, default_value = "yaml,yml,json,toml,kdl,jsonnet,cue")]
    pub extensions: String,

    /// Use filename (without extension) as entity ID prefix.
    #[arg(long)]
    pub use_filename_prefix: bool,

    /// Pattern to filter files (glob pattern, e.g., "*.yaml" or "routes/*.json").
    #[arg(long)]
    pub pattern: Option<String>,
}

/// Runs the import command.
pub fn run(args: ImportArgs, config: &Config, config_dir: &Path) -> Result<()> {
    let actor = resolve_actor(args.actor.as_deref(), args.actor_class.as_deref())?;

    // Load schema
    let schema_path = config_dir.join(&config.schema);
    let schema = Schema::from_file(&schema_path)
        .with_context(|| format!("loading schema from {}", schema_path.display()))?;

    // Verify entity type exists in schema
    if !schema.entities.contains_key(&args.entity_type) {
        anyhow::bail!(
            "entity type '{}' not found in schema. Available types: {}",
            args.entity_type,
            schema.entities.keys().cloned().collect::<Vec<_>>().join(", ")
        );
    }

    // Collect files to import
    let files = collect_files(&args)?;

    if files.is_empty() {
        println!("No files found to import.");
        return Ok(());
    }

    println!("Found {} file(s) to import", files.len());

    if args.dry_run {
        println!("\nDry run - would import:");
        for file in &files {
            let content = std::fs::read_to_string(file)
                .with_context(|| format!("reading {}", file.display()))?;
            let parsed = parse_config_file(file, &content)?;
            let prefix = get_entity_prefix(&args, file);

            for (entity_id, fields) in &parsed {
                let full_id = if let Some(ref p) = prefix {
                    format!("{}.{}", p, entity_id)
                } else {
                    entity_id.clone()
                };
                println!("  Entity: {full_id} (type: {})", args.entity_type);
                for (field, value) in fields {
                    println!("    {field}: {value:?}");
                }
            }
        }
        return Ok(());
    }

    // Open store and load document
    let db_path = config_dir.join(&config.database);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid database path"))?;
    let store = SqliteStore::open(db_path_str)?;

    let clock = Clock::new();
    let mut document = Document::new();

    // Load existing state from store
    let stored_ops = store.query_operations(
        &conflux_store::OperationQuery::new(&config.document_id).limit(100000),
    )?;
    let schema_info = schema.as_schema_info();
    for stored in stored_ops {
        let _ = document.apply(&stored.operation, &schema_info, &clock);
    }

    let mut total_operations = 0;
    let mut total_entities = 0;

    // Import each file
    for file in &files {
        let content = std::fs::read_to_string(file)
            .with_context(|| format!("reading {}", file.display()))?;
        let parsed = parse_config_file(file, &content)?;
        let prefix = get_entity_prefix(&args, file);

        let (ops, entities) = import_entities(
            &parsed,
            prefix.as_deref(),
            &args.entity_type,
            &args.intent,
            &actor,
            &clock,
            &mut document,
            &schema_info,
            &store,
            &config.document_id,
        )?;

        total_operations += ops;
        total_entities += entities;

        println!(
            "  Imported {} ({} entities, {} operations)",
            file.display(),
            entities,
            ops
        );
    }

    println!(
        "\nTotal: imported {} entities with {} operations from {} files",
        total_entities,
        total_operations,
        files.len()
    );

    Ok(())
}

/// Collects files to import based on arguments.
fn collect_files(args: &ImportArgs) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let extensions: Vec<&str> = args.extensions.split(',').map(|s| s.trim()).collect();

    if args.path.is_file() {
        files.push(args.path.clone());
    } else if args.path.is_dir() {
        collect_files_from_dir(&args.path, &extensions, args.recursive, &mut files)?;
    } else {
        anyhow::bail!("path does not exist: {}", args.path.display());
    }

    // Apply pattern filter if specified
    if let Some(ref pattern) = args.pattern {
        let glob_pattern = glob::Pattern::new(pattern)
            .with_context(|| format!("invalid glob pattern: {pattern}"))?;
        files.retain(|f| {
            f.file_name()
                .and_then(|n| n.to_str())
                .map(|n| glob_pattern.matches(n))
                .unwrap_or(false)
        });
    }

    // Sort for deterministic ordering
    files.sort();

    Ok(files)
}

/// Recursively collects files from a directory.
fn collect_files_from_dir(
    dir: &Path,
    extensions: &[&str],
    recursive: bool,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() && recursive {
            collect_files_from_dir(&path, extensions, recursive, files)?;
        } else if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
                    files.push(path);
                }
            }
        }
    }
    Ok(())
}

/// Gets the entity ID prefix based on filename if enabled.
fn get_entity_prefix(args: &ImportArgs, file: &Path) -> Option<String> {
    if args.use_filename_prefix {
        file.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    } else {
        None
    }
}

/// Imports entities from parsed config.
#[allow(clippy::too_many_arguments)]
fn import_entities(
    parsed: &HashMap<String, HashMap<String, FieldValue>>,
    prefix: Option<&str>,
    entity_type: &str,
    intent: &str,
    actor: &conflux_core::ActorId,
    clock: &Clock,
    document: &mut Document,
    schema_info: &dyn conflux_core::schema_info::SchemaInfo,
    store: &SqliteStore,
    document_id: &str,
) -> Result<(usize, usize)> {
    let mut operation_count = 0;
    let mut entity_count = 0;

    for (entity_id, fields) in parsed {
        let full_entity_id = if let Some(p) = prefix {
            format!("{}.{}", p, entity_id)
        } else {
            entity_id.clone()
        };

        let entity_id_obj = EntityId::new(&full_entity_id);

        // Insert entity if it doesn't exist
        if document.get_entity(&entity_id_obj).is_none() {
            let insert_op = Operation::insert_entity(
                full_entity_id.clone(),
                entity_type.to_string(),
                None,
                None,
                actor,
                clock.new_timestamp(),
            )
            .with_intent(intent);

            document.apply(&insert_op, schema_info, clock)?;
            store.append_operation(document_id, &insert_op)?;
            operation_count += 1;
            entity_count += 1;
        }

        // Set fields
        for (field_name, value) in fields {
            let set_op = Operation::set_field(
                full_entity_id.clone(),
                field_name,
                value.clone(),
                actor,
                clock.new_timestamp(),
            )
            .with_intent(intent);

            document.apply(&set_op, schema_info, clock)?;
            store.append_operation(document_id, &set_op)?;
            operation_count += 1;
        }
    }

    Ok((operation_count, entity_count))
}

/// Parses a configuration file and returns entity ID to fields mapping.
fn parse_config_file(
    path: &Path,
    content: &str,
) -> Result<HashMap<String, HashMap<String, FieldValue>>> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let value: serde_json::Value = match extension.as_str() {
        "json" => serde_json::from_str(content).context("parsing JSON")?,
        "yaml" | "yml" => serde_yaml::from_str(content).context("parsing YAML")?,
        "toml" => toml::from_str::<toml::Value>(content)
            .map(|v| serde_json::to_value(v).unwrap())
            .context("parsing TOML")?,
        "kdl" => kdl_to_json(content).context("parsing KDL")?,
        "jsonnet" | "libsonnet" => evaluate_jsonnet(path, content).context("evaluating Jsonnet")?,
        "cue" => evaluate_cue(path).context("evaluating CUE")?,
        _ => anyhow::bail!(
            "unsupported file format: {extension}. Supported: json, yaml, toml, kdl, jsonnet, cue"
        ),
    };

    // Convert JSON to entity map
    json_to_entities(&value)
}

/// Evaluates a Jsonnet file and returns the resulting JSON value.
///
/// Uses the jrsonnet pure-Rust implementation for evaluation.
fn evaluate_jsonnet(path: &Path, content: &str) -> Result<serde_json::Value> {
    // Get parent directory for imports
    let parent = path.parent().unwrap_or(Path::new("."));

    // Build the evaluation state with import resolver and stdlib
    let mut builder = StateBuilder::default();
    builder
        .import_resolver(FileImportResolver::new(vec![parent.to_path_buf()]))
        .context_initializer(ContextInitializer::new(PathResolver::Relative(
            parent.to_path_buf(),
        )));
    let state = builder.build();

    // Evaluate the snippet
    let name = path.to_string_lossy().into_owned();
    let val = state
        .evaluate_snippet(name, content)
        .map_err(|e| anyhow::anyhow!("Jsonnet evaluation error: {e}"))?;

    // Manifest to JSON string using JsonFormat
    let json_str = val
        .manifest(JsonFormat::default())
        .map_err(|e| anyhow::anyhow!("Jsonnet manifest error: {e}"))?;

    serde_json::from_str(&json_str).context("parsing Jsonnet output as JSON")
}

/// Evaluates a CUE file and returns the resulting JSON value.
///
/// Requires the `cue` CLI to be installed and available in PATH.
fn evaluate_cue(path: &Path) -> Result<serde_json::Value> {
    // Check if cue is available
    let check = std::process::Command::new("cue").arg("version").output();

    if check.is_err() || !check.as_ref().unwrap().status.success() {
        anyhow::bail!(
            "CUE CLI not found. Install from https://cuelang.org/docs/install/ to import .cue files"
        );
    }

    let output = std::process::Command::new("cue")
        .args(["export", "--out", "json"])
        .arg(path)
        .current_dir(path.parent().unwrap_or(Path::new(".")))
        .output()
        .context("running cue export")?;

    if !output.status.success() {
        anyhow::bail!(
            "CUE export failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    serde_json::from_slice(&output.stdout).context("parsing CUE output as JSON")
}

/// Converts a JSON value to entity ID -> fields mapping.
fn json_to_entities(
    value: &serde_json::Value,
) -> Result<HashMap<String, HashMap<String, FieldValue>>> {
    let mut result = HashMap::new();

    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                match val {
                    serde_json::Value::Object(fields) => {
                        let mut field_map = HashMap::new();
                        for (field_name, field_value) in fields {
                            let fv = json_to_field_value(field_value);
                            field_map.insert(field_name.clone(), fv);
                        }
                        result.insert(key.clone(), field_map);
                    }
                    _ => {
                        // Top-level primitive becomes a field on a "default" entity
                        let entry = result.entry("default".to_string()).or_default();
                        entry.insert(key.clone(), json_to_field_value(val));
                    }
                }
            }
        }
        _ => anyhow::bail!("expected object at root of config file"),
    }

    Ok(result)
}

/// Converts a JSON value to a FieldValue.
fn json_to_field_value(value: &serde_json::Value) -> FieldValue {
    match value {
        serde_json::Value::String(s) => FieldValue::String(s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                FieldValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                FieldValue::Float(f)
            } else {
                FieldValue::Null
            }
        }
        serde_json::Value::Bool(b) => FieldValue::Bool(*b),
        serde_json::Value::Array(arr) => {
            FieldValue::List(arr.iter().map(json_to_field_value).collect())
        }
        serde_json::Value::Object(map) => FieldValue::Map(
            map.iter()
                .map(|(k, v)| (k.clone(), json_to_field_value(v)))
                .collect(),
        ),
        serde_json::Value::Null => FieldValue::Null,
    }
}

/// Converts a KDL document to a JSON value.
///
/// KDL structure mapping:
/// - Top-level nodes become object keys (entity IDs)
/// - Node properties become field values
/// - Node children with values become nested fields
fn kdl_to_json(content: &str) -> Result<serde_json::Value> {
    let doc: kdl::KdlDocument = content.parse().map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut root = serde_json::Map::new();

    for node in doc.nodes() {
        let name = node.name().value().to_string();
        let value = kdl_node_to_json(node);
        root.insert(name, value);
    }

    Ok(serde_json::Value::Object(root))
}

/// Converts a KDL node to a JSON value.
fn kdl_node_to_json(node: &kdl::KdlNode) -> serde_json::Value {
    let mut obj = serde_json::Map::new();

    // Add properties as fields
    for entry in node.entries() {
        if let Some(name) = entry.name() {
            let value = kdl_value_to_json(entry.value());
            obj.insert(name.value().to_string(), value);
        }
    }

    // Add children as nested fields
    if let Some(children) = node.children() {
        for child in children.nodes() {
            let child_name = child.name().value().to_string();

            // If child has no properties/children but has arguments, use first argument as value
            if child.entries().iter().all(|e| e.name().is_none())
                && child.children().is_none()
                && !child.entries().is_empty()
            {
                let value = kdl_value_to_json(child.entries().first().unwrap().value());
                obj.insert(child_name, value);
            } else {
                obj.insert(child_name, kdl_node_to_json(child));
            }
        }
    }

    serde_json::Value::Object(obj)
}

/// Converts a KDL value to a JSON value.
fn kdl_value_to_json(value: &kdl::KdlValue) -> serde_json::Value {
    match value {
        kdl::KdlValue::String(s) => serde_json::Value::String(s.clone()),
        kdl::KdlValue::Integer(i) => serde_json::Value::Number((*i as i64).into()),
        kdl::KdlValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        kdl::KdlValue::Bool(b) => serde_json::Value::Bool(*b),
        kdl::KdlValue::Null => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_config() {
        let content = r#"{"service1": {"replicas": 3, "image": "nginx"}}"#;
        let path = PathBuf::from("test.json");
        let result = parse_config_file(&path, content).unwrap();

        assert!(result.contains_key("service1"));
        let fields = result.get("service1").unwrap();
        assert_eq!(fields.get("replicas"), Some(&FieldValue::Int(3)));
        assert_eq!(
            fields.get("image"),
            Some(&FieldValue::String("nginx".to_string()))
        );
    }

    #[test]
    fn parse_yaml_config() {
        let content = "service1:\n  replicas: 3\n  image: nginx";
        let path = PathBuf::from("test.yaml");
        let result = parse_config_file(&path, content).unwrap();

        assert!(result.contains_key("service1"));
    }

    #[test]
    fn parse_kdl_config() {
        let content = r#"
service1 {
    replicas 3
    image "nginx"
}
"#;
        let path = PathBuf::from("test.kdl");
        let result = parse_config_file(&path, content).unwrap();

        assert!(result.contains_key("service1"));
        let fields = result.get("service1").unwrap();
        assert_eq!(fields.get("replicas"), Some(&FieldValue::Int(3)));
        assert_eq!(
            fields.get("image"),
            Some(&FieldValue::String("nginx".to_string()))
        );
    }

    #[test]
    fn parse_kdl_config_with_properties() {
        let content = r#"service1 replicas=3 image="nginx""#;
        let path = PathBuf::from("test.kdl");
        let result = parse_config_file(&path, content).unwrap();

        assert!(result.contains_key("service1"));
        let fields = result.get("service1").unwrap();
        assert_eq!(fields.get("replicas"), Some(&FieldValue::Int(3)));
        assert_eq!(
            fields.get("image"),
            Some(&FieldValue::String("nginx".to_string()))
        );
    }

    #[test]
    fn parse_jsonnet_basic() {
        let content = r#"{ name: "test", count: 1 + 2 }"#;
        let path = PathBuf::from("test.jsonnet");
        let result = parse_config_file(&path, content).unwrap();

        // Single object without nested objects becomes a "default" entity
        assert!(result.contains_key("default"));
        let fields = result.get("default").unwrap();
        assert_eq!(
            fields.get("name"),
            Some(&FieldValue::String("test".to_string()))
        );
        assert_eq!(fields.get("count"), Some(&FieldValue::Int(3)));
    }

    #[test]
    fn parse_jsonnet_with_object_output() {
        let content = r#"{ service1: { replicas: 3, image: "nginx" } }"#;
        let path = PathBuf::from("test.jsonnet");
        let result = parse_config_file(&path, content).unwrap();

        assert!(result.contains_key("service1"));
        let fields = result.get("service1").unwrap();
        assert_eq!(fields.get("replicas"), Some(&FieldValue::Int(3)));
        assert_eq!(
            fields.get("image"),
            Some(&FieldValue::String("nginx".to_string()))
        );
    }

    #[test]
    fn parse_jsonnet_with_computation() {
        let content = r#"
        local base = { replicas: 2 };
        {
            service1: base + { image: "nginx" },
            service2: base + { image: "redis", replicas: 5 },
        }
        "#;
        let path = PathBuf::from("test.jsonnet");
        let result = parse_config_file(&path, content).unwrap();

        assert!(result.contains_key("service1"));
        assert!(result.contains_key("service2"));

        let s1 = result.get("service1").unwrap();
        assert_eq!(s1.get("replicas"), Some(&FieldValue::Int(2)));
        assert_eq!(
            s1.get("image"),
            Some(&FieldValue::String("nginx".to_string()))
        );

        let s2 = result.get("service2").unwrap();
        assert_eq!(s2.get("replicas"), Some(&FieldValue::Int(5)));
        assert_eq!(
            s2.get("image"),
            Some(&FieldValue::String("redis".to_string()))
        );
    }

    // CUE test requires the cue CLI to be installed, so we mark it as ignored
    // Run with: cargo test --package conflux -- --ignored
    #[test]
    #[ignore]
    fn parse_cue_basic() {
        use std::io::Write;

        // Create a temporary CUE file
        let dir = tempfile::tempdir().unwrap();
        let cue_path = dir.path().join("test.cue");
        let mut file = std::fs::File::create(&cue_path).unwrap();
        writeln!(file, "service1: replicas: 3").unwrap();
        writeln!(file, "service1: image: \"nginx\"").unwrap();
        drop(file);

        // CUE files are evaluated from the file directly, not from content
        let result = evaluate_cue(&cue_path).unwrap();
        let entities = json_to_entities(&result).unwrap();

        assert!(entities.contains_key("service1"));
        let fields = entities.get("service1").unwrap();
        assert_eq!(fields.get("replicas"), Some(&FieldValue::Int(3)));
        assert_eq!(
            fields.get("image"),
            Some(&FieldValue::String("nginx".to_string()))
        );
    }
}
