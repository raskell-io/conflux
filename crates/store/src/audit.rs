//! Audit log export functionality.
//!
//! This module provides JSON Lines (NDJSON) export of the operation log
//! for compliance, security analysis, and external tooling integration.
//!
//! # Export Format
//!
//! Each line is a self-contained JSON object:
//!
//! ```json
//! {
//!   "version": "1",
//!   "export_time": "2024-01-15T10:30:00Z",
//!   "operation": { ... },
//!   "metadata": {
//!     "document_id": "default",
//!     "stored_at": "2024-01-15T10:00:00Z",
//!     "signature_verified": true
//!   }
//! }
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! use conflux_store::{Store, SqliteStore};
//! use conflux_store::audit::{AuditExportQuery, AuditExporter};
//!
//! let store = SqliteStore::open("data.db").unwrap();
//! let exporter = AuditExporter::new(&store);
//!
//! let query = AuditExportQuery::new("my-document")
//!     .since_wall_clock("2024-01-01T00:00:00Z")
//!     .by_actor("alice");
//!
//! let mut output = Vec::new();
//! exporter.export(&query, &mut output).unwrap();
//! ```

use crate::Store;
use chrono::{DateTime, Utc};
use conflux_core::{HlcTimestamp, Operation, PublicKeyRegistry};
use serde::{Deserialize, Serialize};
use std::io::Write;

/// Query parameters for audit log export.
#[derive(Debug, Clone, Default)]
pub struct AuditExportQuery {
    /// Document ID to export.
    pub document_id: String,
    /// Export operations since this HLC timestamp.
    pub since: Option<HlcTimestamp>,
    /// Export operations until this HLC timestamp.
    pub until: Option<HlcTimestamp>,
    /// Filter by actor ID.
    pub actor_filter: Option<String>,
    /// Filter by entity ID prefix.
    pub entity_filter: Option<String>,
    /// Include full document state at each operation (expensive).
    pub include_state: bool,
    /// Verify operation signatures during export.
    pub verify_signatures: bool,
}

impl AuditExportQuery {
    /// Creates a new query for the given document.
    pub fn new(document_id: impl Into<String>) -> Self {
        Self {
            document_id: document_id.into(),
            ..Default::default()
        }
    }

    /// Filters operations since the given HLC timestamp.
    pub fn since(mut self, ts: &HlcTimestamp) -> Self {
        self.since = Some(*ts);
        self
    }

    /// Filters operations until the given HLC timestamp.
    pub fn until(mut self, ts: &HlcTimestamp) -> Self {
        self.until = Some(*ts);
        self
    }

    /// Parses and sets the since filter from a wall clock timestamp string.
    ///
    /// Note: This uses the string representation which the HlcTimestamp can parse.
    pub fn since_wall_clock(mut self, iso8601: &str) -> Self {
        if let Ok(dt) = DateTime::parse_from_rfc3339(iso8601) {
            // Format as HLC-compatible string: time/counter/id
            let ts_str = format!("{}/0/0", dt.format("%Y-%m-%dT%H:%M:%S%.9fZ"));
            if let Ok(ts) = serde_json::from_value::<HlcTimestamp>(serde_json::json!(ts_str)) {
                self.since = Some(ts);
            }
        }
        self
    }

    /// Parses and sets the until filter from a wall clock timestamp string.
    ///
    /// Note: This uses the string representation which the HlcTimestamp can parse.
    pub fn until_wall_clock(mut self, iso8601: &str) -> Self {
        if let Ok(dt) = DateTime::parse_from_rfc3339(iso8601) {
            // Format as HLC-compatible string with max counter/id
            let ts_str = format!("{}/4294967295/18446744073709551615", dt.format("%Y-%m-%dT%H:%M:%S%.9fZ"));
            if let Ok(ts) = serde_json::from_value::<HlcTimestamp>(serde_json::json!(ts_str)) {
                self.until = Some(ts);
            }
        }
        self
    }

    /// Filters by actor ID.
    pub fn by_actor(mut self, actor_id: impl Into<String>) -> Self {
        self.actor_filter = Some(actor_id.into());
        self
    }

    /// Filters by entity ID prefix.
    pub fn for_entity(mut self, entity_prefix: impl Into<String>) -> Self {
        self.entity_filter = Some(entity_prefix.into());
        self
    }

    /// Includes document state with each operation.
    pub fn with_state(mut self) -> Self {
        self.include_state = true;
        self
    }

    /// Verifies signatures during export.
    pub fn verify(mut self) -> Self {
        self.verify_signatures = true;
        self
    }
}

/// A single line in the audit export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Export format version.
    pub version: String,
    /// When this export was generated.
    pub export_time: String,
    /// The operation.
    pub operation: Operation,
    /// Export metadata.
    pub metadata: AuditMetadata,
}

/// Metadata about an exported operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditMetadata {
    /// Document ID.
    pub document_id: String,
    /// When the operation was stored.
    pub stored_at: String,
    /// Whether the operation's signature was verified (if present).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_verified: Option<bool>,
    /// Signature key ID (if signed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_key_id: Option<String>,
}

/// Audit log exporter.
pub struct AuditExporter<'a, S: Store + ?Sized> {
    store: &'a S,
    key_registry: Option<&'a PublicKeyRegistry>,
}

impl<'a, S: Store + ?Sized> AuditExporter<'a, S> {
    /// Creates a new exporter for the given store.
    pub fn new(store: &'a S) -> Self {
        Self {
            store,
            key_registry: None,
        }
    }

    /// Sets the public key registry for signature verification.
    pub fn with_key_registry(mut self, registry: &'a PublicKeyRegistry) -> Self {
        self.key_registry = Some(registry);
        self
    }

    /// Exports operations matching the query to the given writer.
    ///
    /// Each operation is written as a JSON line (NDJSON format).
    pub fn export<W: Write>(
        &self,
        query: &AuditExportQuery,
        writer: &mut W,
    ) -> Result<ExportStats, crate::StoreError> {
        let export_time = Utc::now().to_rfc3339();
        let mut stats = ExportStats::default();

        // Build the store query
        let mut store_query = crate::OperationQuery::new(&query.document_id);

        if let Some(ref since) = query.since {
            store_query = store_query.since(since);
        }
        if let Some(ref until) = query.until {
            store_query = store_query.until(until);
        }
        if let Some(ref actor) = query.actor_filter {
            store_query = store_query.by_actor(actor);
        }
        if let Some(ref entity) = query.entity_filter {
            store_query = store_query.for_entity(entity);
        }

        // Execute query
        let operations = self.store.query_operations(&store_query)?;

        for stored_op in operations {
            stats.operations_exported += 1;

            let signature_verified = if query.verify_signatures {
                self.verify_signature(&stored_op.operation)
            } else {
                None
            };

            if signature_verified == Some(false) {
                stats.signature_failures += 1;
            }

            let entry = AuditEntry {
                version: "1".to_string(),
                export_time: export_time.clone(),
                operation: stored_op.operation.clone(),
                metadata: AuditMetadata {
                    document_id: stored_op.document_id.clone(),
                    stored_at: stored_op.created_at.clone(),
                    signature_verified,
                    signature_key_id: stored_op
                        .operation
                        .signature
                        .as_ref()
                        .map(|s| s.key_id.clone()),
                },
            };

            let line = serde_json::to_string(&entry)
                .map_err(|e| crate::StoreError::Backend(e.to_string()))?;

            writeln!(writer, "{}", line)
                .map_err(|e| crate::StoreError::Backend(e.to_string()))?;
        }

        Ok(stats)
    }

    /// Verifies the signature on an operation.
    fn verify_signature(&self, operation: &Operation) -> Option<bool> {
        let signature = operation.signature.as_ref()?;

        let registry = self.key_registry?;

        Some(registry.verify_operation(operation, signature).is_ok())
    }
}

/// Statistics from an export operation.
#[derive(Debug, Clone, Default)]
pub struct ExportStats {
    /// Number of operations exported.
    pub operations_exported: usize,
    /// Number of signature verification failures.
    pub signature_failures: usize,
}

/// Verifies signatures in an audit export file.
pub struct AuditVerifier {
    key_registry: PublicKeyRegistry,
}

impl AuditVerifier {
    /// Creates a new verifier with the given key registry.
    pub fn new(key_registry: PublicKeyRegistry) -> Self {
        Self { key_registry }
    }

    /// Verifies all signatures in an audit export.
    ///
    /// Returns verification results for each entry.
    pub fn verify<R: std::io::BufRead>(
        &self,
        reader: R,
    ) -> Result<VerificationResult, crate::StoreError> {
        let mut result = VerificationResult::default();

        for (line_num, line_result) in reader.lines().enumerate() {
            let line = line_result.map_err(|e| crate::StoreError::Backend(e.to_string()))?;

            if line.trim().is_empty() {
                continue;
            }

            let entry: AuditEntry = serde_json::from_str(&line)
                .map_err(|e| crate::StoreError::Backend(format!("line {}: {}", line_num + 1, e)))?;

            result.entries_checked += 1;

            match &entry.operation.signature {
                Some(signature) => {
                    result.signed_entries += 1;

                    match self.key_registry.verify_operation(&entry.operation, signature) {
                        Ok(()) => {
                            result.valid_signatures += 1;
                        }
                        Err(e) => {
                            result.invalid_signatures += 1;
                            result.failures.push(VerificationFailure {
                                line_number: line_num + 1,
                                operation_id: entry.operation.id,
                                key_id: signature.key_id.clone(),
                                error: e.to_string(),
                            });
                        }
                    }
                }
                None => {
                    result.unsigned_entries += 1;
                }
            }
        }

        Ok(result)
    }
}

/// Result of verifying an audit export.
#[derive(Debug, Clone, Default)]
pub struct VerificationResult {
    /// Total entries checked.
    pub entries_checked: usize,
    /// Entries with valid signatures.
    pub valid_signatures: usize,
    /// Entries with invalid signatures.
    pub invalid_signatures: usize,
    /// Entries that are signed.
    pub signed_entries: usize,
    /// Entries without signatures.
    pub unsigned_entries: usize,
    /// Details of verification failures.
    pub failures: Vec<VerificationFailure>,
}

impl VerificationResult {
    /// Returns true if all signed entries have valid signatures.
    pub fn is_valid(&self) -> bool {
        self.invalid_signatures == 0
    }
}

/// A signature verification failure.
#[derive(Debug, Clone)]
pub struct VerificationFailure {
    /// Line number in the export file (1-indexed).
    pub line_number: usize,
    /// The operation ID.
    pub operation_id: uuid::Uuid,
    /// The key ID that was used.
    pub key_id: String,
    /// The error message.
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStore;
    use conflux_core::{ActorClass, ActorId, Clock, FieldValue, SigningKey};

    fn test_store_with_operations() -> MemoryStore {
        let store = MemoryStore::new();
        let actor = ActorId::new("test-actor", ActorClass::Human);
        let clock = Clock::new();

        for i in 0..5 {
            let ts = clock.new_timestamp();
            let op = conflux_core::Operation::set_field(
                format!("entity.{}", i),
                "field",
                FieldValue::Int(i as i64),
                &actor,
                ts,
            );
            store.append_operation("test-doc", &op).unwrap();
        }

        store
    }

    #[test]
    fn export_all_operations() {
        let store = test_store_with_operations();
        let exporter = AuditExporter::new(&store);

        let query = AuditExportQuery::new("test-doc");
        let mut output = Vec::new();
        let stats = exporter.export(&query, &mut output).unwrap();

        assert_eq!(stats.operations_exported, 5);

        let output_str = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = output_str.lines().collect();
        assert_eq!(lines.len(), 5);

        // Parse first line
        let entry: AuditEntry = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(entry.version, "1");
        assert_eq!(entry.metadata.document_id, "test-doc");
    }

    #[test]
    fn export_with_actor_filter() {
        let store = MemoryStore::new();
        let actor1 = ActorId::new("alice", ActorClass::Human);
        let actor2 = ActorId::new("bob", ActorClass::Human);
        let clock = Clock::new();

        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();

        let op1 = conflux_core::Operation::set_field(
            "entity.1",
            "field",
            FieldValue::Int(1),
            &actor1,
            ts1,
        );
        let op2 = conflux_core::Operation::set_field(
            "entity.2",
            "field",
            FieldValue::Int(2),
            &actor2,
            ts2,
        );

        store.append_operation("test-doc", &op1).unwrap();
        store.append_operation("test-doc", &op2).unwrap();

        let exporter = AuditExporter::new(&store);
        let query = AuditExportQuery::new("test-doc").by_actor("alice");

        let mut output = Vec::new();
        let stats = exporter.export(&query, &mut output).unwrap();

        assert_eq!(stats.operations_exported, 1);
    }

    #[test]
    fn export_with_signature_verification() {
        let store = MemoryStore::new();
        let actor = ActorId::new("alice", ActorClass::Human);
        let signing_key = SigningKey::generate();
        let mut registry = PublicKeyRegistry::new();
        registry.register("alice@2024", "alice", &signing_key.verification_key());
        let clock = Clock::new();

        let ts = clock.new_timestamp();
        let mut op = conflux_core::Operation::set_field(
            "entity.1",
            "field",
            FieldValue::Int(1),
            &actor,
            ts,
        );

        // Sign the operation
        let signature = signing_key.sign_operation(&op, "alice@2024").unwrap();
        op = op.with_signature(signature);

        store.append_operation("test-doc", &op).unwrap();

        let exporter = AuditExporter::new(&store).with_key_registry(&registry);
        let query = AuditExportQuery::new("test-doc").verify();

        let mut output = Vec::new();
        let stats = exporter.export(&query, &mut output).unwrap();

        assert_eq!(stats.operations_exported, 1);
        assert_eq!(stats.signature_failures, 0);

        let output_str = String::from_utf8(output).unwrap();
        let entry: AuditEntry = serde_json::from_str(output_str.lines().next().unwrap()).unwrap();
        assert_eq!(entry.metadata.signature_verified, Some(true));
        assert_eq!(entry.metadata.signature_key_id, Some("alice@2024".to_string()));
    }

    #[test]
    fn verify_audit_export() {
        let signing_key = SigningKey::generate();
        let mut registry = PublicKeyRegistry::new();
        registry.register("test@2024", "test-actor", &signing_key.verification_key());

        let actor = ActorId::new("test-actor", ActorClass::Human);
        let clock = Clock::new();
        let ts = clock.new_timestamp();
        let mut op = conflux_core::Operation::set_field(
            "entity.1",
            "field",
            FieldValue::Int(1),
            &actor,
            ts,
        );

        let signature = signing_key.sign_operation(&op, "test@2024").unwrap();
        op = op.with_signature(signature);

        let entry = AuditEntry {
            version: "1".to_string(),
            export_time: Utc::now().to_rfc3339(),
            operation: op,
            metadata: AuditMetadata {
                document_id: "test-doc".to_string(),
                stored_at: Utc::now().to_rfc3339(),
                signature_verified: None,
                signature_key_id: Some("test@2024".to_string()),
            },
        };

        let line = serde_json::to_string(&entry).unwrap();
        let verifier = AuditVerifier::new(registry);

        let result = verifier.verify(line.as_bytes()).unwrap();

        assert!(result.is_valid());
        assert_eq!(result.entries_checked, 1);
        assert_eq!(result.valid_signatures, 1);
        assert_eq!(result.unsigned_entries, 0);
    }
}
