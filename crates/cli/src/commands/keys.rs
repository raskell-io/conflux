//! Cryptographic key management commands.

use anyhow::Result;
use clap::{Args, Subcommand};
use conflux_core::{PublicKeyRegistry, SigningKey, VerificationKey};
use std::path::PathBuf;

/// Key management commands.
#[derive(Debug, Args)]
pub struct KeysArgs {
    #[command(subcommand)]
    pub command: KeysCommands,
}

#[derive(Debug, Subcommand)]
pub enum KeysCommands {
    /// Generate a new Ed25519 signing key pair.
    Generate(GenerateArgs),

    /// Register a public key in the registry.
    Register(RegisterArgs),

    /// List registered public keys.
    List(ListArgs),

    /// Revoke a public key.
    Revoke(RevokeArgs),
}

/// Generate a new key pair.
#[derive(Debug, Args)]
pub struct GenerateArgs {
    /// Actor ID this key belongs to.
    #[arg(long)]
    pub actor: String,

    /// Output file for the private key (PEM format).
    #[arg(long, short)]
    pub output: PathBuf,

    /// Don't print the public key to stdout.
    #[arg(long)]
    pub quiet: bool,
}

/// Register a public key.
#[derive(Debug, Args)]
pub struct RegisterArgs {
    /// Key ID (e.g., "alice@2024-01").
    #[arg(long)]
    pub key_id: String,

    /// Actor ID this key belongs to.
    #[arg(long)]
    pub actor: String,

    /// Base64-encoded public key.
    #[arg(long)]
    pub public_key: String,

    /// Path to the key registry file.
    #[arg(long, default_value = ".conflux/keys.json")]
    pub registry: PathBuf,
}

/// List registered keys.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Path to the key registry file.
    #[arg(long, default_value = ".conflux/keys.json")]
    pub registry: PathBuf,

    /// Output format (table, json).
    #[arg(long, default_value = "table")]
    pub format: String,
}

/// Revoke a key.
#[derive(Debug, Args)]
pub struct RevokeArgs {
    /// Key ID to revoke.
    pub key_id: String,

    /// Path to the key registry file.
    #[arg(long, default_value = ".conflux/keys.json")]
    pub registry: PathBuf,
}

/// Run the keys command.
pub fn run(args: KeysArgs) -> Result<()> {
    match args.command {
        KeysCommands::Generate(gen_args) => run_generate(gen_args),
        KeysCommands::Register(reg_args) => run_register(reg_args),
        KeysCommands::List(list_args) => run_list(list_args),
        KeysCommands::Revoke(revoke_args) => run_revoke(revoke_args),
    }
}

fn run_generate(args: GenerateArgs) -> Result<()> {
    let signing_key = SigningKey::generate();
    let verification_key = signing_key.verification_key();

    // Write the private key
    let pem = signing_key.to_pem();

    // Ensure parent directory exists
    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&args.output, pem)?;

    // Set restrictive permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&args.output)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&args.output, perms)?;
    }

    if !args.quiet {
        println!("Private key written to: {}", args.output.display());
        println!();
        println!("Public key (base64):");
        println!("{}", verification_key.to_base64());
        println!();
        println!("Suggested key ID: {}@{}", args.actor, chrono::Utc::now().format("%Y-%m"));
    }

    Ok(())
}

fn run_register(args: RegisterArgs) -> Result<()> {
    // Load or create registry
    let mut registry = if args.registry.exists() {
        PublicKeyRegistry::from_file(&args.registry)?
    } else {
        PublicKeyRegistry::new()
    };

    // Validate the public key
    let verification_key = VerificationKey::from_base64(&args.public_key)?;

    // Register the key
    registry.register(&args.key_id, &args.actor, &verification_key);

    // Ensure parent directory exists
    if let Some(parent) = args.registry.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Save the registry
    registry.save(&args.registry)?;

    println!("Registered key '{}' for actor '{}'", args.key_id, args.actor);

    Ok(())
}

fn run_list(args: ListArgs) -> Result<()> {
    if !args.registry.exists() {
        println!("No keys registered (registry file not found)");
        return Ok(());
    }

    let registry = PublicKeyRegistry::from_file(&args.registry)?;

    if registry.keys.is_empty() {
        println!("No keys registered");
        return Ok(());
    }

    match args.format.as_str() {
        "json" => {
            let output = serde_json::to_string_pretty(&registry)?;
            println!("{}", output);
        }
        _ => {
            println!("{:<20} {:<15} {:<12} CREATED", "KEY ID", "ACTOR", "STATUS");
            println!("{}", "-".repeat(70));

            for (key_id, entry) in &registry.keys {
                let status = if entry.revoked { "revoked" } else { "active" };
                println!(
                    "{:<20} {:<15} {:<12} {}",
                    key_id, entry.actor_id, status, entry.created_at
                );
            }
        }
    }

    Ok(())
}

fn run_revoke(args: RevokeArgs) -> Result<()> {
    if !args.registry.exists() {
        anyhow::bail!("Registry file not found: {}", args.registry.display());
    }

    let mut registry = PublicKeyRegistry::from_file(&args.registry)?;

    if !registry.revoke(&args.key_id) {
        anyhow::bail!("Key '{}' not found in registry", args.key_id);
    }

    registry.save(&args.registry)?;

    println!("Revoked key '{}'", args.key_id);

    Ok(())
}
