//! Cryptographic signing and verification for operations.
//!
//! This module provides Ed25519 digital signatures for Conflux operations,
//! enabling:
//! - Non-repudiation: Operations can be verified to come from a specific key
//! - Integrity: Tampering with signed operations is detectable
//! - Offline verification: Signatures can be verified without network access
//!
//! # Key Management
//!
//! - Signing keys are stored locally (PEM format) or in environment variables
//! - Public keys are registered in a JSON registry file
//! - Keys are identified by a key ID (e.g., "alice@2024-01")
//!
//! # Usage
//!
//! ```rust,no_run
//! use conflux_core::signing::{SigningKey, VerificationKey, Signer, Verifier};
//! use conflux_core::Operation;
//!
//! // Load a signing key
//! let signing_key = SigningKey::generate();
//! let key_id = "alice@2024-01";
//!
//! // Sign an operation
//! let operation: Operation = todo!();
//! let signature = signing_key.sign_operation(&operation, key_id).unwrap();
//!
//! // Verify the signature
//! let verification_key = signing_key.verification_key();
//! assert!(verification_key.verify_operation(&operation, &signature).is_ok());
//! ```

use crate::error::ConfluxError;
use crate::operation::Operation;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ed25519_dalek::{
    Signature as DalekSignature, Signer as DalekSigner, SigningKey as DalekSigningKey,
    Verifier as DalekVerifier, VerifyingKey as DalekVerifyingKey,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

/// Errors that can occur during signing or verification.
#[derive(Debug, Error)]
pub enum SigningError {
    /// Failed to read key file.
    #[error("failed to read key file: {0}")]
    KeyRead(std::io::Error),

    /// Failed to parse key.
    #[error("failed to parse key: {0}")]
    KeyParse(String),

    /// Failed to serialize operation for signing.
    #[error("failed to serialize operation: {0}")]
    Serialization(serde_json::Error),

    /// Signature verification failed.
    #[error("signature verification failed: {0}")]
    VerificationFailed(String),

    /// Key not found in registry.
    #[error("key not found: {0}")]
    KeyNotFound(String),

    /// Invalid base64 encoding.
    #[error("invalid base64: {0}")]
    InvalidBase64(base64::DecodeError),

    /// Invalid signature format.
    #[error("invalid signature format: {0}")]
    InvalidSignature(String),
}

impl From<SigningError> for ConfluxError {
    fn from(e: SigningError) -> Self {
        ConfluxError::Validation(crate::error::ValidationError::Custom(e.to_string()))
    }
}

/// Signature scheme identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SignatureScheme {
    /// Ed25519 signature scheme (version 1).
    #[serde(rename = "ed25519v1")]
    #[default]
    Ed25519v1,
}

/// A signature on an operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationSignature {
    /// The base64-encoded signature bytes.
    pub signature: String,
    /// The key ID that created this signature.
    pub key_id: String,
    /// The signature scheme used.
    #[serde(default)]
    pub scheme: SignatureScheme,
}

impl OperationSignature {
    /// Creates a new operation signature.
    pub fn new(signature_bytes: &[u8], key_id: impl Into<String>) -> Self {
        Self {
            signature: BASE64.encode(signature_bytes),
            key_id: key_id.into(),
            scheme: SignatureScheme::Ed25519v1,
        }
    }

    /// Decodes the signature bytes.
    pub fn signature_bytes(&self) -> Result<Vec<u8>, SigningError> {
        BASE64
            .decode(&self.signature)
            .map_err(SigningError::InvalidBase64)
    }
}

/// An Ed25519 signing key.
#[derive(Debug)]
pub struct SigningKey {
    inner: DalekSigningKey,
}

impl SigningKey {
    /// Generates a new random signing key.
    pub fn generate() -> Self {
        let mut rng = rand::thread_rng();
        Self {
            inner: DalekSigningKey::generate(&mut rng),
        }
    }

    /// Loads a signing key from a PEM file.
    pub fn from_pem_file(path: &Path) -> Result<Self, SigningError> {
        let content = std::fs::read_to_string(path).map_err(SigningError::KeyRead)?;
        Self::from_pem(&content)
    }

    /// Parses a signing key from PEM format.
    pub fn from_pem(pem: &str) -> Result<Self, SigningError> {
        // Simple PEM parsing - extract base64 between headers
        let lines: Vec<&str> = pem.lines().collect();
        let mut in_key = false;
        let mut key_data = String::new();

        for line in lines {
            if line.contains("BEGIN") && line.contains("PRIVATE KEY") {
                in_key = true;
                continue;
            }
            if line.contains("END") && line.contains("PRIVATE KEY") {
                break;
            }
            if in_key {
                key_data.push_str(line.trim());
            }
        }

        if key_data.is_empty() {
            return Err(SigningError::KeyParse("no private key found in PEM".to_string()));
        }

        let bytes = BASE64
            .decode(&key_data)
            .map_err(SigningError::InvalidBase64)?;

        // Ed25519 private keys are 32 bytes, but PKCS#8 wrapping adds overhead
        // For simplicity, we support raw 32-byte keys or full PKCS#8
        let key_bytes: [u8; 32] = if bytes.len() == 32 {
            bytes.try_into().map_err(|_| {
                SigningError::KeyParse("invalid key length".to_string())
            })?
        } else if bytes.len() > 32 {
            // PKCS#8 format - key is in the last 32 bytes
            let start = bytes.len() - 32;
            bytes[start..].try_into().map_err(|_| {
                SigningError::KeyParse("invalid key length".to_string())
            })?
        } else {
            return Err(SigningError::KeyParse(format!(
                "invalid key length: {} bytes",
                bytes.len()
            )));
        };

        Ok(Self {
            inner: DalekSigningKey::from_bytes(&key_bytes),
        })
    }

    /// Exports the signing key to PEM format.
    pub fn to_pem(&self) -> String {
        let encoded = BASE64.encode(self.inner.to_bytes());
        format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            encoded
        )
    }

    /// Returns the corresponding verification key.
    pub fn verification_key(&self) -> VerificationKey {
        VerificationKey {
            inner: self.inner.verifying_key(),
        }
    }

    /// Signs an operation.
    pub fn sign_operation(
        &self,
        operation: &Operation,
        key_id: impl Into<String>,
    ) -> Result<OperationSignature, SigningError> {
        let payload = canonical_operation_bytes(operation)?;
        let signature = self.inner.sign(&payload);
        Ok(OperationSignature::new(signature.to_bytes().as_slice(), key_id))
    }
}

/// An Ed25519 verification key.
#[derive(Debug, Clone)]
pub struct VerificationKey {
    inner: DalekVerifyingKey,
}

impl VerificationKey {
    /// Parses a verification key from base64.
    pub fn from_base64(encoded: &str) -> Result<Self, SigningError> {
        let bytes = BASE64
            .decode(encoded)
            .map_err(SigningError::InvalidBase64)?;

        let key_bytes: [u8; 32] = bytes.try_into().map_err(|_| {
            SigningError::KeyParse("verification key must be 32 bytes".to_string())
        })?;

        let inner = DalekVerifyingKey::from_bytes(&key_bytes).map_err(|e| {
            SigningError::KeyParse(format!("invalid verification key: {}", e))
        })?;

        Ok(Self { inner })
    }

    /// Exports the verification key to base64.
    pub fn to_base64(&self) -> String {
        BASE64.encode(self.inner.to_bytes())
    }

    /// Verifies an operation signature.
    pub fn verify_operation(
        &self,
        operation: &Operation,
        signature: &OperationSignature,
    ) -> Result<(), SigningError> {
        let payload = canonical_operation_bytes(operation)?;
        let sig_bytes = signature.signature_bytes()?;

        let sig_array: [u8; 64] = sig_bytes.try_into().map_err(|_| {
            SigningError::InvalidSignature("signature must be 64 bytes".to_string())
        })?;

        let sig = DalekSignature::from_bytes(&sig_array);

        self.inner.verify(&payload, &sig).map_err(|e| {
            SigningError::VerificationFailed(format!("signature verification failed: {}", e))
        })
    }
}

/// The signable portion of an operation (excludes the signature field).
#[derive(Serialize)]
struct SignableOperation<'a> {
    id: &'a uuid::Uuid,
    timestamp: &'a crate::clock::HlcTimestamp,
    actor: &'a crate::identity::ActorId,
    intent: &'a Option<String>,
    kind: &'a crate::operation::OperationKind,
}

impl<'a> From<&'a Operation> for SignableOperation<'a> {
    fn from(op: &'a Operation) -> Self {
        Self {
            id: &op.id,
            timestamp: &op.timestamp,
            actor: &op.actor,
            intent: &op.intent,
            kind: &op.kind,
        }
    }
}

/// Returns the canonical bytes for signing an operation.
///
/// This produces a deterministic byte representation that can be signed
/// and verified consistently. The signature field is excluded so that
/// signing and verification produce the same bytes.
fn canonical_operation_bytes(operation: &Operation) -> Result<Vec<u8>, SigningError> {
    // Create a view that excludes the signature field
    let signable = SignableOperation::from(operation);
    // We sign the JSON representation with sorted keys for determinism
    let value = serde_json::to_value(&signable).map_err(SigningError::Serialization)?;
    serde_json::to_vec(&value).map_err(SigningError::Serialization)
}

/// A registry of public keys for signature verification.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PublicKeyRegistry {
    /// Map of key ID to key entry.
    #[serde(default)]
    pub keys: HashMap<String, PublicKeyEntry>,
}

/// An entry in the public key registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKeyEntry {
    /// The key ID.
    pub key_id: String,
    /// The actor ID this key belongs to.
    pub actor_id: String,
    /// The base64-encoded public key.
    pub public_key: String,
    /// When the key was created.
    pub created_at: String,
    /// When the key expires (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    /// Whether the key has been revoked.
    #[serde(default)]
    pub revoked: bool,
}

impl PublicKeyRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads a registry from a JSON file.
    pub fn from_file(path: &Path) -> Result<Self, SigningError> {
        let content = std::fs::read_to_string(path).map_err(SigningError::KeyRead)?;
        serde_json::from_str(&content).map_err(|e| SigningError::KeyParse(e.to_string()))
    }

    /// Saves the registry to a JSON file.
    pub fn save(&self, path: &Path) -> Result<(), SigningError> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| SigningError::KeyParse(e.to_string()))?;
        std::fs::write(path, content).map_err(SigningError::KeyRead)
    }

    /// Registers a new public key.
    pub fn register(
        &mut self,
        key_id: impl Into<String>,
        actor_id: impl Into<String>,
        public_key: &VerificationKey,
    ) {
        let key_id = key_id.into();
        self.keys.insert(
            key_id.clone(),
            PublicKeyEntry {
                key_id,
                actor_id: actor_id.into(),
                public_key: public_key.to_base64(),
                created_at: chrono::Utc::now().to_rfc3339(),
                expires_at: None,
                revoked: false,
            },
        );
    }

    /// Gets a verification key by ID.
    pub fn get_key(&self, key_id: &str) -> Result<VerificationKey, SigningError> {
        let entry = self
            .keys
            .get(key_id)
            .ok_or_else(|| SigningError::KeyNotFound(key_id.to_string()))?;

        if entry.revoked {
            return Err(SigningError::KeyNotFound(format!(
                "key '{}' has been revoked",
                key_id
            )));
        }

        VerificationKey::from_base64(&entry.public_key)
    }

    /// Revokes a key by ID.
    pub fn revoke(&mut self, key_id: &str) -> bool {
        if let Some(entry) = self.keys.get_mut(key_id) {
            entry.revoked = true;
            true
        } else {
            false
        }
    }

    /// Verifies an operation signature using the registry.
    pub fn verify_operation(
        &self,
        operation: &Operation,
        signature: &OperationSignature,
    ) -> Result<(), SigningError> {
        let key = self.get_key(&signature.key_id)?;
        key.verify_operation(operation, signature)
    }
}

/// Trait for types that can sign operations.
pub trait Signer {
    /// Signs an operation and returns the signature.
    fn sign_operation(
        &self,
        operation: &Operation,
        key_id: &str,
    ) -> Result<OperationSignature, SigningError>;
}

impl Signer for SigningKey {
    fn sign_operation(
        &self,
        operation: &Operation,
        key_id: &str,
    ) -> Result<OperationSignature, SigningError> {
        SigningKey::sign_operation(self, operation, key_id)
    }
}

/// Trait for types that can verify operation signatures.
pub trait Verifier {
    /// Verifies an operation signature.
    fn verify_operation(
        &self,
        operation: &Operation,
        signature: &OperationSignature,
    ) -> Result<(), SigningError>;
}

impl Verifier for VerificationKey {
    fn verify_operation(
        &self,
        operation: &Operation,
        signature: &OperationSignature,
    ) -> Result<(), SigningError> {
        VerificationKey::verify_operation(self, operation, signature)
    }
}

impl Verifier for PublicKeyRegistry {
    fn verify_operation(
        &self,
        operation: &Operation,
        signature: &OperationSignature,
    ) -> Result<(), SigningError> {
        PublicKeyRegistry::verify_operation(self, operation, signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{Clock, HlcTimestamp};
    use crate::field::FieldValue;
    use crate::identity::{ActorClass, ActorId, EntityId};

    fn test_timestamp() -> HlcTimestamp {
        let clock = Clock::new();
        clock.new_timestamp()
    }

    fn test_operation() -> Operation {
        let actor = ActorId::new("test-actor", ActorClass::Human);
        let timestamp = test_timestamp();

        Operation::set_field(
            EntityId::new("service.api-gateway"),
            "replicas",
            FieldValue::Int(3),
            &actor,
            timestamp,
        )
    }

    #[test]
    fn generate_and_use_signing_key() {
        let key = SigningKey::generate();
        let operation = test_operation();
        let key_id = "test@2024";

        let signature = key.sign_operation(&operation, key_id).unwrap();
        assert_eq!(signature.key_id, key_id);
        assert_eq!(signature.scheme, SignatureScheme::Ed25519v1);

        let verification_key = key.verification_key();
        assert!(verification_key.verify_operation(&operation, &signature).is_ok());
    }

    #[test]
    fn signature_fails_on_tampered_operation() {
        let key = SigningKey::generate();
        let operation = test_operation();
        let signature = key.sign_operation(&operation, "test@2024").unwrap();

        // Create a different operation
        let actor = ActorId::new("other-actor", ActorClass::Human);
        let timestamp = test_timestamp();
        let tampered = Operation::set_field(
            EntityId::new("service.api-gateway"),
            "replicas",
            FieldValue::Int(100), // Different value
            &actor,
            timestamp,
        );

        let verification_key = key.verification_key();
        assert!(verification_key.verify_operation(&tampered, &signature).is_err());
    }

    #[test]
    fn key_pem_round_trip() {
        let key = SigningKey::generate();
        let pem = key.to_pem();

        let loaded = SigningKey::from_pem(&pem).unwrap();

        // Verify they produce the same signatures
        let operation = test_operation();
        let sig1 = key.sign_operation(&operation, "test").unwrap();
        let sig2 = loaded.sign_operation(&operation, "test").unwrap();

        // Signatures include randomness in the signing process for Ed25519,
        // but both should verify with either key
        let vk = key.verification_key();
        assert!(vk.verify_operation(&operation, &sig1).is_ok());
        assert!(vk.verify_operation(&operation, &sig2).is_ok());
    }

    #[test]
    fn verification_key_base64_round_trip() {
        let key = SigningKey::generate();
        let vk = key.verification_key();

        let encoded = vk.to_base64();
        let decoded = VerificationKey::from_base64(&encoded).unwrap();

        let operation = test_operation();
        let signature = key.sign_operation(&operation, "test").unwrap();

        assert!(decoded.verify_operation(&operation, &signature).is_ok());
    }

    #[test]
    fn public_key_registry() {
        let key = SigningKey::generate();
        let vk = key.verification_key();

        let mut registry = PublicKeyRegistry::new();
        registry.register("alice@2024", "alice", &vk);

        let operation = test_operation();
        let signature = key.sign_operation(&operation, "alice@2024").unwrap();

        assert!(registry.verify_operation(&operation, &signature).is_ok());
    }

    #[test]
    fn revoked_key_fails_verification() {
        let key = SigningKey::generate();
        let vk = key.verification_key();

        let mut registry = PublicKeyRegistry::new();
        registry.register("alice@2024", "alice", &vk);
        registry.revoke("alice@2024");

        let operation = test_operation();
        let signature = key.sign_operation(&operation, "alice@2024").unwrap();

        assert!(registry.verify_operation(&operation, &signature).is_err());
    }

    #[test]
    fn unknown_key_fails_verification() {
        let registry = PublicKeyRegistry::new();
        let key = SigningKey::generate();

        let operation = test_operation();
        let signature = key.sign_operation(&operation, "unknown@2024").unwrap();

        assert!(registry.verify_operation(&operation, &signature).is_err());
    }
}
