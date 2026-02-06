//! Conflux CLI — schema-aware config state coordination.

use anyhow::Result;
use clap::{Parser, Subcommand};
use conflux::commands;
use conflux::config::Config;

/// Conflux — schema-aware config state coordination.
///
/// Multiple writers, deterministic merge, git milestones.
#[derive(Debug, Parser)]
#[command(name = "conflux")]
#[command(version, about, long_about = None)]
struct Cli {
    /// Path to configuration file (defaults to searching for conflux.toml).
    #[arg(long, short, global = true)]
    config: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Initialize a new Conflux project.
    Init(commands::init::InitArgs),

    /// Import existing configuration files.
    Import(commands::import::ImportArgs),

    /// Set a field value on an entity.
    Set(commands::set::SetArgs),

    /// Bulk set a field value across multiple entities.
    BulkSet(commands::bulk_set::BulkSetArgs),

    /// Get entity or field values.
    Get(commands::get::GetArgs),

    /// Show differences between states.
    Diff(commands::diff::DiffArgs),

    /// Show the operation log.
    Log(commands::log::LogArgs),

    /// Show who changed what (per-field attribution).
    Blame(commands::blame::BlameArgs),

    /// Show document status.
    Status(commands::status::StatusArgs),

    /// Milestone management.
    Milestone(commands::milestone::MilestoneArgs),

    /// Promote configuration between environments.
    Promote(commands::promote::PromoteArgs),

    /// List unresolved conflicts.
    Conflicts(commands::conflicts::ConflictsArgs),

    /// Resolve a conflict by choosing a value.
    Resolve(commands::resolve::ResolveArgs),

    /// Watch for state changes in real-time.
    Watch(commands::watch::WatchArgs),

    /// Run the Conflux daemon (API server).
    Daemon(commands::daemon::DaemonArgs),

    /// Authentication management (API keys).
    Auth(commands::auth::AuthArgs),

    /// Cryptographic key management.
    Keys(commands::keys::KeysArgs),

    /// Role-based access control management.
    Rbac(commands::rbac::RbacArgs),

    /// Audit log export and verification.
    Audit(commands::audit::AuditArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle commands that don't need an existing config
    match cli.command {
        Commands::Init(args) => return commands::init::run(args),
        Commands::Auth(args) => return commands::auth::run(args),
        Commands::Keys(args) => return commands::keys::run(args),
        Commands::Rbac(args) => return commands::rbac::run(args),
        command => {
            // Load configuration for commands that need it
            let (config, config_path) = if let Some(path) = cli.config {
                let cfg = Config::from_file(&path)?;
                (cfg, path)
            } else {
                Config::find_and_load()?
            };

            // Get the directory containing the config file
            let config_dir = config_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from("."));

            match command {
                Commands::Init(_) | Commands::Auth(_) | Commands::Keys(_) | Commands::Rbac(_) => {
                    unreachable!() // Handled above
                }
                Commands::Import(args) => commands::import::run(args, &config, &config_dir),
                Commands::Set(args) => commands::set::run(args, &config, &config_dir),
                Commands::BulkSet(args) => commands::bulk_set::run(args, &config, &config_dir),
                Commands::Get(args) => commands::get::run(args, &config, &config_dir),
                Commands::Diff(args) => commands::diff::run(args, &config, &config_dir),
                Commands::Log(args) => commands::log::run(args, &config, &config_dir),
                Commands::Blame(args) => commands::blame::run(args, &config, &config_dir),
                Commands::Status(args) => commands::status::run(args, &config, &config_dir),
                Commands::Milestone(args) => commands::milestone::run(args, &config, &config_dir),
                Commands::Promote(args) => commands::promote::run(args, &config, &config_dir),
                Commands::Conflicts(args) => commands::conflicts::run(args, &config, &config_dir),
                Commands::Resolve(args) => commands::resolve::run(args, &config, &config_dir),
                Commands::Watch(args) => commands::watch::run(args, &config, &config_dir).await,
                Commands::Daemon(args) => commands::daemon::run(args, &config, &config_dir).await,
                Commands::Audit(args) => commands::audit::run(args, &config, &config_dir),
            }
        }
    }
}
