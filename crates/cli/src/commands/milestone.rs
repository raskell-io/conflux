//! Milestone management commands.

use crate::config::Config;
use crate::output::{OutputFormat, Table};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use conflux_core::{Clock, Document};
use conflux_git::{MilestoneProjector, OutputFormat as GitFormat, ProjectorConfig};
use conflux_schema::Schema;
use conflux_store::{OperationQuery, SqliteStore};
use std::path::Path;

/// Milestone management.
#[derive(Debug, Args)]
pub struct MilestoneArgs {
    #[command(subcommand)]
    pub command: MilestoneCommand,
}

/// Milestone subcommands.
#[derive(Debug, Subcommand)]
pub enum MilestoneCommand {
    /// Create a new milestone (project to git).
    Create(CreateArgs),
    /// List all milestones.
    List(ListArgs),
    /// Show details of a specific milestone.
    Show(ShowArgs),
}

/// Create a milestone.
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Milestone message.
    #[arg(long, short)]
    pub message: String,

    /// Environments to include (comma-separated).
    #[arg(long, short, default_value = "production")]
    pub environments: String,

    /// Dry run - show what would be committed.
    #[arg(long)]
    pub dry_run: bool,
}

/// List milestones.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Maximum number to show.
    #[arg(long, short, default_value = "20")]
    pub limit: usize,

    /// Output format.
    #[arg(long, short, default_value = "plain")]
    pub output: OutputFormat,
}

/// Show milestone details.
#[derive(Debug, Args)]
pub struct ShowArgs {
    /// Milestone ID (can be partial).
    pub id: String,

    /// Output format.
    #[arg(long, short, default_value = "plain")]
    pub output: OutputFormat,
}

/// Runs the milestone command.
pub fn run(args: MilestoneArgs, config: &Config, config_dir: &Path) -> Result<()> {
    match args.command {
        MilestoneCommand::Create(create_args) => run_create(create_args, config, config_dir),
        MilestoneCommand::List(list_args) => run_list(list_args, config, config_dir),
        MilestoneCommand::Show(show_args) => run_show(show_args, config, config_dir),
    }
}

fn run_create(args: CreateArgs, config: &Config, config_dir: &Path) -> Result<()> {
    let git_config = config
        .git
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("git configuration not found in conflux.toml"))?;

    // Load schema
    let schema_path = config_dir.join(&config.schema);
    let schema = Schema::from_file(&schema_path)
        .with_context(|| format!("loading schema from {}", schema_path.display()))?;

    // Open store
    let db_path = config_dir.join(&config.database);
    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid database path"))?;
    let store = SqliteStore::open(db_path_str)?;

    let clock = Clock::new();
    let mut document = Document::new();

    // Load document state
    let stored_ops = store.query_operations(&OperationQuery::new(&config.document_id).limit(10000))?;
    let schema_info = schema.as_schema_info();
    for stored in &stored_ops {
        let _ = document.apply(&stored.operation, &schema_info, &clock);
    }

    // Get operations since last milestone
    let ops_since = store.operations_since_milestone(&config.document_id)?;
    if ops_since == 0 {
        println!("No operations since last milestone. Nothing to commit.");
        return Ok(());
    }

    let operations: Vec<_> = stored_ops
        .into_iter()
        .rev()
        .take(ops_since as usize)
        .map(|s| s.operation)
        .collect();

    let environments: Vec<String> = args
        .environments
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    if args.dry_run {
        println!("Dry run - would create milestone:");
        println!("  Message: {}", args.message);
        println!("  Environments: {}", environments.join(", "));
        println!("  Operations: {}", operations.len());
        return Ok(());
    }

    // Configure projector
    let repo_path = config_dir.join(&git_config.repo);
    let format = parse_format(&git_config.format)?;
    let projector_config = ProjectorConfig {
        repo_path: repo_path.clone(),
        default_format: format,
        author_name: git_config
            .author_name
            .clone()
            .unwrap_or_else(|| "Conflux".to_string()),
        author_email: git_config
            .author_email
            .clone()
            .unwrap_or_else(|| "conflux@localhost".to_string()),
    };

    let projector = MilestoneProjector::new(projector_config);

    // Project to git
    let result = projector.project(&document, operations, &environments, &args.message)?;

    // Record milestone
    let milestone_id = if let (Some(start), Some(end)) = (result.hlc_start, result.hlc_end) {
        store.record_milestone(
            &config.document_id,
            Some(&result.commit_sha),
            &start,
            &end,
            Some(&args.message),
        )?
    } else {
        let now = clock.new_timestamp();
        store.record_milestone(
            &config.document_id,
            Some(&result.commit_sha),
            &now,
            &now,
            Some(&args.message),
        )?
    };

    println!("Created milestone: {}", milestone_id);
    println!("  Git commit: {}", result.commit_sha);
    println!("  Operations: {}", result.operation_count);
    println!("  Files written: {}", result.files_written.len());
    for file in &result.files_written {
        println!("    {}", file);
    }

    Ok(())
}

fn run_list(args: ListArgs, config: &Config, config_dir: &Path) -> Result<()> {
    // Open store
    let db_path = config_dir.join(&config.database);
    if !db_path.exists() {
        println!("No milestones yet.");
        return Ok(());
    }
    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid database path"))?;
    let store = SqliteStore::open(db_path_str)?;

    let milestones = store.list_milestones(&config.document_id)?;

    if milestones.is_empty() {
        println!("No milestones yet.");
        return Ok(());
    }

    let display_milestones: Vec<_> = milestones.into_iter().take(args.limit).collect();

    match args.output {
        OutputFormat::Plain => {
            let mut table = Table::new(vec!["ID", "Git Commit", "Message", "Created"]);
            for m in &display_milestones {
                table.add_row(vec![
                    m.id.to_string()
                        .get(..8)
                        .unwrap_or(&m.id.to_string())
                        .to_string(),
                    m.git_commit.clone().unwrap_or_else(|| "-".to_string()),
                    m.message.clone().unwrap_or_default(),
                    m.created_at.clone(),
                ]);
            }
            table.print();
        }
        OutputFormat::Json | OutputFormat::Yaml => {
            let list: Vec<serde_json::Value> = display_milestones
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "id": m.id.to_string(),
                        "git_commit": m.git_commit,
                        "message": m.message,
                        "created_at": m.created_at,
                        "hlc_range_start": m.hlc_range_start.to_string(),
                        "hlc_range_end": m.hlc_range_end.to_string(),
                    })
                })
                .collect();

            let value = serde_json::Value::Array(list);
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

fn run_show(args: ShowArgs, config: &Config, config_dir: &Path) -> Result<()> {
    // Open store
    let db_path = config_dir.join(&config.database);
    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid database path"))?;
    let store = SqliteStore::open(db_path_str)?;

    let milestones = store.list_milestones(&config.document_id)?;

    let milestone = milestones
        .into_iter()
        .find(|m| m.id.to_string().starts_with(&args.id))
        .ok_or_else(|| anyhow::anyhow!("milestone '{}' not found", args.id))?;

    match args.output {
        OutputFormat::Plain => {
            println!("Milestone: {}", milestone.id);
            println!(
                "Git Commit: {}",
                milestone.git_commit.as_deref().unwrap_or("-")
            );
            println!("Message: {}", milestone.message.as_deref().unwrap_or("-"));
            println!("Created: {}", milestone.created_at);
            println!(
                "HLC Range: {} - {}",
                milestone.hlc_range_start, milestone.hlc_range_end
            );
        }
        OutputFormat::Json | OutputFormat::Yaml => {
            let value = serde_json::json!({
                "id": milestone.id.to_string(),
                "git_commit": milestone.git_commit,
                "message": milestone.message,
                "created_at": milestone.created_at,
                "hlc_range_start": milestone.hlc_range_start.to_string(),
                "hlc_range_end": milestone.hlc_range_end.to_string(),
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

fn parse_format(s: &str) -> Result<GitFormat> {
    match s.to_lowercase().as_str() {
        "yaml" | "yml" => Ok(GitFormat::Yaml),
        "json" => Ok(GitFormat::Json),
        "toml" => Ok(GitFormat::Toml),
        "kdl" => Ok(GitFormat::Kdl),
        "xml" => Ok(GitFormat::Xml),
        "hcl" | "tf" => Ok(GitFormat::Hcl),
        _ => anyhow::bail!("unknown output format: {s}"),
    }
}
