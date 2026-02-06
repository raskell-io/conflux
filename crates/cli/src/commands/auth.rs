//! Authentication management commands.

use anyhow::Result;
use clap::{Args, Subcommand};
use conflux_api::auth::api_key::format_hash;

/// Authentication management commands.
#[derive(Debug, Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommands,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommands {
    /// Hash a raw API key for use in api-keys.toml.
    HashKey(HashKeyArgs),
}

/// Hash an API key.
#[derive(Debug, Args)]
pub struct HashKeyArgs {
    /// The raw API key to hash.
    #[arg(help = "Raw API key (use - to read from stdin)")]
    pub key: String,
}

/// Run the auth command.
pub fn run(args: AuthArgs) -> Result<()> {
    match args.command {
        AuthCommands::HashKey(hash_args) => run_hash_key(hash_args),
    }
}

fn run_hash_key(args: HashKeyArgs) -> Result<()> {
    let key = if args.key == "-" {
        // Read from stdin
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        input.trim().to_string()
    } else {
        args.key
    };

    if key.is_empty() {
        anyhow::bail!("API key cannot be empty");
    }

    let hash = format_hash(&key);
    println!("{}", hash);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_key_produces_sha256_format() {
        let hash = format_hash("test-key");
        assert!(hash.starts_with("sha256:"));
        assert_eq!(hash.len(), 7 + 64); // "sha256:" + 64 hex chars
    }
}
