//! SQLite-backed persistent storage for Conflux.

use crate::error::StoreError;
use crate::models::{StoredMilestone, StoredOperation, StoredSnapshot};
use crate::query::OperationQuery;
use conflux_core::{Document, HlcTimestamp, Operation, OperationKind};
use rusqlite::{params, Connection};
use uuid::Uuid;

/// SQLite-backed store for operations, snapshots, and milestones.
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Opens a store backed by a file at the given path.
    ///
    /// Creates tables and indexes if they don't exist.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Database` if the connection or DDL fails.
    pub fn open(path: &str) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.create_tables()?;
        Ok(store)
    }

    /// Opens an in-memory store for testing.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Database` if the DDL fails.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.create_tables()?;
        Ok(store)
    }

    fn create_tables(&self) -> Result<(), StoreError> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS operations (
                id TEXT PRIMARY KEY,
                document_id TEXT NOT NULL,
                hlc_timestamp TEXT NOT NULL,
                actor_id TEXT NOT NULL,
                actor_class TEXT NOT NULL,
                op_type TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                payload TEXT NOT NULL,
                intent TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_operations_doc_hlc
                ON operations (document_id, hlc_timestamp);
            CREATE INDEX IF NOT EXISTS idx_operations_entity
                ON operations (entity_id);
            CREATE INDEX IF NOT EXISTS idx_operations_actor
                ON operations (actor_id);
            CREATE INDEX IF NOT EXISTS idx_operations_doc_created
                ON operations (document_id, created_at DESC);

            CREATE TABLE IF NOT EXISTS snapshots (
                id TEXT PRIMARY KEY,
                document_id TEXT NOT NULL,
                hlc_timestamp TEXT NOT NULL,
                data TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_snapshots_doc_hlc
                ON snapshots (document_id, hlc_timestamp);

            CREATE TABLE IF NOT EXISTS milestones (
                id TEXT PRIMARY KEY,
                document_id TEXT NOT NULL,
                git_commit TEXT,
                hlc_range_start TEXT NOT NULL,
                hlc_range_end TEXT NOT NULL,
                message TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_milestones_doc_created
                ON milestones (document_id, created_at DESC);

            CREATE TABLE IF NOT EXISTS version_vectors (
                document_id TEXT PRIMARY KEY,
                data TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            ",
        )?;
        Ok(())
    }

    // --- Operations ---

    /// Appends an operation to the log for a given document.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Serialization` if the operation can't be serialized,
    /// or `StoreError::Database` on insert failure.
    pub fn append_operation(
        &self,
        document_id: &str,
        operation: &Operation,
    ) -> Result<(), StoreError> {
        let payload = serde_json::to_string(operation)?;
        let entity_id = entity_id_from_op(operation);
        let op_type = op_type_str(operation);

        self.conn.execute(
            "INSERT INTO operations (id, document_id, hlc_timestamp, actor_id, actor_class, op_type, entity_id, payload, intent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                operation.id.to_string(),
                document_id,
                operation.timestamp.to_string(),
                operation.actor.id,
                operation.actor.class.to_string(),
                op_type,
                entity_id,
                payload,
                operation.intent,
            ],
        )?;
        Ok(())
    }

    /// Retrieves a single operation by ID.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::NotFound` if the operation doesn't exist.
    pub fn get_operation(&self, operation_id: &Uuid) -> Result<StoredOperation, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT payload, document_id, created_at FROM operations WHERE id = ?1")?;

        let result = stmt.query_row(params![operation_id.to_string()], |row| {
            let payload: String = row.get(0)?;
            let document_id: String = row.get(1)?;
            let created_at: String = row.get(2)?;
            Ok((payload, document_id, created_at))
        });

        match result {
            Ok((payload, document_id, created_at)) => {
                let operation: Operation = serde_json::from_str(&payload)?;
                Ok(StoredOperation {
                    operation,
                    document_id,
                    created_at,
                })
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                Err(StoreError::NotFound(format!("operation {operation_id}")))
            }
            Err(e) => Err(StoreError::Database(e)),
        }
    }

    /// Queries operations using a fluent query builder.
    ///
    /// Results are ordered by HLC timestamp ascending.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Database` on query failure or
    /// `StoreError::Serialization` if stored payloads are corrupt.
    pub fn query_operations(
        &self,
        query: &OperationQuery,
    ) -> Result<Vec<StoredOperation>, StoreError> {
        let (where_clause, params) = query.to_sql();

        let limit_clause = match query.limit {
            Some(n) => format!(" LIMIT {n}"),
            None => String::new(),
        };

        let sql = format!(
            "SELECT payload, document_id, created_at FROM operations {where_clause} ORDER BY hlc_timestamp ASC{limit_clause}"
        );

        let mut stmt = self.conn.prepare(&sql)?;

        let param_refs: Vec<(&str, &dyn rusqlite::types::ToSql)> = params
            .iter()
            .map(|(name, val)| (name.as_str(), val as &dyn rusqlite::types::ToSql))
            .collect();

        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            let payload: String = row.get(0)?;
            let document_id: String = row.get(1)?;
            let created_at: String = row.get(2)?;
            Ok((payload, document_id, created_at))
        })?;

        let mut results = Vec::new();
        for row in rows {
            let (payload, document_id, created_at) = row?;
            let operation: Operation = serde_json::from_str(&payload)?;
            results.push(StoredOperation {
                operation,
                document_id,
                created_at,
            });
        }
        Ok(results)
    }

    /// Returns the number of operations for a document.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Database` on query failure.
    pub fn operation_count(&self, document_id: &str) -> Result<u64, StoreError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM operations WHERE document_id = ?1",
            params![document_id],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }

    // --- Snapshots ---

    /// Saves a document snapshot.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Serialization` if the document can't be serialized,
    /// or `StoreError::Database` on insert failure.
    pub fn save_snapshot(
        &self,
        document_id: &str,
        hlc_timestamp: &HlcTimestamp,
        document: &Document,
    ) -> Result<Uuid, StoreError> {
        let id = Uuid::new_v4();
        let data = serde_json::to_string(document)?;

        self.conn.execute(
            "INSERT INTO snapshots (id, document_id, hlc_timestamp, data)
             VALUES (?1, ?2, ?3, ?4)",
            params![id.to_string(), document_id, hlc_timestamp.to_string(), data,],
        )?;
        Ok(id)
    }

    /// Returns the latest snapshot for a document.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::NotFound` if no snapshots exist for the document.
    pub fn latest_snapshot(&self, document_id: &str) -> Result<StoredSnapshot, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, hlc_timestamp, data, created_at FROM snapshots
             WHERE document_id = ?1
             ORDER BY hlc_timestamp DESC LIMIT 1",
        )?;

        let result = stmt.query_row(params![document_id], |row| {
            let id: String = row.get(0)?;
            let hlc_timestamp: String = row.get(1)?;
            let data: String = row.get(2)?;
            let created_at: String = row.get(3)?;
            Ok((id, hlc_timestamp, data, created_at))
        });

        match result {
            Ok((id, hlc_timestamp, data, created_at)) => {
                let id: Uuid = id
                    .parse()
                    .map_err(|e| StoreError::Integrity(format!("invalid snapshot UUID: {e}")))?;
                let hlc_timestamp: HlcTimestamp =
                    serde_json::from_value(serde_json::Value::String(hlc_timestamp))?;
                let document: Document = serde_json::from_str(&data)?;
                Ok(StoredSnapshot {
                    id,
                    document_id: document_id.to_string(),
                    hlc_timestamp,
                    document,
                    created_at,
                })
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(StoreError::NotFound(format!(
                "snapshot for document {document_id}"
            ))),
            Err(e) => Err(StoreError::Database(e)),
        }
    }

    /// Returns the number of operations after the latest snapshot for a document.
    ///
    /// If no snapshots exist, returns the total operation count.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Database` on query failure.
    pub fn operations_since_snapshot(&self, document_id: &str) -> Result<u64, StoreError> {
        let snapshot_ts: Option<String> = self
            .conn
            .query_row(
                "SELECT hlc_timestamp FROM snapshots
                 WHERE document_id = ?1
                 ORDER BY hlc_timestamp DESC LIMIT 1",
                params![document_id],
                |row| row.get(0),
            )
            .ok();

        let count: i64 = match snapshot_ts {
            Some(ts) => self.conn.query_row(
                "SELECT COUNT(*) FROM operations
                 WHERE document_id = ?1 AND hlc_timestamp > ?2",
                params![document_id, ts],
                |row| row.get(0),
            )?,
            None => self.conn.query_row(
                "SELECT COUNT(*) FROM operations WHERE document_id = ?1",
                params![document_id],
                |row| row.get(0),
            )?,
        };
        Ok(count as u64)
    }

    // --- Milestones ---

    /// Records a milestone for a document.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Database` on insert failure.
    pub fn record_milestone(
        &self,
        document_id: &str,
        git_commit: Option<&str>,
        hlc_range_start: &HlcTimestamp,
        hlc_range_end: &HlcTimestamp,
        message: Option<&str>,
    ) -> Result<Uuid, StoreError> {
        let id = Uuid::new_v4();

        self.conn.execute(
            "INSERT INTO milestones (id, document_id, git_commit, hlc_range_start, hlc_range_end, message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id.to_string(),
                document_id,
                git_commit,
                hlc_range_start.to_string(),
                hlc_range_end.to_string(),
                message,
            ],
        )?;
        Ok(id)
    }

    /// Returns the latest milestone for a document.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::NotFound` if no milestones exist.
    pub fn latest_milestone(
        &self,
        document_id: &str,
    ) -> Result<Option<StoredMilestone>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, git_commit, hlc_range_start, hlc_range_end, message, created_at
             FROM milestones
             WHERE document_id = ?1
             ORDER BY rowid DESC LIMIT 1",
        )?;

        let result = stmt.query_row(params![document_id], |row| {
            let id: String = row.get(0)?;
            let git_commit: Option<String> = row.get(1)?;
            let hlc_range_start: String = row.get(2)?;
            let hlc_range_end: String = row.get(3)?;
            let message: Option<String> = row.get(4)?;
            let created_at: String = row.get(5)?;
            Ok((
                id,
                git_commit,
                hlc_range_start,
                hlc_range_end,
                message,
                created_at,
            ))
        });

        match result {
            Ok((id, git_commit, hlc_range_start, hlc_range_end, message, created_at)) => {
                let id: Uuid = id
                    .parse()
                    .map_err(|e| StoreError::Integrity(format!("invalid milestone UUID: {e}")))?;
                let hlc_range_start: HlcTimestamp =
                    serde_json::from_value(serde_json::Value::String(hlc_range_start))?;
                let hlc_range_end: HlcTimestamp =
                    serde_json::from_value(serde_json::Value::String(hlc_range_end))?;
                Ok(Some(StoredMilestone {
                    id,
                    document_id: document_id.to_string(),
                    git_commit,
                    hlc_range_start,
                    hlc_range_end,
                    message,
                    created_at,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StoreError::Database(e)),
        }
    }

    /// Lists all milestones for a document, newest first.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Database` on query failure.
    pub fn list_milestones(&self, document_id: &str) -> Result<Vec<StoredMilestone>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, git_commit, hlc_range_start, hlc_range_end, message, created_at
             FROM milestones
             WHERE document_id = ?1
             ORDER BY rowid DESC",
        )?;

        let rows = stmt.query_map(params![document_id], |row| {
            let id: String = row.get(0)?;
            let git_commit: Option<String> = row.get(1)?;
            let hlc_range_start: String = row.get(2)?;
            let hlc_range_end: String = row.get(3)?;
            let message: Option<String> = row.get(4)?;
            let created_at: String = row.get(5)?;
            Ok((
                id,
                git_commit,
                hlc_range_start,
                hlc_range_end,
                message,
                created_at,
            ))
        })?;

        let mut results = Vec::new();
        for row in rows {
            let (id, git_commit, hlc_range_start, hlc_range_end, message, created_at) = row?;
            let id: Uuid = id
                .parse()
                .map_err(|e| StoreError::Integrity(format!("invalid milestone UUID: {e}")))?;
            let hlc_range_start: HlcTimestamp =
                serde_json::from_value(serde_json::Value::String(hlc_range_start))?;
            let hlc_range_end: HlcTimestamp =
                serde_json::from_value(serde_json::Value::String(hlc_range_end))?;
            results.push(StoredMilestone {
                id,
                document_id: document_id.to_string(),
                git_commit,
                hlc_range_start,
                hlc_range_end,
                message,
                created_at,
            });
        }
        Ok(results)
    }

    /// Returns the number of operations after the latest milestone for a document.
    ///
    /// If no milestones exist, returns the total operation count.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Database` on query failure.
    pub fn operations_since_milestone(&self, document_id: &str) -> Result<u64, StoreError> {
        let milestone_end: Option<String> = self
            .conn
            .query_row(
                "SELECT hlc_range_end FROM milestones
                 WHERE document_id = ?1
                 ORDER BY rowid DESC LIMIT 1",
                params![document_id],
                |row| row.get(0),
            )
            .ok();

        let count: i64 = match milestone_end {
            Some(ts) => self.conn.query_row(
                "SELECT COUNT(*) FROM operations
                 WHERE document_id = ?1 AND hlc_timestamp > ?2",
                params![document_id, ts],
                |row| row.get(0),
            )?,
            None => self.conn.query_row(
                "SELECT COUNT(*) FROM operations WHERE document_id = ?1",
                params![document_id],
                |row| row.get(0),
            )?,
        };
        Ok(count as u64)
    }

    // --- Version Vectors ---

    /// Saves a version vector for a document.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Serialization` if the version vector can't be serialized,
    /// or `StoreError::Database` on upsert failure.
    pub fn save_version_vector(
        &self,
        document_id: &str,
        version_vector_json: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO version_vectors (document_id, data)
             VALUES (?1, ?2)
             ON CONFLICT(document_id) DO UPDATE SET
                data = excluded.data,
                updated_at = datetime('now')",
            params![document_id, version_vector_json],
        )?;
        Ok(())
    }

    /// Loads a version vector for a document.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::NotFound` if no version vector exists for the document.
    pub fn load_version_vector(&self, document_id: &str) -> Result<String, StoreError> {
        let result = self.conn.query_row(
            "SELECT data FROM version_vectors WHERE document_id = ?1",
            params![document_id],
            |row| row.get::<_, String>(0),
        );

        match result {
            Ok(data) => Ok(data),
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(StoreError::NotFound(format!(
                "version vector for document {document_id}"
            ))),
            Err(e) => Err(StoreError::Database(e)),
        }
    }

    /// Queries operations by actor (node) since a given timestamp.
    ///
    /// This is useful for anti-entropy sync to find operations from a specific node
    /// that another node might be missing.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Database` on query failure.
    pub fn get_operations_by_actor_since(
        &self,
        document_id: &str,
        actor_id: &str,
        since: Option<&HlcTimestamp>,
    ) -> Result<Vec<StoredOperation>, StoreError> {
        let (sql, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = match since {
            Some(ts) => (
                "SELECT payload, document_id, created_at FROM operations
                 WHERE document_id = ?1 AND actor_id = ?2 AND hlc_timestamp > ?3
                 ORDER BY hlc_timestamp ASC".to_string(),
                vec![
                    Box::new(document_id.to_string()),
                    Box::new(actor_id.to_string()),
                    Box::new(ts.to_string()),
                ],
            ),
            None => (
                "SELECT payload, document_id, created_at FROM operations
                 WHERE document_id = ?1 AND actor_id = ?2
                 ORDER BY hlc_timestamp ASC".to_string(),
                vec![
                    Box::new(document_id.to_string()),
                    Box::new(actor_id.to_string()),
                ],
            ),
        };

        let mut stmt = self.conn.prepare(&sql)?;

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();

        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            let payload: String = row.get(0)?;
            let document_id: String = row.get(1)?;
            let created_at: String = row.get(2)?;
            Ok((payload, document_id, created_at))
        })?;

        let mut results = Vec::new();
        for row in rows {
            let (payload, document_id, created_at) = row?;
            let operation: Operation = serde_json::from_str(&payload)?;
            results.push(StoredOperation {
                operation,
                document_id,
                created_at,
            });
        }
        Ok(results)
    }

    /// Checks if an operation with the given ID already exists.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Database` on query failure.
    pub fn operation_exists(&self, operation_id: &Uuid) -> Result<bool, StoreError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM operations WHERE id = ?1",
            params![operation_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(count > 0)
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

/// Returns the operation type as a string for the op_type column.
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
        let store = SqliteStore::open_in_memory().unwrap();
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
        let store = SqliteStore::open_in_memory().unwrap();
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
        let store = SqliteStore::open_in_memory().unwrap();
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
        let store = SqliteStore::open_in_memory().unwrap();
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
        let store = SqliteStore::open_in_memory().unwrap();
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
        let store = SqliteStore::open_in_memory().unwrap();
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
        let store = SqliteStore::open_in_memory().unwrap();
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
        let store = SqliteStore::open_in_memory().unwrap();
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
        let store = SqliteStore::open_in_memory().unwrap();
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
        let store = SqliteStore::open_in_memory().unwrap();
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
        let first_ts = clock.new_timestamp(); // Dummy — we just need range_end > all ops
        store
            .record_milestone("doc-1", None, &first_ts, &last_ts, None)
            .unwrap();

        // ops_since_milestone uses hlc_range_end — but our milestone range_end == last_ts
        // which is the timestamp of the last op, and we check > (strict),
        // so 0 ops are after the milestone
        // Actually we need to be careful: the milestone range_end is last_ts which is
        // the timestamp of the 3rd op. The query is hlc_timestamp > range_end,
        // so the 3rd op's timestamp is NOT > last_ts (it equals it). Result: 0.
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
        let store = SqliteStore::open_in_memory().unwrap();
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
        let store = SqliteStore::open_in_memory().unwrap();

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
        let store = SqliteStore::open_in_memory().unwrap();
        let result = store.load_version_vector("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn get_operations_by_actor_since() {
        let store = SqliteStore::open_in_memory().unwrap();
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
        let store = SqliteStore::open_in_memory().unwrap();
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
}
