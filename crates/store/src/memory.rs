//! In-memory storage backend for testing and embedded use.

use crate::error::StoreError;
use crate::models::{StoredMilestone, StoredOperation, StoredSnapshot};
use crate::query::OperationQuery;
use crate::traits::Store;
use chrono::Utc;
use conflux_core::{Document, HlcTimestamp, Operation, OperationKind};
use parking_lot::RwLock;
use std::collections::HashMap;
use uuid::Uuid;

/// In-memory store for operations, snapshots, and milestones.
///
/// This implementation is useful for:
/// - Unit and integration testing
/// - Embedded use cases where persistence isn't needed
/// - Development and prototyping
///
/// All data is stored in memory and lost when the store is dropped.
pub struct MemoryStore {
    /// Operations keyed by document_id, ordered by insertion.
    operations: RwLock<HashMap<String, Vec<StoredOperation>>>,
    /// Secondary index: operation_id -> (document_id, index in operations vec).
    operations_by_id: RwLock<HashMap<Uuid, (String, usize)>>,
    /// Snapshots keyed by document_id, ordered by insertion (newest last).
    snapshots: RwLock<HashMap<String, Vec<StoredSnapshot>>>,
    /// Milestones keyed by document_id, ordered by insertion (newest last).
    milestones: RwLock<HashMap<String, Vec<StoredMilestone>>>,
    /// Version vectors keyed by document_id.
    version_vectors: RwLock<HashMap<String, String>>,
}

impl MemoryStore {
    /// Creates a new empty in-memory store.
    pub fn new() -> Self {
        Self {
            operations: RwLock::new(HashMap::new()),
            operations_by_id: RwLock::new(HashMap::new()),
            snapshots: RwLock::new(HashMap::new()),
            milestones: RwLock::new(HashMap::new()),
            version_vectors: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Store for MemoryStore {
    fn append_operation(&self, document_id: &str, operation: &Operation) -> Result<(), StoreError> {
        let stored = StoredOperation {
            operation: operation.clone(),
            document_id: document_id.to_string(),
            created_at: Utc::now().to_rfc3339(),
        };

        let mut ops = self.operations.write();
        let mut ops_by_id = self.operations_by_id.write();

        let doc_ops = ops.entry(document_id.to_string()).or_default();
        let index = doc_ops.len();
        doc_ops.push(stored);
        ops_by_id.insert(operation.id, (document_id.to_string(), index));

        Ok(())
    }

    fn get_operation(&self, operation_id: &Uuid) -> Result<StoredOperation, StoreError> {
        let ops_by_id = self.operations_by_id.read();
        let ops = self.operations.read();

        let (doc_id, index) = ops_by_id
            .get(operation_id)
            .ok_or_else(|| StoreError::NotFound(format!("operation {operation_id}")))?;

        let doc_ops = ops
            .get(doc_id)
            .ok_or_else(|| StoreError::NotFound(format!("operation {operation_id}")))?;

        doc_ops
            .get(*index)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("operation {operation_id}")))
    }

    fn query_operations(&self, query: &OperationQuery) -> Result<Vec<StoredOperation>, StoreError> {
        let ops = self.operations.read();
        let doc_ops = match ops.get(&query.document_id) {
            Some(ops) => ops,
            None => return Ok(Vec::new()),
        };

        let mut results: Vec<StoredOperation> = doc_ops
            .iter()
            .filter(|op| {
                // Filter by entity_id
                if let Some(ref entity_id) = query.entity_id {
                    if entity_id_from_op(&op.operation) != entity_id {
                        return false;
                    }
                }

                // Filter by actor_id
                if let Some(ref actor_id) = query.actor_id {
                    if op.operation.actor.id != *actor_id {
                        return false;
                    }
                }

                // Filter by since (HLC timestamp)
                if let Some(ref since) = query.since {
                    if op.operation.timestamp.to_string() < *since {
                        return false;
                    }
                }

                // Filter by until (HLC timestamp)
                if let Some(ref until) = query.until {
                    if op.operation.timestamp.to_string() > *until {
                        return false;
                    }
                }

                // Filter by op_type
                if let Some(ref op_type) = query.op_type {
                    if op_type_str(&op.operation) != op_type {
                        return false;
                    }
                }

                true
            })
            .cloned()
            .collect();

        // Sort by HLC timestamp
        results.sort_by(|a, b| {
            a.operation
                .timestamp
                .to_string()
                .cmp(&b.operation.timestamp.to_string())
        });

        // Apply limit
        if let Some(limit) = query.limit {
            results.truncate(limit as usize);
        }

        Ok(results)
    }

    fn operation_count(&self, document_id: &str) -> Result<u64, StoreError> {
        let ops = self.operations.read();
        let count = ops.get(document_id).map(|v| v.len()).unwrap_or(0);
        Ok(count as u64)
    }

    fn operation_exists(&self, operation_id: &Uuid) -> Result<bool, StoreError> {
        let ops_by_id = self.operations_by_id.read();
        Ok(ops_by_id.contains_key(operation_id))
    }

    fn get_operations_by_actor_since(
        &self,
        document_id: &str,
        actor_id: &str,
        since: Option<&HlcTimestamp>,
    ) -> Result<Vec<StoredOperation>, StoreError> {
        let ops = self.operations.read();
        let doc_ops = match ops.get(document_id) {
            Some(ops) => ops,
            None => return Ok(Vec::new()),
        };

        let mut results: Vec<StoredOperation> = doc_ops
            .iter()
            .filter(|op| {
                // Filter by actor_id
                if op.operation.actor.id != actor_id {
                    return false;
                }

                // Filter by since timestamp (exclusive)
                if let Some(since_ts) = since {
                    if op.operation.timestamp.to_string() <= since_ts.to_string() {
                        return false;
                    }
                }

                true
            })
            .cloned()
            .collect();

        // Sort by HLC timestamp ascending
        results.sort_by(|a, b| {
            a.operation
                .timestamp
                .to_string()
                .cmp(&b.operation.timestamp.to_string())
        });

        Ok(results)
    }

    fn save_snapshot(
        &self,
        document_id: &str,
        hlc_timestamp: &HlcTimestamp,
        document: &Document,
    ) -> Result<Uuid, StoreError> {
        let id = Uuid::new_v4();
        let stored = StoredSnapshot {
            id,
            document_id: document_id.to_string(),
            hlc_timestamp: *hlc_timestamp,
            document: document.clone(),
            created_at: Utc::now().to_rfc3339(),
        };

        let mut snapshots = self.snapshots.write();
        snapshots
            .entry(document_id.to_string())
            .or_default()
            .push(stored);

        Ok(id)
    }

    fn latest_snapshot(&self, document_id: &str) -> Result<StoredSnapshot, StoreError> {
        let snapshots = self.snapshots.read();
        let doc_snapshots = snapshots
            .get(document_id)
            .ok_or_else(|| StoreError::NotFound(format!("snapshot for document {document_id}")))?;

        // Find the snapshot with the highest HLC timestamp
        doc_snapshots
            .iter()
            .max_by(|a, b| {
                a.hlc_timestamp
                    .to_string()
                    .cmp(&b.hlc_timestamp.to_string())
            })
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("snapshot for document {document_id}")))
    }

    fn operations_since_snapshot(&self, document_id: &str) -> Result<u64, StoreError> {
        let snapshot_ts = self
            .latest_snapshot(document_id)
            .ok()
            .map(|s| s.hlc_timestamp.to_string());

        let ops = self.operations.read();
        let doc_ops = match ops.get(document_id) {
            Some(ops) => ops,
            None => return Ok(0),
        };

        let count = match snapshot_ts {
            Some(ts) => doc_ops
                .iter()
                .filter(|op| op.operation.timestamp.to_string() > ts)
                .count(),
            None => doc_ops.len(),
        };

        Ok(count as u64)
    }

    fn record_milestone(
        &self,
        document_id: &str,
        git_commit: Option<&str>,
        hlc_range_start: &HlcTimestamp,
        hlc_range_end: &HlcTimestamp,
        message: Option<&str>,
    ) -> Result<Uuid, StoreError> {
        let id = Uuid::new_v4();
        let stored = StoredMilestone {
            id,
            document_id: document_id.to_string(),
            git_commit: git_commit.map(String::from),
            hlc_range_start: *hlc_range_start,
            hlc_range_end: *hlc_range_end,
            message: message.map(String::from),
            created_at: Utc::now().to_rfc3339(),
        };

        let mut milestones = self.milestones.write();
        milestones
            .entry(document_id.to_string())
            .or_default()
            .push(stored);

        Ok(id)
    }

    fn latest_milestone(&self, document_id: &str) -> Result<Option<StoredMilestone>, StoreError> {
        let milestones = self.milestones.read();
        let doc_milestones = match milestones.get(document_id) {
            Some(m) => m,
            None => return Ok(None),
        };

        // Return the last inserted (newest)
        Ok(doc_milestones.last().cloned())
    }

    fn list_milestones(&self, document_id: &str) -> Result<Vec<StoredMilestone>, StoreError> {
        let milestones = self.milestones.read();
        let doc_milestones = match milestones.get(document_id) {
            Some(m) => m,
            None => return Ok(Vec::new()),
        };

        // Return in reverse order (newest first)
        let mut results = doc_milestones.clone();
        results.reverse();
        Ok(results)
    }

    fn operations_since_milestone(&self, document_id: &str) -> Result<u64, StoreError> {
        let milestone_end = self
            .latest_milestone(document_id)?
            .map(|m| m.hlc_range_end.to_string());

        let ops = self.operations.read();
        let doc_ops = match ops.get(document_id) {
            Some(ops) => ops,
            None => return Ok(0),
        };

        let count = match milestone_end {
            Some(ts) => doc_ops
                .iter()
                .filter(|op| op.operation.timestamp.to_string() > ts)
                .count(),
            None => doc_ops.len(),
        };

        Ok(count as u64)
    }

    fn save_version_vector(
        &self,
        document_id: &str,
        version_vector_json: &str,
    ) -> Result<(), StoreError> {
        let mut vv = self.version_vectors.write();
        vv.insert(document_id.to_string(), version_vector_json.to_string());
        Ok(())
    }

    fn load_version_vector(&self, document_id: &str) -> Result<String, StoreError> {
        let vv = self.version_vectors.read();
        vv.get(document_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("version vector for document {document_id}")))
    }
}

/// Extracts the entity_id string from an operation's kind.
fn entity_id_from_op(op: &Operation) -> &str {
    match &op.kind {
        OperationKind::SetField { entity_id, .. }
        | OperationKind::InsertEntity { entity_id, .. }
        | OperationKind::RemoveEntity { entity_id }
        | OperationKind::MoveEntity { entity_id, .. }
        | OperationKind::SetOverride { entity_id, .. }
        | OperationKind::ResolveConflict { entity_id, .. } => entity_id.as_str(),
    }
}

/// Returns the operation type as a string for filtering.
fn op_type_str(op: &Operation) -> &'static str {
    match &op.kind {
        OperationKind::SetField { .. } => "set_field",
        OperationKind::InsertEntity { .. } => "insert_entity",
        OperationKind::RemoveEntity { .. } => "remove_entity",
        OperationKind::MoveEntity { .. } => "move_entity",
        OperationKind::SetOverride { .. } => "set_override",
        OperationKind::ResolveConflict { .. } => "resolve_conflict",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conflux_core::{ActorClass, ActorId, Clock, FieldValue};

    fn test_actor(name: &str) -> ActorId {
        ActorId::new(name, ActorClass::Human)
    }

    fn make_set_field_op(
        clock: &Clock,
        actor: &ActorId,
        entity_id: &str,
        field: &str,
        value: FieldValue,
    ) -> Operation {
        Operation::set_field(entity_id, field, value, actor, clock.new_timestamp())
    }

    fn make_insert_op(
        clock: &Clock,
        actor: &ActorId,
        entity_id: &str,
        entity_type: &str,
    ) -> Operation {
        Operation::insert_entity(
            entity_id,
            entity_type,
            None,
            None,
            actor,
            clock.new_timestamp(),
        )
    }

    #[test]
    fn append_and_get_operation() {
        let store = MemoryStore::new();
        let clock = Clock::new();
        let actor = test_actor("alice");

        let op = make_set_field_op(&clock, &actor, "route.api", "weight", FieldValue::Int(80));
        store.append_operation("doc-1", &op).unwrap();

        let stored = store.get_operation(&op.id).unwrap();
        assert_eq!(stored.operation, op);
        assert_eq!(stored.document_id, "doc-1");
    }

    #[test]
    fn query_operations_by_entity() {
        let store = MemoryStore::new();
        let clock = Clock::new();
        let actor = test_actor("alice");

        let op1 = make_set_field_op(&clock, &actor, "route.api", "weight", FieldValue::Int(80));
        let op2 = make_set_field_op(&clock, &actor, "route.web", "weight", FieldValue::Int(50));
        let op3 = make_set_field_op(
            &clock,
            &actor,
            "route.api",
            "timeout",
            FieldValue::Int(5000),
        );

        store.append_operation("doc-1", &op1).unwrap();
        store.append_operation("doc-1", &op2).unwrap();
        store.append_operation("doc-1", &op3).unwrap();

        let query = OperationQuery::new("doc-1").for_entity("route.api");
        let results = store.query_operations(&query).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].operation.id, op1.id);
        assert_eq!(results[1].operation.id, op3.id);
    }

    #[test]
    fn query_operations_by_actor() {
        let store = MemoryStore::new();
        let clock = Clock::new();
        let alice = test_actor("alice");
        let bob = test_actor("bob");

        let op1 = make_set_field_op(&clock, &alice, "route.api", "weight", FieldValue::Int(80));
        let op2 = make_set_field_op(&clock, &bob, "route.api", "timeout", FieldValue::Int(5000));
        let op3 = make_set_field_op(&clock, &alice, "route.web", "weight", FieldValue::Int(50));

        store.append_operation("doc-1", &op1).unwrap();
        store.append_operation("doc-1", &op2).unwrap();
        store.append_operation("doc-1", &op3).unwrap();

        let query = OperationQuery::new("doc-1").by_actor("bob");
        let results = store.query_operations(&query).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].operation.id, op2.id);
    }

    #[test]
    fn query_operations_by_time_range() {
        let store = MemoryStore::new();
        let clock = Clock::new();
        let actor = test_actor("alice");

        let op1 = make_set_field_op(&clock, &actor, "route.api", "weight", FieldValue::Int(1));
        let ts_after_1 = clock.new_timestamp();
        let op2 = make_set_field_op(&clock, &actor, "route.api", "weight", FieldValue::Int(2));
        let op3 = make_set_field_op(&clock, &actor, "route.api", "weight", FieldValue::Int(3));
        let ts_before_4 = clock.new_timestamp();
        let _op4 = make_set_field_op(&clock, &actor, "route.api", "weight", FieldValue::Int(4));

        store.append_operation("doc-1", &op1).unwrap();
        store.append_operation("doc-1", &op2).unwrap();
        store.append_operation("doc-1", &op3).unwrap();
        store.append_operation("doc-1", &_op4).unwrap();

        let query = OperationQuery::new("doc-1")
            .since(&ts_after_1)
            .until(&ts_before_4);
        let results = store.query_operations(&query).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].operation.id, op2.id);
        assert_eq!(results[1].operation.id, op3.id);
    }

    #[test]
    fn query_operations_with_limit() {
        let store = MemoryStore::new();
        let clock = Clock::new();
        let actor = test_actor("alice");

        for i in 0..10 {
            let op = make_set_field_op(&clock, &actor, "route.api", "weight", FieldValue::Int(i));
            store.append_operation("doc-1", &op).unwrap();
        }

        let query = OperationQuery::new("doc-1").limit(3);
        let results = store.query_operations(&query).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn operation_count() {
        let store = MemoryStore::new();
        let clock = Clock::new();
        let actor = test_actor("alice");

        assert_eq!(store.operation_count("doc-1").unwrap(), 0);

        for i in 0..5 {
            let op = make_set_field_op(&clock, &actor, "route.api", "weight", FieldValue::Int(i));
            store.append_operation("doc-1", &op).unwrap();
        }

        assert_eq!(store.operation_count("doc-1").unwrap(), 5);
    }

    #[test]
    fn snapshot_save_and_load() {
        let store = MemoryStore::new();
        let clock = Clock::new();
        let ts = clock.new_timestamp();
        let doc = Document::new();

        store.save_snapshot("doc-1", &ts, &doc).unwrap();

        let snapshot = store.latest_snapshot("doc-1").unwrap();
        assert_eq!(snapshot.document_id, "doc-1");
        assert_eq!(snapshot.hlc_timestamp, ts);
        assert_eq!(snapshot.document, doc);
    }

    #[test]
    fn operations_since_snapshot() {
        let store = MemoryStore::new();
        let clock = Clock::new();
        let actor = test_actor("alice");

        // Add 3 ops before snapshot
        for _ in 0..3 {
            let op = make_set_field_op(&clock, &actor, "route.api", "weight", FieldValue::Int(1));
            store.append_operation("doc-1", &op).unwrap();
        }

        // No snapshot yet — all 3 counted
        assert_eq!(store.operations_since_snapshot("doc-1").unwrap(), 3);

        // Take snapshot
        let snap_ts = clock.new_timestamp();
        store
            .save_snapshot("doc-1", &snap_ts, &Document::new())
            .unwrap();

        // No ops since snapshot
        assert_eq!(store.operations_since_snapshot("doc-1").unwrap(), 0);

        // Add 2 more ops after snapshot
        for _ in 0..2 {
            let op = make_set_field_op(&clock, &actor, "route.api", "weight", FieldValue::Int(2));
            store.append_operation("doc-1", &op).unwrap();
        }

        assert_eq!(store.operations_since_snapshot("doc-1").unwrap(), 2);
    }

    #[test]
    fn milestone_record_and_list() {
        let store = MemoryStore::new();
        let clock = Clock::new();

        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();
        let ts3 = clock.new_timestamp();
        let ts4 = clock.new_timestamp();

        store
            .record_milestone("doc-1", Some("abc123"), &ts1, &ts2, Some("first milestone"))
            .unwrap();
        store
            .record_milestone(
                "doc-1",
                Some("def456"),
                &ts3,
                &ts4,
                Some("second milestone"),
            )
            .unwrap();

        let latest = store.latest_milestone("doc-1").unwrap().unwrap();
        assert_eq!(latest.message.as_deref(), Some("second milestone"));
        assert_eq!(latest.git_commit.as_deref(), Some("def456"));

        let all = store.list_milestones("doc-1").unwrap();
        assert_eq!(all.len(), 2);
        // Newest first
        assert_eq!(all[0].message.as_deref(), Some("second milestone"));
        assert_eq!(all[1].message.as_deref(), Some("first milestone"));
    }

    #[test]
    fn operations_since_milestone() {
        let store = MemoryStore::new();
        let clock = Clock::new();
        let actor = test_actor("alice");

        // Add 3 ops
        let mut last_ts = clock.new_timestamp();
        for _ in 0..3 {
            let op = make_set_field_op(&clock, &actor, "route.api", "weight", FieldValue::Int(1));
            last_ts = op.timestamp;
            store.append_operation("doc-1", &op).unwrap();
        }

        // No milestone — all 3 counted
        assert_eq!(store.operations_since_milestone("doc-1").unwrap(), 3);

        // Record milestone covering all 3 ops
        let first_ts = clock.new_timestamp();
        store
            .record_milestone("doc-1", None, &first_ts, &last_ts, None)
            .unwrap();

        // 0 ops after milestone (last op timestamp == milestone range_end)
        assert_eq!(store.operations_since_milestone("doc-1").unwrap(), 0);

        // Add 2 more ops after milestone
        for _ in 0..2 {
            let op = make_set_field_op(&clock, &actor, "route.api", "weight", FieldValue::Int(2));
            store.append_operation("doc-1", &op).unwrap();
        }

        assert_eq!(store.operations_since_milestone("doc-1").unwrap(), 2);
    }

    #[test]
    fn multiple_documents_isolated() {
        let store = MemoryStore::new();
        let clock = Clock::new();
        let actor = test_actor("alice");

        // Add ops to doc-1
        for _ in 0..3 {
            let op = make_set_field_op(&clock, &actor, "route.api", "weight", FieldValue::Int(1));
            store.append_operation("doc-1", &op).unwrap();
        }

        // Add ops to doc-2
        for _ in 0..2 {
            let op = make_insert_op(&clock, &actor, "service.web", "service");
            store.append_operation("doc-2", &op).unwrap();
        }

        assert_eq!(store.operation_count("doc-1").unwrap(), 3);
        assert_eq!(store.operation_count("doc-2").unwrap(), 2);

        let query_1 = OperationQuery::new("doc-1");
        let results_1 = store.query_operations(&query_1).unwrap();
        assert_eq!(results_1.len(), 3);

        let query_2 = OperationQuery::new("doc-2");
        let results_2 = store.query_operations(&query_2).unwrap();
        assert_eq!(results_2.len(), 2);

        // Snapshot isolation
        let ts = clock.new_timestamp();
        store.save_snapshot("doc-1", &ts, &Document::new()).unwrap();
        assert!(store.latest_snapshot("doc-2").is_err());
    }

    #[test]
    fn version_vector_save_and_load() {
        let store = MemoryStore::new();

        let vv_json = r#"{"node-a":"2024-01-01T00:00:00.000000000Z/1","node-b":"2024-01-02T00:00:00.000000000Z/2"}"#;

        store.save_version_vector("doc-1", vv_json).unwrap();

        let loaded = store.load_version_vector("doc-1").unwrap();
        assert_eq!(loaded, vv_json);

        // Update it
        let vv_json2 = r#"{"node-a":"2024-01-03T00:00:00.000000000Z/3"}"#;
        store.save_version_vector("doc-1", vv_json2).unwrap();

        let loaded2 = store.load_version_vector("doc-1").unwrap();
        assert_eq!(loaded2, vv_json2);
    }

    #[test]
    fn version_vector_not_found() {
        let store = MemoryStore::new();
        let result = store.load_version_vector("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn get_operations_by_actor_since() {
        let store = MemoryStore::new();
        let clock = Clock::new();
        let alice = test_actor("alice");
        let bob = test_actor("bob");

        // Add ops from alice and bob
        let op1 = make_set_field_op(&clock, &alice, "route.api", "weight", FieldValue::Int(1));
        let ts_after_1 = clock.new_timestamp();
        let op2 = make_set_field_op(&clock, &bob, "route.api", "weight", FieldValue::Int(2));
        let op3 = make_set_field_op(&clock, &alice, "route.api", "weight", FieldValue::Int(3));

        store.append_operation("doc-1", &op1).unwrap();
        store.append_operation("doc-1", &op2).unwrap();
        store.append_operation("doc-1", &op3).unwrap();

        // Get all ops from alice
        let alice_ops = store
            .get_operations_by_actor_since("doc-1", "alice", None)
            .unwrap();
        assert_eq!(alice_ops.len(), 2);
        assert_eq!(alice_ops[0].operation.id, op1.id);
        assert_eq!(alice_ops[1].operation.id, op3.id);

        // Get ops from alice since ts_after_1
        let alice_ops_since = store
            .get_operations_by_actor_since("doc-1", "alice", Some(&ts_after_1))
            .unwrap();
        assert_eq!(alice_ops_since.len(), 1);
        assert_eq!(alice_ops_since[0].operation.id, op3.id);

        // Get ops from bob
        let bob_ops = store
            .get_operations_by_actor_since("doc-1", "bob", None)
            .unwrap();
        assert_eq!(bob_ops.len(), 1);
        assert_eq!(bob_ops[0].operation.id, op2.id);
    }

    #[test]
    fn operation_exists() {
        let store = MemoryStore::new();
        let clock = Clock::new();
        let actor = test_actor("alice");

        let op = make_set_field_op(&clock, &actor, "route.api", "weight", FieldValue::Int(1));

        // Doesn't exist yet
        assert!(!store.operation_exists(&op.id).unwrap());

        // Add it
        store.append_operation("doc-1", &op).unwrap();

        // Now exists
        assert!(store.operation_exists(&op.id).unwrap());
    }

    #[test]
    fn default_creates_empty_store() {
        let store = MemoryStore::default();
        assert_eq!(store.operation_count("any-doc").unwrap(), 0);
    }
}
