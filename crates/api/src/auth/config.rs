//! Authentication configuration types.

use serde::Deserialize;
use std::path::PathBuf;

/// Authentication mode.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMode {
    /// No authentication required.
    #[default]
    None,
    /// API key authentication only.
    ApiKey,
    /// mTLS authentication only.
    Mtls,
    /// Both API key and mTLS are accepted.
    Both,
}

/// Top-level authentication configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AuthConfig {
    /// Authentication mode.
    #[serde(default)]
    pub mode: AuthMode,
    /// Whether to allow unauthenticated requests (legacy header mode).
    #[serde(default)]
    pub allow_unauthenticated: bool,
    /// API key configuration.
    pub api_keys: Option<ApiKeyConfig>,
    /// mTLS configuration.
    pub mtls: Option<MtlsConfig>,
}

/// API key storage configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiKeyConfig {
    /// Storage type (currently only "file" is supported).
    #[serde(default = "default_storage")]
    pub storage: String,
    /// Path to the API keys file.
    pub file: PathBuf,
}

fn default_storage() -> String {
    "file".to_string()
}

/// mTLS configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct MtlsConfig {
    /// Path to the CA certificate.
    pub ca_cert: PathBuf,
    /// Path to the server certificate.
    pub server_cert: PathBuf,
    /// Path to the server private key.
    pub server_key: PathBuf,
    /// How to map certificates to actor IDs.
    #[serde(default)]
    pub certificate_mapping: CertificateMapping,
}

/// How to extract actor ID from a client certificate.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CertificateMapping {
    /// Use the Common Name (CN) from the subject.
    #[default]
    Cn,
    /// Use the Subject Alternative Name (SAN).
    San,
    /// Use a custom mapping file.
    Mapping,
}

/// An API key entry in the keys file.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiKeyEntry {
    /// SHA-256 hash of the key (format: "sha256:hex_digest").
    pub key_hash: String,
    /// Actor ID associated with this key.
    pub actor_id: String,
    /// Actor class.
    pub actor_class: ApiKeyActorClass,
    /// Human-readable display name.
    pub display_name: Option<String>,
}

/// Actor class for API key entries.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ApiKeyActorClass {
    Human,
    Pipeline,
    Operator,
    System,
}

impl From<ApiKeyActorClass> for conflux_core::ActorClass {
    fn from(class: ApiKeyActorClass) -> Self {
        match class {
            ApiKeyActorClass::Human => conflux_core::ActorClass::Human,
            ApiKeyActorClass::Pipeline => conflux_core::ActorClass::Pipeline,
            ApiKeyActorClass::Operator => conflux_core::ActorClass::Operator,
            ApiKeyActorClass::System => conflux_core::ActorClass::System,
        }
    }
}

/// API keys file structure.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiKeysFile {
    /// List of API key entries.
    #[serde(default)]
    pub keys: Vec<ApiKeyEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_auth_config() {
        let toml = r#"
            mode = "api-key"
            allow_unauthenticated = false

            [api_keys]
            storage = "file"
            file = ".conflux/api-keys.toml"
        "#;

        let config: AuthConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.mode, AuthMode::ApiKey);
        assert!(!config.allow_unauthenticated);
        assert!(config.api_keys.is_some());
    }

    #[test]
    fn parse_api_keys_file() {
        let toml = r#"
            [[keys]]
            key_hash = "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
            actor_id = "ci-pipeline"
            actor_class = "pipeline"
            display_name = "GitHub Actions CI"
        "#;

        let file: ApiKeysFile = toml::from_str(toml).unwrap();
        assert_eq!(file.keys.len(), 1);
        assert_eq!(file.keys[0].actor_id, "ci-pipeline");
        assert_eq!(file.keys[0].actor_class, ApiKeyActorClass::Pipeline);
    }

    #[test]
    fn actor_class_conversion() {
        let class = ApiKeyActorClass::Pipeline;
        let core_class: conflux_core::ActorClass = class.into();
        assert_eq!(core_class, conflux_core::ActorClass::Pipeline);
    }
}
