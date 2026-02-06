//! Audit log commands.

use crate::config::Config;
use anyhow::Result;
use clap::{Args, Subcommand};
use conflux_core::PublicKeyRegistry;
use conflux_store::audit::{AuditExportQuery, AuditExporter, AuditVerifier};
use conflux_store::SqliteStore;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

/// Audit log commands.
#[derive(Debug, Args)]
pub struct AuditArgs {
    #[command(subcommand)]
    pub command: AuditCommands,
}

#[derive(Debug, Subcommand)]
pub enum AuditCommands {
    /// Export the operation log as JSON Lines.
    Export(ExportArgs),

    /// Verify signatures in an audit export.
    Verify(VerifyArgs),
}

/// Export audit log.
#[derive(Debug, Args)]
pub struct ExportArgs {
    /// Output file (use - for stdout).
    #[arg(long, short, default_value = "-")]
    pub output: String,

    /// Export operations since this timestamp (ISO 8601).
    #[arg(long)]
    pub since: Option<String>,

    /// Export operations until this timestamp (ISO 8601).
    #[arg(long)]
    pub until: Option<String>,

    /// Filter by actor ID.
    #[arg(long)]
    pub actor: Option<String>,

    /// Filter by entity ID prefix.
    #[arg(long)]
    pub entity: Option<String>,

    /// Verify signatures during export.
    #[arg(long)]
    pub verify: bool,

    /// Path to the public key registry for verification.
    #[arg(long, default_value = ".conflux/keys.json")]
    pub key_registry: PathBuf,
}

/// Verify audit export.
#[derive(Debug, Args)]
pub struct VerifyArgs {
    /// Input file to verify.
    pub input: PathBuf,

    /// Path to the public key registry.
    #[arg(long, default_value = ".conflux/keys.json")]
    pub key_registry: PathBuf,
}

/// Run the audit command.
pub fn run(args: AuditArgs, config: &Config, config_dir: &Path) -> Result<()> {
    match args.command {
        AuditCommands::Export(export_args) => run_export(export_args, config, config_dir),
        AuditCommands::Verify(verify_args) => run_verify(verify_args),
    }
}

fn run_export(args: ExportArgs, config: &Config, config_dir: &Path) -> Result<()> {
    // Open the store
    let db_path = config_dir.join(&config.database);
    let db_path_str = db_path.to_string_lossy();
    let store = SqliteStore::open(&db_path_str)?;

    // Build the query
    let mut query = AuditExportQuery::new(&config.document_id);

    if let Some(ref since) = args.since {
        query = query.since_wall_clock(since);
    }
    if let Some(ref until) = args.until {
        query = query.until_wall_clock(until);
    }
    if let Some(ref actor) = args.actor {
        query = query.by_actor(actor);
    }
    if let Some(ref entity) = args.entity {
        query = query.for_entity(entity);
    }
    if args.verify {
        query = query.verify();
    }

    // Load key registry if verifying
    let key_registry = if args.verify {
        let registry_path = config_dir.join(&args.key_registry);
        if registry_path.exists() {
            Some(PublicKeyRegistry::from_file(&registry_path)?)
        } else {
            eprintln!("Warning: Key registry not found, skipping signature verification");
            None
        }
    } else {
        None
    };

    // Create exporter
    let exporter = if let Some(ref registry) = key_registry {
        AuditExporter::new(&store).with_key_registry(registry)
    } else {
        AuditExporter::new(&store)
    };

    // Export to output
    if args.output == "-" {
        let stdout = std::io::stdout();
        let mut writer = BufWriter::new(stdout.lock());
        let stats = exporter.export(&query, &mut writer)?;
        eprintln!(
            "Exported {} operations ({} signature failures)",
            stats.operations_exported, stats.signature_failures
        );
    } else {
        let file = File::create(&args.output)?;
        let mut writer = BufWriter::new(file);
        let stats = exporter.export(&query, &mut writer)?;
        println!(
            "Exported {} operations to {} ({} signature failures)",
            stats.operations_exported, args.output, stats.signature_failures
        );
    }

    Ok(())
}

fn run_verify(args: VerifyArgs) -> Result<()> {
    // Load key registry
    if !args.key_registry.exists() {
        anyhow::bail!(
            "Key registry not found: {}",
            args.key_registry.display()
        );
    }

    let registry = PublicKeyRegistry::from_file(&args.key_registry)?;
    let verifier = AuditVerifier::new(registry);

    // Open input file
    let file = File::open(&args.input)?;
    let reader = BufReader::new(file);

    // Verify
    let result = verifier.verify(reader)?;

    println!("Verification Results:");
    println!("  Total entries:        {}", result.entries_checked);
    println!("  Signed entries:       {}", result.signed_entries);
    println!("  Unsigned entries:     {}", result.unsigned_entries);
    println!("  Valid signatures:     {}", result.valid_signatures);
    println!("  Invalid signatures:   {}", result.invalid_signatures);

    if !result.failures.is_empty() {
        println!();
        println!("Failures:");
        for failure in &result.failures {
            println!(
                "  Line {}: {} (key: {})",
                failure.line_number, failure.error, failure.key_id
            );
        }
    }

    if result.is_valid() {
        println!();
        println!("✓ All signatures are valid");
    } else {
        println!();
        println!("✗ Verification failed");
        std::process::exit(1);
    }

    Ok(())
}
