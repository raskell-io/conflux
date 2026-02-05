//! Configuration loading for the CLI.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// CLI configuration, typically loaded from `conflux.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Path to the schema file.
    pub schema: PathBuf,

    /// Path to the SQLite database.
    #[serde(default = "default_database_path")]
    pub database: PathBuf,

    /// Document ID for the project.
    #[serde(default = "default_document_id")]
    pub document_id: String,

    /// Server configuration.
    #[serde(default)]
    pub server: ServerConfig,

    /// Git projection configuration.
    pub git: Option<GitConfig>,
}

/// Server configuration.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ServerConfig {
    /// HTTP listen address.
    #[serde(default = "default_http_host")]
    pub host: String,

    /// HTTP listen port.
    #[serde(default = "default_http_port")]
    pub port: u16,
}

/// Git projection configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct GitConfig {
    /// Path to the git repository.
    pub repo: PathBuf,

    /// Default output format.
    #[serde(default = "default_output_format")]
    pub format: String,

    /// Author name for commits.
    pub author_name: Option<String>,

    /// Author email for commits.
    pub author_email: Option<String>,
}

fn default_database_path() -> PathBuf {
    PathBuf::from(".conflux/state.db")
}

fn default_document_id() -> String {
    "default".to_string()
}

fn default_http_host() -> String {
    "0.0.0.0".to_string()
}

fn default_http_port() -> u16 {
    9400
}

fn default_output_format() -> String {
    "yaml".to_string()
}

impl Config {
    /// Loads configuration from a file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let config: Config =
            toml::from_str(&content).with_context(|| format!("parsing {}", path.display()))?;
        Ok(config)
    }

    /// Finds and loads configuration from the current directory or parents.
    pub fn find_and_load() -> Result<(Self, PathBuf)> {
        let config_path = find_config_file()?;
        let config = Self::from_file(&config_path)?;
        Ok((config, config_path))
    }
}

/// Default configuration file names to search for.
const CONFIG_FILES: &[&str] = &["conflux.toml", ".conflux.toml"];

/// Finds the configuration file by walking up from the current directory.
pub fn find_config_file() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("getting current directory")?;
    let mut dir = cwd.as_path();

    loop {
        for name in CONFIG_FILES {
            let path = dir.join(name);
            if path.exists() {
                return Ok(path);
            }
        }

        match dir.parent() {
            Some(parent) => dir = parent,
            None => {
                anyhow::bail!(
                    "no conflux.toml found in current directory or any parent directory"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn parse_minimal_config() {
        let content = r#"
schema = "schema.toml"
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let config = Config::from_file(file.path()).unwrap();
        assert_eq!(config.schema, PathBuf::from("schema.toml"));
        assert_eq!(config.database, PathBuf::from(".conflux/state.db"));
        assert_eq!(config.document_id, "default");
    }

    #[test]
    fn parse_full_config() {
        let content = r#"
schema = "my-schema.toml"
database = "data/conflux.db"
document_id = "my-project"

[server]
host = "127.0.0.1"
port = 8080

[git]
repo = "./output"
format = "json"
author_name = "Conflux Bot"
author_email = "conflux@example.com"
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let config = Config::from_file(file.path()).unwrap();
        assert_eq!(config.schema, PathBuf::from("my-schema.toml"));
        assert_eq!(config.database, PathBuf::from("data/conflux.db"));
        assert_eq!(config.document_id, "my-project");
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 8080);

        let git = config.git.unwrap();
        assert_eq!(git.repo, PathBuf::from("./output"));
        assert_eq!(git.format, "json");
        assert_eq!(git.author_name, Some("Conflux Bot".to_string()));
    }
}
