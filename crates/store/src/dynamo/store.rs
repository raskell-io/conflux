//! DynamoDB implementation of AsyncStore.

use super::keys::Keys;
use crate::error::StoreError;
use crate::models::{StoredMilestone, StoredOperation, StoredSnapshot};
use crate::query::OperationQuery;
use crate::AsyncStore;
use async_trait::async_trait;
use aws_sdk_dynamodb::types::{
    AttributeDefinition, AttributeValue, GlobalSecondaryIndex, KeySchemaElement, KeyType,
    Projection, ProjectionType, ProvisionedThroughput, ScalarAttributeType,
};
use aws_sdk_dynamodb::Client;
use conflux_core::{Document, HlcTimestamp, Operation, OperationKind};
use uuid::Uuid;

/// DynamoDB-backed async store for operations, snapshots, and milestones.
///
/// Uses a single-table design optimized for serverless deployments.
pub struct DynamoStore {
    client: Client,
    table_name: String,
}

impl DynamoStore {
    /// Creates a new DynamoDB store using the default AWS configuration.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::DynamoDB` if AWS configuration loading fails.
    pub async fn new(table_name: &str) -> Result<Self, StoreError> {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = Client::new(&config);
        Ok(Self {
            client,
            table_name: table_name.to_string(),
        })
    }

    /// Creates a DynamoDB store with a custom client.
    ///
    /// Useful for testing with LocalStack or custom configurations.
    pub fn with_client(client: Client, table_name: &str) -> Self {
        Self {
            client,
            table_name: table_name.to_string(),
        }
    }

    /// Returns a reference to the underlying DynamoDB client.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Returns the table name.
    pub fn table_name(&self) -> &str {
        &self.table_name
    }

    /// Creates the table with required GSIs if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::DynamoDB` if table creation fails.
    pub async fn create_table_if_not_exists(&self) -> Result<(), StoreError> {
        // Check if table exists
        let tables = self
            .client
            .list_tables()
            .send()
            .await?;

        if tables
            .table_names()
            .iter()
            .any(|n| n == &self.table_name)
        {
            return Ok(());
        }

        // Create table with GSIs
        self.client
            .create_table()
            .table_name(&self.table_name)
            // Primary key: pk (partition) + sk (sort)
            .attribute_definitions(
                AttributeDefinition::builder()
                    .attribute_name("pk")
                    .attribute_type(ScalarAttributeType::S)
                    .build()?,
            )
            .attribute_definitions(
                AttributeDefinition::builder()
                    .attribute_name("sk")
                    .attribute_type(ScalarAttributeType::S)
                    .build()?,
            )
            // GSI1 key
            .attribute_definitions(
                AttributeDefinition::builder()
                    .attribute_name("gsi1_pk")
                    .attribute_type(ScalarAttributeType::S)
                    .build()?,
            )
            // GSI2 keys
            .attribute_definitions(
                AttributeDefinition::builder()
                    .attribute_name("gsi2_pk")
                    .attribute_type(ScalarAttributeType::S)
                    .build()?,
            )
            .attribute_definitions(
                AttributeDefinition::builder()
                    .attribute_name("gsi2_sk")
                    .attribute_type(ScalarAttributeType::S)
                    .build()?,
            )
            // Primary key schema
            .key_schema(
                KeySchemaElement::builder()
                    .attribute_name("pk")
                    .key_type(KeyType::Hash)
                    .build()?,
            )
            .key_schema(
                KeySchemaElement::builder()
                    .attribute_name("sk")
                    .key_type(KeyType::Range)
                    .build()?,
            )
            // GSI1: Operation lookup by ID
            .global_secondary_indexes(
                GlobalSecondaryIndex::builder()
                    .index_name("gsi1")
                    .key_schema(
                        KeySchemaElement::builder()
                            .attribute_name("gsi1_pk")
                            .key_type(KeyType::Hash)
                            .build()?,
                    )
                    .projection(
                        Projection::builder()
                            .projection_type(ProjectionType::All)
                            .build(),
                    )
                    .provisioned_throughput(
                        ProvisionedThroughput::builder()
                            .read_capacity_units(5)
                            .write_capacity_units(5)
                            .build()?,
                    )
                    .build()?,
            )
            // GSI2: Operations by actor
            .global_secondary_indexes(
                GlobalSecondaryIndex::builder()
                    .index_name("gsi2")
                    .key_schema(
                        KeySchemaElement::builder()
                            .attribute_name("gsi2_pk")
                            .key_type(KeyType::Hash)
                            .build()?,
                    )
                    .key_schema(
                        KeySchemaElement::builder()
                            .attribute_name("gsi2_sk")
                            .key_type(KeyType::Range)
                            .build()?,
                    )
                    .projection(
                        Projection::builder()
                            .projection_type(ProjectionType::All)
                            .build(),
                    )
                    .provisioned_throughput(
                        ProvisionedThroughput::builder()
                            .read_capacity_units(5)
                            .write_capacity_units(5)
                            .build()?,
                    )
                    .build()?,
            )
            .provisioned_throughput(
                ProvisionedThroughput::builder()
                    .read_capacity_units(5)
                    .write_capacity_units(5)
                    .build()?,
            )
            .send()
            .await?;

        // Wait for table to become active
        self.client
            .describe_table()
            .table_name(&self.table_name)
            .send()
            .await?;

        Ok(())
    }
}

#[async_trait]
impl AsyncStore for DynamoStore {
    async fn append_operation(
        &self,
        document_id: &str,
        operation: &Operation,
    ) -> Result<(), StoreError> {
        let payload = serde_json::to_string(operation)?;
        let entity_id = entity_id_from_op(operation);
        let op_type = op_type_str(operation);
        let created_at = chrono::Utc::now().to_rfc3339();

        self.client
            .put_item()
            .table_name(&self.table_name)
            // Primary key
            .item("pk", AttributeValue::S(Keys::doc_pk(document_id)))
            .item(
                "sk",
                AttributeValue::S(Keys::operation_sk(&operation.timestamp, &operation.id)),
            )
            // GSI1: Operation lookup by ID
            .item(
                "gsi1_pk",
                AttributeValue::S(Keys::operation_gsi1_pk(&operation.id)),
            )
            // GSI2: Actor-based queries
            .item(
                "gsi2_pk",
                AttributeValue::S(Keys::actor_gsi2_pk(document_id, &operation.actor.id)),
            )
            .item(
                "gsi2_sk",
                AttributeValue::S(operation.timestamp.to_string()),
            )
            // Data attributes
            .item("id", AttributeValue::S(operation.id.to_string()))
            .item("document_id", AttributeValue::S(document_id.to_string()))
            .item(
                "hlc_timestamp",
                AttributeValue::S(operation.timestamp.to_string()),
            )
            .item("actor_id", AttributeValue::S(operation.actor.id.clone()))
            .item(
                "actor_class",
                AttributeValue::S(operation.actor.class.to_string()),
            )
            .item("op_type", AttributeValue::S(op_type.to_string()))
            .item("entity_id", AttributeValue::S(entity_id.to_string()))
            .item("payload", AttributeValue::S(payload))
            .item(
                "intent",
                match &operation.intent {
                    Some(i) => AttributeValue::S(i.clone()),
                    None => AttributeValue::Null(true),
                },
            )
            .item("created_at", AttributeValue::S(created_at))
            .item("item_type", AttributeValue::S("operation".to_string()))
            .send()
            .await?;

        Ok(())
    }

    async fn get_operation(&self, operation_id: &Uuid) -> Result<StoredOperation, StoreError> {
        // Use GSI1 for operation lookup by ID
        let result = self
            .client
            .query()
            .table_name(&self.table_name)
            .index_name("gsi1")
            .key_condition_expression("gsi1_pk = :pk")
            .expression_attribute_values(
                ":pk",
                AttributeValue::S(Keys::operation_gsi1_pk(operation_id)),
            )
            .send()
            .await?;

        let items = result.items();
        if items.is_empty() {
            return Err(StoreError::NotFound(format!("operation {operation_id}")));
        }

        let item = &items[0];
        parse_operation_item(item)
    }

    async fn query_operations(
        &self,
        query: &OperationQuery,
    ) -> Result<Vec<StoredOperation>, StoreError> {
        let pk = Keys::doc_pk(&query.document_id);

        let mut key_condition = "pk = :pk AND begins_with(sk, :sk_prefix)".to_string();
        let mut filter_conditions: Vec<String> = Vec::new();
        let mut expr_values: Vec<(String, AttributeValue)> = vec![
            (":pk".to_string(), AttributeValue::S(pk)),
            (
                ":sk_prefix".to_string(),
                AttributeValue::S(Keys::OP_PREFIX.to_string()),
            ),
        ];

        // Add range conditions
        if let Some(ref since) = query.since {
            key_condition = "pk = :pk AND sk >= :sk_start".to_string();
            expr_values.retain(|(k, _)| k != ":sk_prefix");
            // Use a sort key that starts after the "since" timestamp
            let sk_start = format!("{}{}#", Keys::OP_PREFIX, since);
            expr_values.push((":sk_start".to_string(), AttributeValue::S(sk_start)));
        }

        if let Some(ref until) = query.until {
            if query.since.is_some() {
                // Add upper bound to key condition (can't use both in key condition, use filter)
                filter_conditions.push("hlc_timestamp <= :until".to_string());
            } else {
                // Use sort key for upper bound
                key_condition = "pk = :pk AND sk <= :sk_end".to_string();
                expr_values.retain(|(k, _)| k != ":sk_prefix");
                // Pad with high UUID value to include all operations at this timestamp
                let sk_end = format!(
                    "{}{}#ffffffff-ffff-ffff-ffff-ffffffffffff",
                    Keys::OP_PREFIX, until
                );
                expr_values.push((":sk_end".to_string(), AttributeValue::S(sk_end)));
            }
            expr_values.push((":until".to_string(), AttributeValue::S(until.clone())));
        }

        // Add filter conditions
        if let Some(ref entity_id) = query.entity_id {
            filter_conditions.push("entity_id = :entity_id".to_string());
            expr_values.push((":entity_id".to_string(), AttributeValue::S(entity_id.clone())));
        }

        if let Some(ref actor_id) = query.actor_id {
            filter_conditions.push("actor_id = :actor_id".to_string());
            expr_values.push((":actor_id".to_string(), AttributeValue::S(actor_id.clone())));
        }

        if let Some(ref op_type) = query.op_type {
            filter_conditions.push("op_type = :op_type".to_string());
            expr_values.push((":op_type".to_string(), AttributeValue::S(op_type.clone())));
        }

        // Ensure we only get operations (not snapshots, milestones, etc.)
        filter_conditions.push("item_type = :item_type".to_string());
        expr_values.push((
            ":item_type".to_string(),
            AttributeValue::S("operation".to_string()),
        ));

        let filter_expression = if filter_conditions.is_empty() {
            None
        } else {
            Some(filter_conditions.join(" AND "))
        };

        let mut query_builder = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression(&key_condition);

        for (name, value) in expr_values {
            query_builder = query_builder.expression_attribute_values(name, value);
        }

        if let Some(filter) = filter_expression {
            query_builder = query_builder.filter_expression(filter);
        }

        if let Some(limit) = query.limit {
            query_builder = query_builder.limit(limit as i32);
        }

        let result = query_builder.send().await?;

        let mut operations = Vec::new();
        for item in result.items() {
            operations.push(parse_operation_item(item)?);
        }

        Ok(operations)
    }

    async fn operation_count(&self, document_id: &str) -> Result<u64, StoreError> {
        let pk = Keys::doc_pk(document_id);

        let result = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("pk = :pk AND begins_with(sk, :sk_prefix)")
            .filter_expression("item_type = :item_type")
            .expression_attribute_values(":pk", AttributeValue::S(pk))
            .expression_attribute_values(
                ":sk_prefix",
                AttributeValue::S(Keys::OP_PREFIX.to_string()),
            )
            .expression_attribute_values(
                ":item_type",
                AttributeValue::S("operation".to_string()),
            )
            .select(aws_sdk_dynamodb::types::Select::Count)
            .send()
            .await?;

        Ok(result.count() as u64)
    }

    async fn operation_exists(&self, operation_id: &Uuid) -> Result<bool, StoreError> {
        let result = self
            .client
            .query()
            .table_name(&self.table_name)
            .index_name("gsi1")
            .key_condition_expression("gsi1_pk = :pk")
            .expression_attribute_values(
                ":pk",
                AttributeValue::S(Keys::operation_gsi1_pk(operation_id)),
            )
            .select(aws_sdk_dynamodb::types::Select::Count)
            .send()
            .await?;

        Ok(result.count() > 0)
    }

    async fn get_operations_by_actor_since(
        &self,
        document_id: &str,
        actor_id: &str,
        since: Option<&HlcTimestamp>,
    ) -> Result<Vec<StoredOperation>, StoreError> {
        let gsi2_pk = Keys::actor_gsi2_pk(document_id, actor_id);

        let key_condition = match since {
            Some(_ts) => "gsi2_pk = :pk AND gsi2_sk > :since".to_string(),
            None => "gsi2_pk = :pk".to_string(),
        };

        let mut query_builder = self
            .client
            .query()
            .table_name(&self.table_name)
            .index_name("gsi2")
            .key_condition_expression(&key_condition)
            .expression_attribute_values(":pk", AttributeValue::S(gsi2_pk));

        if let Some(ts) = since {
            query_builder = query_builder
                .expression_attribute_values(":since", AttributeValue::S(ts.to_string()));
        }

        let result = query_builder.send().await?;

        let mut operations = Vec::new();
        for item in result.items() {
            operations.push(parse_operation_item(item)?);
        }

        Ok(operations)
    }

    async fn save_snapshot(
        &self,
        document_id: &str,
        hlc_timestamp: &HlcTimestamp,
        document: &Document,
    ) -> Result<Uuid, StoreError> {
        let id = Uuid::new_v4();
        let data = serde_json::to_string(document)?;
        let created_at = chrono::Utc::now().to_rfc3339();

        self.client
            .put_item()
            .table_name(&self.table_name)
            .item("pk", AttributeValue::S(Keys::doc_pk(document_id)))
            .item(
                "sk",
                AttributeValue::S(Keys::snapshot_sk(hlc_timestamp, &id)),
            )
            .item("id", AttributeValue::S(id.to_string()))
            .item("document_id", AttributeValue::S(document_id.to_string()))
            .item(
                "hlc_timestamp",
                AttributeValue::S(hlc_timestamp.to_string()),
            )
            .item("data", AttributeValue::S(data))
            .item("created_at", AttributeValue::S(created_at))
            .item("item_type", AttributeValue::S("snapshot".to_string()))
            .send()
            .await?;

        Ok(id)
    }

    async fn latest_snapshot(&self, document_id: &str) -> Result<StoredSnapshot, StoreError> {
        let pk = Keys::doc_pk(document_id);

        let result = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("pk = :pk AND begins_with(sk, :sk_prefix)")
            .filter_expression("item_type = :item_type")
            .expression_attribute_values(":pk", AttributeValue::S(pk))
            .expression_attribute_values(
                ":sk_prefix",
                AttributeValue::S(Keys::SNAP_PREFIX.to_string()),
            )
            .expression_attribute_values(
                ":item_type",
                AttributeValue::S("snapshot".to_string()),
            )
            .scan_index_forward(false) // Descending order (latest first)
            .limit(1)
            .send()
            .await?;

        let items = result.items();
        if items.is_empty() {
            return Err(StoreError::NotFound(format!(
                "snapshot for document {document_id}"
            )));
        }

        let item = &items[0];
        parse_snapshot_item(item, document_id)
    }

    async fn operations_since_snapshot(&self, document_id: &str) -> Result<u64, StoreError> {
        // Try to get latest snapshot
        let snapshot_result = self.latest_snapshot(document_id).await;

        match snapshot_result {
            Ok(snapshot) => {
                // Count operations after snapshot timestamp
                let pk = Keys::doc_pk(document_id);
                let sk_start = format!("{}{}#", Keys::OP_PREFIX, snapshot.hlc_timestamp);

                let result = self
                    .client
                    .query()
                    .table_name(&self.table_name)
                    .key_condition_expression("pk = :pk AND sk > :sk_start")
                    .filter_expression(
                        "item_type = :item_type AND begins_with(sk, :op_prefix)",
                    )
                    .expression_attribute_values(":pk", AttributeValue::S(pk))
                    .expression_attribute_values(":sk_start", AttributeValue::S(sk_start))
                    .expression_attribute_values(
                        ":item_type",
                        AttributeValue::S("operation".to_string()),
                    )
                    .expression_attribute_values(
                        ":op_prefix",
                        AttributeValue::S(Keys::OP_PREFIX.to_string()),
                    )
                    .select(aws_sdk_dynamodb::types::Select::Count)
                    .send()
                    .await?;

                Ok(result.count() as u64)
            }
            Err(StoreError::NotFound(_)) => {
                // No snapshots, return total operation count
                self.operation_count(document_id).await
            }
            Err(e) => Err(e),
        }
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
        let created_at = chrono::Utc::now().to_rfc3339();

        let mut put_item = self
            .client
            .put_item()
            .table_name(&self.table_name)
            .item("pk", AttributeValue::S(Keys::doc_pk(document_id)))
            .item(
                "sk",
                AttributeValue::S(Keys::milestone_sk(&created_at, &id)),
            )
            .item("id", AttributeValue::S(id.to_string()))
            .item("document_id", AttributeValue::S(document_id.to_string()))
            .item(
                "hlc_range_start",
                AttributeValue::S(hlc_range_start.to_string()),
            )
            .item(
                "hlc_range_end",
                AttributeValue::S(hlc_range_end.to_string()),
            )
            .item("created_at", AttributeValue::S(created_at))
            .item("item_type", AttributeValue::S("milestone".to_string()));

        if let Some(commit) = git_commit {
            put_item = put_item.item("git_commit", AttributeValue::S(commit.to_string()));
        }
        if let Some(msg) = message {
            put_item = put_item.item("message", AttributeValue::S(msg.to_string()));
        }

        put_item.send().await?;

        Ok(id)
    }

    async fn latest_milestone(
        &self,
        document_id: &str,
    ) -> Result<Option<StoredMilestone>, StoreError> {
        let pk = Keys::doc_pk(document_id);

        let result = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("pk = :pk AND begins_with(sk, :sk_prefix)")
            .filter_expression("item_type = :item_type")
            .expression_attribute_values(":pk", AttributeValue::S(pk))
            .expression_attribute_values(
                ":sk_prefix",
                AttributeValue::S(Keys::MILE_PREFIX.to_string()),
            )
            .expression_attribute_values(
                ":item_type",
                AttributeValue::S("milestone".to_string()),
            )
            .scan_index_forward(false) // Descending order (latest first)
            .limit(1)
            .send()
            .await?;

        let items = result.items();
        if items.is_empty() {
            return Ok(None);
        }

        let item = &items[0];
        Ok(Some(parse_milestone_item(item, document_id)?))
    }

    async fn list_milestones(&self, document_id: &str) -> Result<Vec<StoredMilestone>, StoreError> {
        let pk = Keys::doc_pk(document_id);

        let result = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("pk = :pk AND begins_with(sk, :sk_prefix)")
            .filter_expression("item_type = :item_type")
            .expression_attribute_values(":pk", AttributeValue::S(pk))
            .expression_attribute_values(
                ":sk_prefix",
                AttributeValue::S(Keys::MILE_PREFIX.to_string()),
            )
            .expression_attribute_values(
                ":item_type",
                AttributeValue::S("milestone".to_string()),
            )
            .scan_index_forward(false) // Descending order (newest first)
            .send()
            .await?;

        let mut milestones = Vec::new();
        for item in result.items() {
            milestones.push(parse_milestone_item(item, document_id)?);
        }

        Ok(milestones)
    }

    async fn operations_since_milestone(&self, document_id: &str) -> Result<u64, StoreError> {
        let milestone = self.latest_milestone(document_id).await?;

        match milestone {
            Some(m) => {
                // Count operations after milestone's hlc_range_end
                let pk = Keys::doc_pk(document_id);
                let sk_start = format!("{}{}#", Keys::OP_PREFIX, m.hlc_range_end);

                let result = self
                    .client
                    .query()
                    .table_name(&self.table_name)
                    .key_condition_expression("pk = :pk AND sk > :sk_start")
                    .filter_expression(
                        "item_type = :item_type AND begins_with(sk, :op_prefix)",
                    )
                    .expression_attribute_values(":pk", AttributeValue::S(pk))
                    .expression_attribute_values(":sk_start", AttributeValue::S(sk_start))
                    .expression_attribute_values(
                        ":item_type",
                        AttributeValue::S("operation".to_string()),
                    )
                    .expression_attribute_values(
                        ":op_prefix",
                        AttributeValue::S(Keys::OP_PREFIX.to_string()),
                    )
                    .select(aws_sdk_dynamodb::types::Select::Count)
                    .send()
                    .await?;

                Ok(result.count() as u64)
            }
            None => {
                // No milestones, return total operation count
                self.operation_count(document_id).await
            }
        }
    }

    async fn save_version_vector(
        &self,
        document_id: &str,
        version_vector_json: &str,
    ) -> Result<(), StoreError> {
        let updated_at = chrono::Utc::now().to_rfc3339();

        self.client
            .put_item()
            .table_name(&self.table_name)
            .item("pk", AttributeValue::S(Keys::doc_pk(document_id)))
            .item(
                "sk",
                AttributeValue::S(Keys::version_vector_sk().to_string()),
            )
            .item("document_id", AttributeValue::S(document_id.to_string()))
            .item("data", AttributeValue::S(version_vector_json.to_string()))
            .item("updated_at", AttributeValue::S(updated_at))
            .item(
                "item_type",
                AttributeValue::S("version_vector".to_string()),
            )
            .send()
            .await?;

        Ok(())
    }

    async fn load_version_vector(&self, document_id: &str) -> Result<String, StoreError> {
        let result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(Keys::doc_pk(document_id)))
            .key(
                "sk",
                AttributeValue::S(Keys::version_vector_sk().to_string()),
            )
            .send()
            .await?;

        match result.item() {
            Some(item) => {
                let data = get_string_attr(item, "data")?;
                Ok(data)
            }
            None => Err(StoreError::NotFound(format!(
                "version vector for document {document_id}"
            ))),
        }
    }
}

// --- Helper functions ---

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

/// Returns the operation type as a string for the op_type attribute.
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

/// Extracts a string attribute from a DynamoDB item.
fn get_string_attr(
    item: &std::collections::HashMap<String, AttributeValue>,
    key: &str,
) -> Result<String, StoreError> {
    item.get(key)
        .and_then(|v| v.as_s().ok())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            StoreError::Integrity(format!("missing or invalid attribute: {key}"))
        })
}

/// Extracts an optional string attribute from a DynamoDB item.
fn get_optional_string_attr(
    item: &std::collections::HashMap<String, AttributeValue>,
    key: &str,
) -> Option<String> {
    item.get(key)
        .and_then(|v| v.as_s().ok())
        .map(|s| s.to_string())
}

/// Parses an operation item from DynamoDB.
fn parse_operation_item(
    item: &std::collections::HashMap<String, AttributeValue>,
) -> Result<StoredOperation, StoreError> {
    let payload = get_string_attr(item, "payload")?;
    let document_id = get_string_attr(item, "document_id")?;
    let created_at = get_string_attr(item, "created_at")?;

    let operation: Operation = serde_json::from_str(&payload)?;

    Ok(StoredOperation {
        operation,
        document_id,
        created_at,
    })
}

/// Parses a snapshot item from DynamoDB.
fn parse_snapshot_item(
    item: &std::collections::HashMap<String, AttributeValue>,
    document_id: &str,
) -> Result<StoredSnapshot, StoreError> {
    let id_str = get_string_attr(item, "id")?;
    let hlc_timestamp_str = get_string_attr(item, "hlc_timestamp")?;
    let data = get_string_attr(item, "data")?;
    let created_at = get_string_attr(item, "created_at")?;

    let id: Uuid = id_str
        .parse()
        .map_err(|e| StoreError::Integrity(format!("invalid snapshot UUID: {e}")))?;
    let hlc_timestamp: HlcTimestamp =
        serde_json::from_value(serde_json::Value::String(hlc_timestamp_str))?;
    let document: Document = serde_json::from_str(&data)?;

    Ok(StoredSnapshot {
        id,
        document_id: document_id.to_string(),
        hlc_timestamp,
        document,
        created_at,
    })
}

/// Parses a milestone item from DynamoDB.
fn parse_milestone_item(
    item: &std::collections::HashMap<String, AttributeValue>,
    document_id: &str,
) -> Result<StoredMilestone, StoreError> {
    let id_str = get_string_attr(item, "id")?;
    let hlc_range_start_str = get_string_attr(item, "hlc_range_start")?;
    let hlc_range_end_str = get_string_attr(item, "hlc_range_end")?;
    let created_at = get_string_attr(item, "created_at")?;
    let git_commit = get_optional_string_attr(item, "git_commit");
    let message = get_optional_string_attr(item, "message");

    let id: Uuid = id_str
        .parse()
        .map_err(|e| StoreError::Integrity(format!("invalid milestone UUID: {e}")))?;
    let hlc_range_start: HlcTimestamp =
        serde_json::from_value(serde_json::Value::String(hlc_range_start_str))?;
    let hlc_range_end: HlcTimestamp =
        serde_json::from_value(serde_json::Value::String(hlc_range_end_str))?;

    Ok(StoredMilestone {
        id,
        document_id: document_id.to_string(),
        git_commit,
        hlc_range_start,
        hlc_range_end,
        message,
        created_at,
    })
}

#[cfg(test)]
mod tests {
    // Integration tests require LocalStack or a real DynamoDB instance.
    // Use testcontainers with LocalStack for CI.
    //
    // Example test setup:
    //
    // #[tokio::test]
    // async fn dynamo_append_and_get() {
    //     // Start LocalStack container
    //     let client = localstack_client().await;
    //     let store = DynamoStore::with_client(client, "conflux-test");
    //     store.create_table_if_not_exists().await.unwrap();
    //     // ... test logic
    // }
}
