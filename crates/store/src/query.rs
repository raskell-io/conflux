//! Fluent query builder for filtering operations.

use conflux_core::HlcTimestamp;

/// Fluent builder for querying the operation log.
///
/// # Example
///
/// ```ignore
/// let query = OperationQuery::new("doc-1")
///     .for_entity("route.api")
///     .by_actor("alice")
///     .since(&ts1)
///     .until(&ts2)
///     .of_type("set_field")
///     .limit(100);
/// ```
#[derive(Debug, Clone)]
pub struct OperationQuery {
    pub(crate) document_id: String,
    pub(crate) entity_id: Option<String>,
    pub(crate) actor_id: Option<String>,
    pub(crate) since: Option<String>,
    pub(crate) until: Option<String>,
    pub(crate) op_type: Option<String>,
    pub(crate) limit: Option<u32>,
}

impl OperationQuery {
    /// Creates a new query scoped to a document.
    pub fn new(document_id: impl Into<String>) -> Self {
        Self {
            document_id: document_id.into(),
            entity_id: None,
            actor_id: None,
            since: None,
            until: None,
            op_type: None,
            limit: None,
        }
    }

    /// Filters operations for a specific entity.
    pub fn for_entity(mut self, entity_id: impl Into<String>) -> Self {
        self.entity_id = Some(entity_id.into());
        self
    }

    /// Filters operations by a specific actor.
    pub fn by_actor(mut self, actor_id: impl Into<String>) -> Self {
        self.actor_id = Some(actor_id.into());
        self
    }

    /// Filters operations after (inclusive) a given HLC timestamp.
    pub fn since(mut self, ts: &HlcTimestamp) -> Self {
        self.since = Some(ts.to_string());
        self
    }

    /// Filters operations before (inclusive) a given HLC timestamp.
    pub fn until(mut self, ts: &HlcTimestamp) -> Self {
        self.until = Some(ts.to_string());
        self
    }

    /// Filters operations by type name (e.g. "set_field", "insert_entity").
    pub fn of_type(mut self, op_type: impl Into<String>) -> Self {
        self.op_type = Some(op_type.into());
        self
    }

    /// Limits the number of results returned.
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Builds a SQL WHERE clause and parameter values from this query.
    ///
    /// Returns `(where_clause, params)` where `where_clause` starts with "WHERE"
    /// and params are `(name, value)` pairs for binding.
    pub(crate) fn to_sql(&self) -> (String, Vec<(String, String)>) {
        let mut conditions = vec!["document_id = :document_id".to_string()];
        let mut params = vec![(":document_id".to_string(), self.document_id.clone())];

        if let Some(ref entity_id) = self.entity_id {
            conditions.push("entity_id = :entity_id".to_string());
            params.push((":entity_id".to_string(), entity_id.clone()));
        }

        if let Some(ref actor_id) = self.actor_id {
            conditions.push("actor_id = :actor_id".to_string());
            params.push((":actor_id".to_string(), actor_id.clone()));
        }

        if let Some(ref since) = self.since {
            conditions.push("hlc_timestamp >= :since".to_string());
            params.push((":since".to_string(), since.clone()));
        }

        if let Some(ref until) = self.until {
            conditions.push("hlc_timestamp <= :until".to_string());
            params.push((":until".to_string(), until.clone()));
        }

        if let Some(ref op_type) = self.op_type {
            conditions.push("op_type = :op_type".to_string());
            params.push((":op_type".to_string(), op_type.clone()));
        }

        let where_clause = format!("WHERE {}", conditions.join(" AND "));
        (where_clause, params)
    }
}
