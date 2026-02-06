//! API key authentication.

use super::config::{ApiKeyEntry, ApiKeysFile};
use crate::error::ApiError;
use conflux_core::{ActorClass, ActorId};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// API key authenticator.
#[derive(Debug, Clone)]
pub struct ApiKeyAuthenticator {
    /// Map of key hash to actor info.
    keys: Arc<HashMap<String, ApiKeyInfo>>,
}

/// Information about an API key.
#[derive(Debug, Clone)]
pub struct ApiKeyInfo {
    /// The raw key hash (sha256:...).
    pub key_hash: String,
    /// The actor ID.
    pub actor_id: String,
    /// The actor class.
    pub actor_class: ActorClass,
    /// Optional display name.
    pub display_name: Option<String>,
}

impl ApiKeyAuthenticator {
    /// Creates an empty authenticator (for testing).
    pub fn empty() -> Self {
        Self {
            keys: Arc::new(HashMap::new()),
        }
    }

    /// Loads API keys from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn from_file(path: &Path) -> Result<Self, ApiError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            ApiError::Internal(format!("failed to read API keys file {}: {}", path.display(), e))
        })?;

        let file: ApiKeysFile = toml::from_str(&content).map_err(|e| {
            ApiError::Internal(format!(
                "failed to parse API keys file {}: {}",
                path.display(),
                e
            ))
        })?;

        Ok(Self::from_entries(&file.keys))
    }

    /// Creates an authenticator from a list of key entries.
    pub fn from_entries(entries: &[ApiKeyEntry]) -> Self {
        let keys = entries
            .iter()
            .map(|entry| {
                let hash = normalize_hash(&entry.key_hash);
                let info = ApiKeyInfo {
                    key_hash: entry.key_hash.clone(),
                    actor_id: entry.actor_id.clone(),
                    actor_class: entry.actor_class.into(),
                    display_name: entry.display_name.clone(),
                };
                (hash, info)
            })
            .collect();

        Self {
            keys: Arc::new(keys),
        }
    }

    /// Authenticates a raw API key.
    ///
    /// Returns the actor ID if the key is valid.
    pub fn authenticate(&self, raw_key: &str) -> Option<AuthenticatedActor> {
        let hash = hash_key(raw_key);
        self.keys.get(&hash).map(|info| AuthenticatedActor {
            actor: ActorId::new(&info.actor_id, info.actor_class),
            method: AuthMethod::ApiKey {
                key_id: info
                    .display_name
                    .clone()
                    .unwrap_or_else(|| info.actor_id.clone()),
            },
        })
    }

    /// Returns the number of registered keys.
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }
}

/// An authenticated actor with authentication metadata.
#[derive(Debug, Clone)]
pub struct AuthenticatedActor {
    /// The authenticated actor identity.
    pub actor: ActorId,
    /// How the actor was authenticated.
    pub method: AuthMethod,
}

/// How an actor was authenticated.
#[derive(Debug, Clone)]
pub enum AuthMethod {
    /// Authenticated via API key.
    ApiKey {
        /// Key ID or display name.
        key_id: String,
    },
    /// Authenticated via mTLS client certificate.
    Mtls {
        /// Certificate fingerprint.
        cert_fingerprint: String,
    },
    /// Legacy header-based authentication.
    LegacyHeaders,
}

/// Hashes a raw API key using SHA-256.
pub fn hash_key(raw_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

/// Formats a hash in the canonical format (sha256:hex).
pub fn format_hash(raw_key: &str) -> String {
    format!("sha256:{}", hash_key(raw_key))
}

/// Normalizes a hash by stripping the "sha256:" prefix if present.
fn normalize_hash(hash: &str) -> String {
    hash.strip_prefix("sha256:").unwrap_or(hash).to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::config::ApiKeyActorClass;

    #[test]
    fn hash_key_deterministic() {
        let hash1 = hash_key("test-api-key");
        let hash2 = hash_key("test-api-key");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn hash_key_differs_for_different_keys() {
        let hash1 = hash_key("key1");
        let hash2 = hash_key("key2");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn format_hash_includes_prefix() {
        let formatted = format_hash("hello");
        assert!(formatted.starts_with("sha256:"));
        // SHA-256 of "hello" is 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        assert_eq!(
            formatted,
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn authenticate_valid_key() {
        let entries = vec![ApiKeyEntry {
            key_hash: format_hash("secret-key"),
            actor_id: "test-actor".to_string(),
            actor_class: ApiKeyActorClass::Pipeline,
            display_name: Some("Test Key".to_string()),
        }];

        let auth = ApiKeyAuthenticator::from_entries(&entries);
        let result = auth.authenticate("secret-key");

        assert!(result.is_some());
        let authenticated = result.unwrap();
        assert_eq!(authenticated.actor.id, "test-actor");
        assert_eq!(authenticated.actor.class, ActorClass::Pipeline);
        assert!(matches!(authenticated.method, AuthMethod::ApiKey { key_id } if key_id == "Test Key"));
    }

    #[test]
    fn authenticate_invalid_key() {
        let entries = vec![ApiKeyEntry {
            key_hash: format_hash("secret-key"),
            actor_id: "test-actor".to_string(),
            actor_class: ApiKeyActorClass::Pipeline,
            display_name: None,
        }];

        let auth = ApiKeyAuthenticator::from_entries(&entries);
        let result = auth.authenticate("wrong-key");

        assert!(result.is_none());
    }

    #[test]
    fn empty_authenticator() {
        let auth = ApiKeyAuthenticator::empty();
        assert_eq!(auth.key_count(), 0);
        assert!(auth.authenticate("any-key").is_none());
    }
}
