//! Initialize a new Conflux project.

use anyhow::{Context, Result};
use clap::Args;
use std::fs;
use std::path::PathBuf;

/// Initialize a new Conflux project.
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Path to create the project in (defaults to current directory).
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Path to an existing schema file to use.
    #[arg(long)]
    pub schema: Option<PathBuf>,

    /// Project name (defaults to directory name).
    #[arg(long)]
    pub name: Option<String>,

    /// Force initialization even if files exist.
    #[arg(long, short)]
    pub force: bool,
}

/// Runs the init command.
pub fn run(args: InitArgs) -> Result<()> {
    let project_dir = args.path.canonicalize().unwrap_or(args.path.clone());

    // Create project directory if it doesn't exist
    if !project_dir.exists() {
        fs::create_dir_all(&project_dir)
            .with_context(|| format!("creating directory {}", project_dir.display()))?;
    }

    let config_path = project_dir.join("conflux.toml");
    let schema_path = project_dir.join("schema.toml");
    let data_dir = project_dir.join(".conflux");

    // Check for existing files
    if !args.force && config_path.exists() {
        anyhow::bail!(
            "conflux.toml already exists at {}. Use --force to overwrite.",
            config_path.display()
        );
    }

    // Create .conflux directory
    if !data_dir.exists() {
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("creating {}", data_dir.display()))?;
    }

    // Copy or create schema
    let schema_rel_path = if let Some(source_schema) = &args.schema {
        // Copy existing schema
        let source_content = fs::read_to_string(source_schema)
            .with_context(|| format!("reading schema from {}", source_schema.display()))?;
        fs::write(&schema_path, &source_content)
            .with_context(|| format!("writing schema to {}", schema_path.display()))?;
        println!("Copied schema from {}", source_schema.display());
        "schema.toml".to_string()
    } else {
        // Create example schema
        let example_schema = generate_example_schema();
        fs::write(&schema_path, &example_schema)
            .with_context(|| format!("writing schema to {}", schema_path.display()))?;
        println!("Created example schema at {}", schema_path.display());
        "schema.toml".to_string()
    };

    // Determine project name
    let project_name = args.name.unwrap_or_else(|| {
        project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("conflux-project")
            .to_string()
    });

    // Create configuration file
    let config_content = generate_config(&schema_rel_path, &project_name);
    fs::write(&config_path, &config_content)
        .with_context(|| format!("writing config to {}", config_path.display()))?;
    println!("Created configuration at {}", config_path.display());

    // Create .gitignore for .conflux directory
    let gitignore_path = data_dir.join(".gitignore");
    fs::write(&gitignore_path, "*.db\n*.db-journal\n")
        .with_context(|| format!("writing {}", gitignore_path.display()))?;

    println!();
    println!("Initialized Conflux project: {project_name}");
    println!();
    println!("Next steps:");
    println!("  1. Edit schema.toml to define your entity types and fields");
    println!("  2. Run `conflux daemon` to start the server");
    println!("  3. Use `conflux set` to add data or `conflux import` to import existing configs");

    Ok(())
}

fn generate_config(schema_path: &str, document_id: &str) -> String {
    format!(
        r#"# Conflux configuration
# See https://github.com/raskell-io/conflux for documentation

# Path to the schema definition
schema = "{schema_path}"

# Unique identifier for this document
document_id = "{document_id}"

# SQLite database for the operation log
database = ".conflux/state.db"

# HTTP server configuration
[server]
host = "0.0.0.0"
port = 9400

# Git milestone projection (optional)
# [git]
# repo = "./output"
# format = "yaml"
# author_name = "Conflux"
# author_email = "conflux@example.com"
"#
    )
}

fn generate_example_schema() -> String {
    r#"# Conflux Schema
# Defines entity types, fields, and merge strategies

name = "example"
version = "1.0"

# Define entity types
[entities.service]
# Fields for the service entity type
[[entities.service.fields]]
name = "replicas"
type = "int"
merge = "max"  # Concurrent updates take the maximum value

[[entities.service.fields]]
name = "image"
type = "string"
merge = "lww"  # Last-writer-wins

[[entities.service.fields]]
name = "enabled"
type = "bool"
merge = "lww"

# Another entity type example
[entities.route]
[[entities.route.fields]]
name = "path"
type = "string"
merge = "lww"

[[entities.route.fields]]
name = "weight"
type = "int"
merge = "max"

[[entities.route.fields]]
name = "timeout_ms"
type = "int"
merge = "min"  # Take the more conservative timeout

# Environment configuration (optional)
# [environments]
# base = "production"
# chain = ["staging", "development"]
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn init_creates_files() {
        let temp = TempDir::new().unwrap();
        let args = InitArgs {
            path: temp.path().to_path_buf(),
            schema: None,
            name: Some("test-project".to_string()),
            force: false,
        };

        run(args).unwrap();

        assert!(temp.path().join("conflux.toml").exists());
        assert!(temp.path().join("schema.toml").exists());
        assert!(temp.path().join(".conflux").exists());
    }

    #[test]
    fn init_fails_without_force() {
        let temp = TempDir::new().unwrap();

        // First init
        let args = InitArgs {
            path: temp.path().to_path_buf(),
            schema: None,
            name: None,
            force: false,
        };
        run(args).unwrap();

        // Second init without force should fail
        let args = InitArgs {
            path: temp.path().to_path_buf(),
            schema: None,
            name: None,
            force: false,
        };
        assert!(run(args).is_err());
    }
}
