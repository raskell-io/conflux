//! PostgreSQL implementation of AsyncStore.

use crate::error::StoreError;
use crate::models::{StoredMilestone, StoredOperation, StoredSnapshot};
use crate::query::OperationQuery;
use crate::AsyncStore;
use async_trait::async_trait;
use conflux_core::{Document, HlcTimestamp, Operation, OperationKind};
use sqlx_core::executor::Executor;
use sqlx_core::pool::Pool;
use sqlx_core::query::query as sqlx_query;
use sqlx_core::row::Row;
use sqlx_postgres::{PgPoolOptions, PgRow, Postgres};
use uuid::Uuid;

type PgPool = Pool<Postgres>;

/// PostgreSQL-backed async store for operations, snapshots, and milestones.
///
/// Uses sqlx with connection pooling for efficient concurrent access.
pub struct PostgresStore {
    pool: PgPool,
}

impl PostgresStore {
    /// Connects to PostgreSQL with default pool settings.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Postgres` if the connection fails.
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new().connect(url).await?;
        Ok(Self { pool })
    }

    /// Connects to PostgreSQL with a custom maximum connection pool size.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Postgres` if the connection fails.
    pub async fn connect_with_pool_size(url: &str, max_connections: u32) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(url)
            .await?;
        Ok(Self { pool })
    }

    /// Creates an instance from an existing connection pool.
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns a reference to the underlying connection pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Creates the required tables and indexes if they don't exist.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Postgres` if DDL execution fails.
    pub async fn create_tables(&self) -> Result<(), StoreError> {
        self.pool
            .execute(include_str!("schema.sql"))
            .await?;
        Ok(())
    }
}

/// Helper to extract a row value with proper type inference.
fn get_value<'r, T>(row: &'r PgRow, col: &str) -> T
where
    T: sqlx_core::decode::Decode<'r, Postgres> + sqlx_core::types::Type<Postgres>,
{
    row.get(col)
}

#[async_trait]
impl AsyncStore for PostgresStore {
    async fn append_operation(
        &self,
        document_id: &str,
        operation: &Operation,
    ) -> Result<(), StoreError> {
        let payload = serde_json::to_value(operation)?;
        let entity_id = entity_id_from_op(operation);
        let op_type = op_type_str(operation);

        sqlx_query(
            r#"
            INSERT INTO operations (id, document_id, hlc_timestamp, actor_id, actor_class, op_type, entity_id, payload, intent)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(operation.id)
        .bind(document_id)
        .bind(operation.timestamp.to_string())
        .bind(&operation.actor.id)
        .bind(operation.actor.class.to_string())
        .bind(op_type)
        .bind(entity_id)
        .bind(&payload)
        .bind(&operation.intent)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_operation(&self, operation_id: &Uuid) -> Result<StoredOperation, StoreError> {
        let row: Option<PgRow> = sqlx_query(
            r#"
            SELECT payload, document_id, created_at
            FROM operations
            WHERE id = $1
            "#,
        )
        .bind(operation_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let payload: serde_json::Value = get_value(&row, "payload");
                let document_id: String = get_value(&row, "document_id");
                let created_at: chrono::DateTime<chrono::Utc> = get_value(&row, "created_at");

                let operation: Operation = serde_json::from_value(payload)?;
                Ok(StoredOperation {
                    operation,
                    document_id,
                    created_at: created_at.to_rfc3339(),
                })
            }
            None => Err(StoreError::NotFound(format!("operation {operation_id}"))),
        }
    }

    async fn query_operations(
        &self,
        query: &OperationQuery,
    ) -> Result<Vec<StoredOperation>, StoreError> {
        let mut sql = String::from(
            "SELECT payload, document_id, created_at FROM operations WHERE document_id = $1",
        );
        let mut param_idx = 2u32;

        // Build dynamic WHERE clauses
        let entity_id_idx = if query.entity_id.is_some() {
            sql.push_str(&format!(" AND entity_id = ${param_idx}"));
            let idx = param_idx;
            param_idx += 1;
            Some(idx)
        } else {
            None
        };

        let actor_id_idx = if query.actor_id.is_some() {
            sql.push_str(&format!(" AND actor_id = ${param_idx}"));
            let idx = param_idx;
            param_idx += 1;
            Some(idx)
        } else {
            None
        };

        let since_idx = if query.since.is_some() {
            sql.push_str(&format!(" AND hlc_timestamp >= ${param_idx}"));
            let idx = param_idx;
            param_idx += 1;
            Some(idx)
        } else {
            None
        };

        let until_idx = if query.until.is_some() {
            sql.push_str(&format!(" AND hlc_timestamp <= ${param_idx}"));
            let idx = param_idx;
            param_idx += 1;
            Some(idx)
        } else {
            None
        };

        let op_type_idx = if query.op_type.is_some() {
            sql.push_str(&format!(" AND op_type = ${param_idx}"));
            let idx = param_idx;
            param_idx += 1;
            Some(idx)
        } else {
            None
        };

        sql.push_str(" ORDER BY hlc_timestamp ASC");

        if query.limit.is_some() {
            sql.push_str(&format!(" LIMIT ${param_idx}"));
            let _ = param_idx; // silence warning
        }

        // Build and execute query with dynamic bindings
        let mut sqlx_query = sqlx_query(&sql).bind(&query.document_id);

        if let Some(ref entity_id) = query.entity_id {
            let _ = entity_id_idx;
            sqlx_query = sqlx_query.bind(entity_id);
        }
        if let Some(ref actor_id) = query.actor_id {
            let _ = actor_id_idx;
            sqlx_query = sqlx_query.bind(actor_id);
        }
        if let Some(ref since) = query.since {
            let _ = since_idx;
            sqlx_query = sqlx_query.bind(since);
        }
        if let Some(ref until) = query.until {
            let _ = until_idx;
            sqlx_query = sqlx_query.bind(until);
        }
        if let Some(ref op_type) = query.op_type {
            let _ = op_type_idx;
            sqlx_query = sqlx_query.bind(op_type);
        }
        if let Some(limit) = query.limit {
            sqlx_query = sqlx_query.bind(limit as i64);
        }

        let rows: Vec<PgRow> = sqlx_query.fetch_all(&self.pool).await?;

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let payload: serde_json::Value = get_value(&row, "payload");
            let document_id: String = get_value(&row, "document_id");
            let created_at: chrono::DateTime<chrono::Utc> = get_value(&row, "created_at");

            let operation: Operation = serde_json::from_value(payload)?;
            results.push(StoredOperation {
                operation,
                document_id,
                created_at: created_at.to_rfc3339(),
            });
        }

        Ok(results)
    }

    async fn operation_count(&self, document_id: &str) -> Result<u64, StoreError> {
        let row: PgRow = sqlx_query(
            "SELECT COUNT(*) as count FROM operations WHERE document_id = $1",
        )
        .bind(document_id)
        .fetch_one(&self.pool)
        .await?;

        let count: i64 = get_value(&row, "count");
        Ok(count as u64)
    }

    async fn operation_exists(&self, operation_id: &Uuid) -> Result<bool, StoreError> {
        let row: PgRow = sqlx_query(
            "SELECT COUNT(*) as count FROM operations WHERE id = $1",
        )
        .bind(operation_id)
        .fetch_one(&self.pool)
        .await?;

        let count: i64 = get_value(&row, "count");
        Ok(count > 0)
    }

    async fn get_operations_by_actor_since(
        &self,
        document_id: &str,
        actor_id: &str,
        since: Option<&HlcTimestamp>,
    ) -> Result<Vec<StoredOperation>, StoreError> {
        let rows: Vec<PgRow> = match since {
            Some(ts) => {
                sqlx_query(
                    r#"
                    SELECT payload, document_id, created_at
                    FROM operations
                    WHERE document_id = $1 AND actor_id = $2 AND hlc_timestamp > $3
                    ORDER BY hlc_timestamp ASC
                    "#,
                )
                .bind(document_id)
                .bind(actor_id)
                .bind(ts.to_string())
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx_query(
                    r#"
                    SELECT payload, document_id, created_at
                    FROM operations
                    WHERE document_id = $1 AND actor_id = $2
                    ORDER BY hlc_timestamp ASC
                    "#,
                )
                .bind(document_id)
                .bind(actor_id)
                .fetch_all(&self.pool)
                .await?
            }
        };

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let payload: serde_json::Value = get_value(&row, "payload");
            let document_id: String = get_value(&row, "document_id");
            let created_at: chrono::DateTime<chrono::Utc> = get_value(&row, "created_at");

            let operation: Operation = serde_json::from_value(payload)?;
            results.push(StoredOperation {
                operation,
                document_id,
                created_at: created_at.to_rfc3339(),
            });
        }

        Ok(results)
    }

    async fn save_snapshot(
        &self,
        document_id: &str,
        hlc_timestamp: &HlcTimestamp,
        document: &Document,
    ) -> Result<Uuid, StoreError> {
        let id = Uuid::new_v4();
        let data = serde_json::to_value(document)?;

        sqlx_query(
            r#"
            INSERT INTO snapshots (id, document_id, hlc_timestamp, data)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(id)
        .bind(document_id)
        .bind(hlc_timestamp.to_string())
        .bind(&data)
        .execute(&self.pool)
        .await?;

        Ok(id)
    }

    async fn latest_snapshot(&self, document_id: &str) -> Result<StoredSnapshot, StoreError> {
        let row: Option<PgRow> = sqlx_query(
            r#"
            SELECT id, hlc_timestamp, data, created_at
            FROM snapshots
            WHERE document_id = $1
            ORDER BY hlc_timestamp DESC
            LIMIT 1
            "#,
        )
        .bind(document_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let id: Uuid = get_value(&row, "id");
                let hlc_timestamp_str: String = get_value(&row, "hlc_timestamp");
                let data: serde_json::Value = get_value(&row, "data");
                let created_at: chrono::DateTime<chrono::Utc> = get_value(&row, "created_at");

                let hlc_timestamp: HlcTimestamp =
                    serde_json::from_value(serde_json::Value::String(hlc_timestamp_str))?;
                let document: Document = serde_json::from_value(data)?;

                Ok(StoredSnapshot {
                    id,
                    document_id: document_id.to_string(),
                    hlc_timestamp,
                    document,
                    created_at: created_at.to_rfc3339(),
                })
            }
            None => Err(StoreError::NotFound(format!(
                "snapshot for document {document_id}"
            ))),
        }
    }

    async fn operations_since_snapshot(&self, document_id: &str) -> Result<u64, StoreError> {
        // Get latest snapshot timestamp
        let snapshot_row: Option<PgRow> = sqlx_query(
            r#"
            SELECT hlc_timestamp
            FROM snapshots
            WHERE document_id = $1
            ORDER BY hlc_timestamp DESC
            LIMIT 1
            "#,
        )
        .bind(document_id)
        .fetch_optional(&self.pool)
        .await?;

        let count: i64 = match snapshot_row {
            Some(row) => {
                let ts: String = get_value(&row, "hlc_timestamp");
                let count_row: PgRow = sqlx_query(
                    r#"
                    SELECT COUNT(*) as count
                    FROM operations
                    WHERE document_id = $1 AND hlc_timestamp > $2
                    "#,
                )
                .bind(document_id)
                .bind(ts)
                .fetch_one(&self.pool)
                .await?;
                get_value(&count_row, "count")
            }
            None => {
                let count_row: PgRow = sqlx_query(
                    "SELECT COUNT(*) as count FROM operations WHERE document_id = $1",
                )
                .bind(document_id)
                .fetch_one(&self.pool)
                .await?;
                get_value(&count_row, "count")
            }
        };

        Ok(count as u64)
    }

    async fn record_milestone(
        &self,
        document_id: &str,
        git_commit: Option<&str>,
        hlc_range_start: &HlcTimestamp,
        hlc_range_end: &HlcTimestamp,
        message: Option<&str>,
    ) -> Result<Uuid, StoreError> {
        let id = Uuid::new_v4();

        sqlx_query(
            r#"
            INSERT INTO milestones (id, document_id, git_commit, hlc_range_start, hlc_range_end, message)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(id)
        .bind(document_id)
        .bind(git_commit)
        .bind(hlc_range_start.to_string())
        .bind(hlc_range_end.to_string())
        .bind(message)
        .execute(&self.pool)
        .await?;

        Ok(id)
    }

    async fn latest_milestone(
        &self,
        document_id: &str,
    ) -> Result<Option<StoredMilestone>, StoreError> {
        let row: Option<PgRow> = sqlx_query(
            r#"
            SELECT id, git_commit, hlc_range_start, hlc_range_end, message, created_at
            FROM milestones
            WHERE document_id = $1
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(document_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let id: Uuid = get_value(&row, "id");
                let git_commit: Option<String> = get_value(&row, "git_commit");
                let hlc_range_start_str: String = get_value(&row, "hlc_range_start");
                let hlc_range_end_str: String = get_value(&row, "hlc_range_end");
                let message: Option<String> = get_value(&row, "message");
                let created_at: chrono::DateTime<chrono::Utc> = get_value(&row, "created_at");

                let hlc_range_start: HlcTimestamp =
                    serde_json::from_value(serde_json::Value::String(hlc_range_start_str))?;
                let hlc_range_end: HlcTimestamp =
                    serde_json::from_value(serde_json::Value::String(hlc_range_end_str))?;

                Ok(Some(StoredMilestone {
                    id,
                    document_id: document_id.to_string(),
                    git_commit,
                    hlc_range_start,
                    hlc_range_end,
                    message,
                    created_at: created_at.to_rfc3339(),
                }))
            }
            None => Ok(None),
        }
    }

    async fn list_milestones(&self, document_id: &str) -> Result<Vec<StoredMilestone>, StoreError> {
        let rows: Vec<PgRow> = sqlx_query(
            r#"
            SELECT id, git_commit, hlc_range_start, hlc_range_end, message, created_at
            FROM milestones
            WHERE document_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(document_id)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let id: Uuid = get_value(&row, "id");
            let git_commit: Option<String> = get_value(&row, "git_commit");
            let hlc_range_start_str: String = get_value(&row, "hlc_range_start");
            let hlc_range_end_str: String = get_value(&row, "hlc_range_end");
            let message: Option<String> = get_value(&row, "message");
            let created_at: chrono::DateTime<chrono::Utc> = get_value(&row, "created_at");

            let hlc_range_start: HlcTimestamp =
                serde_json::from_value(serde_json::Value::String(hlc_range_start_str))?;
            let hlc_range_end: HlcTimestamp =
                serde_json::from_value(serde_json::Value::String(hlc_range_end_str))?;

            results.push(StoredMilestone {
                id,
                document_id: document_id.to_string(),
                git_commit,
                hlc_range_start,
                hlc_range_end,
                message,
                created_at: created_at.to_rfc3339(),
            });
        }

        Ok(results)
    }

    async fn operations_since_milestone(&self, document_id: &str) -> Result<u64, StoreError> {
        // Get latest milestone's hlc_range_end
        let milestone_row: Option<PgRow> = sqlx_query(
            r#"
            SELECT hlc_range_end
            FROM milestones
            WHERE document_id = $1
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(document_id)
        .fetch_optional(&self.pool)
        .await?;

        let count: i64 = match milestone_row {
            Some(row) => {
                let ts: String = get_value(&row, "hlc_range_end");
                let count_row: PgRow = sqlx_query(
                    r#"
                    SELECT COUNT(*) as count
                    FROM operations
                    WHERE document_id = $1 AND hlc_timestamp > $2
                    "#,
                )
                .bind(document_id)
                .bind(ts)
                .fetch_one(&self.pool)
                .await?;
                get_value(&count_row, "count")
            }
            None => {
                let count_row: PgRow = sqlx_query(
                    "SELECT COUNT(*) as count FROM operations WHERE document_id = $1",
                )
                .bind(document_id)
                .fetch_one(&self.pool)
                .await?;
                get_value(&count_row, "count")
            }
        };

        Ok(count as u64)
    }

    async fn save_version_vector(
        &self,
        document_id: &str,
        version_vector_json: &str,
    ) -> Result<(), StoreError> {
        let data: serde_json::Value = serde_json::from_str(version_vector_json)?;

        sqlx_query(
            r#"
            INSERT INTO version_vectors (document_id, data)
            VALUES ($1, $2)
            ON CONFLICT (document_id) DO UPDATE SET
                data = EXCLUDED.data,
                updated_at = NOW()
            "#,
        )
        .bind(document_id)
        .bind(&data)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn load_version_vector(&self, document_id: &str) -> Result<String, StoreError> {
        let row: Option<PgRow> = sqlx_query(
            "SELECT data FROM version_vectors WHERE document_id = $1",
        )
        .bind(document_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let data: serde_json::Value = get_value(&row, "data");
                Ok(serde_json::to_string(&data)?)
            }
            None => Err(StoreError::NotFound(format!(
                "version vector for document {document_id}"
            ))),
        }
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
    // Integration tests require a real PostgreSQL instance.
    // Use testcontainers for CI or run against a local instance.
    //
    // Example test setup:
    //
    // #[tokio::test]
    // async fn postgres_append_and_get() {
    //     let store = PostgresStore::connect("postgres://localhost/conflux_test").await.unwrap();
    //     store.create_tables().await.unwrap();
    //     // ... test logic
    // }
}
