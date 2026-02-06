//! Request and response data transfer objects.

use conflux_core::{FieldValue, OperationKind};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Operation DTOs
// ============================================================================

/// Request to submit an operation.
#[derive(Debug, Clone, Deserialize)]
pub struct SubmitOperationRequest {
    /// The operation kind.
    #[serde(flatten)]
    pub operation: OperationDto,
    /// Optional intent describing why this operation is being made.
    pub intent: Option<String>,
}

/// An operation DTO that can be submitted via the API.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationDto {
    /// Set a field value on an entity.
    SetField {
        entity_id: String,
        field: String,
        value: serde_json::Value,
    },
    /// Insert a new entity.
    InsertEntity {
        entity_id: String,
        entity_type: String,
        parent_id: Option<String>,
        position: Option<String>,
    },
    /// Remove an entity (soft delete).
    RemoveEntity { entity_id: String },
    /// Move an entity to a new parent.
    MoveEntity {
        entity_id: String,
        new_parent_id: String,
        new_position: String,
    },
    /// Set an environment-specific field override.
    SetOverride {
        entity_id: String,
        field: String,
        environment: String,
        value: serde_json::Value,
    },
    /// Resolve a conflict by choosing a value.
    ResolveConflict {
        entity_id: String,
        field: String,
        /// Environment if resolving an override conflict.
        environment: Option<String>,
        chosen_value: serde_json::Value,
    },
}

/// Response after submitting an operation.
#[derive(Debug, Clone, Serialize)]
pub struct SubmitOperationResponse {
    /// The operation ID.
    pub operation_id: Uuid,
    /// The HLC timestamp assigned.
    pub timestamp: String,
    /// Whether a conflict was detected.
    pub conflict: bool,
    /// Conflict details if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict_info: Option<ConflictInfoDto>,
}

/// Conflict information.
#[derive(Debug, Clone, Serialize)]
pub struct ConflictInfoDto {
    pub entity_id: String,
    pub field: String,
    pub contending_values: Vec<serde_json::Value>,
}

/// Request to submit multiple operations atomically.
#[derive(Debug, Clone, Deserialize)]
pub struct BatchOperationRequest {
    pub operations: Vec<SubmitOperationRequest>,
}

/// Response for batch operation submission.
#[derive(Debug, Clone, Serialize)]
pub struct BatchOperationResponse {
    pub results: Vec<SubmitOperationResponse>,
    pub total: usize,
    pub conflicts: usize,
}

// ============================================================================
// State DTOs
// ============================================================================

/// Response containing entity state.
#[derive(Debug, Clone, Serialize)]
pub struct EntityStateResponse {
    pub entity_id: String,
    pub entity_type: String,
    pub fields: serde_json::Map<String, serde_json::Value>,
    pub parent_id: Option<String>,
    pub children: Vec<String>,
    pub tombstoned: bool,
}

/// Response containing full document state.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentStateResponse {
    pub entities: Vec<EntityStateResponse>,
    pub root_entities: Vec<String>,
}

// ============================================================================
// Milestone DTOs
// ============================================================================

/// Request to create a milestone.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateMilestoneRequest {
    /// Milestone message.
    pub message: String,
    /// Environments to include.
    #[serde(default)]
    pub environments: Vec<String>,
}

/// Response after creating a milestone.
#[derive(Debug, Clone, Serialize)]
pub struct CreateMilestoneResponse {
    pub milestone_id: Uuid,
    pub git_commit: Option<String>,
    pub operation_count: usize,
    pub files_written: Vec<String>,
}

/// Milestone summary for listing.
#[derive(Debug, Clone, Serialize)]
pub struct MilestoneSummary {
    pub id: Uuid,
    pub message: Option<String>,
    pub git_commit: Option<String>,
    pub created_at: String,
    pub hlc_range_start: String,
    pub hlc_range_end: String,
}

// ============================================================================
// Log DTOs
// ============================================================================

/// Query parameters for operation log.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LogQueryParams {
    /// Filter by entity ID.
    pub entity_id: Option<String>,
    /// Filter by actor ID.
    pub actor_id: Option<String>,
    /// Maximum number of results.
    pub limit: Option<u32>,
    /// Offset for pagination.
    pub offset: Option<u32>,
}

/// An operation in the log.
#[derive(Debug, Clone, Serialize)]
pub struct OperationLogEntry {
    pub id: Uuid,
    pub timestamp: String,
    pub actor_id: String,
    pub actor_class: String,
    pub intent: Option<String>,
    #[serde(flatten)]
    pub operation: OperationLogKind,
}

/// Operation kind in log format.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationLogKind {
    SetField {
        entity_id: String,
        field: String,
        value: serde_json::Value,
    },
    InsertEntity {
        entity_id: String,
        entity_type: String,
        parent_id: Option<String>,
    },
    RemoveEntity {
        entity_id: String,
    },
    MoveEntity {
        entity_id: String,
        new_parent_id: String,
    },
    SetOverride {
        entity_id: String,
        field: String,
        environment: String,
        value: serde_json::Value,
    },
    ResolveConflict {
        entity_id: String,
        field: String,
        environment: Option<String>,
        chosen_value: serde_json::Value,
    },
}

/// Response containing operation log entries.
#[derive(Debug, Clone, Serialize)]
pub struct OperationLogResponse {
    pub operations: Vec<OperationLogEntry>,
    pub total: u64,
    pub has_more: bool,
}

// ============================================================================
// Health DTOs
// ============================================================================

/// Health check response.
#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

// ============================================================================
// Environment DTOs
// ============================================================================

/// Response listing available environments.
#[derive(Debug, Clone, Serialize)]
pub struct EnvironmentsResponse {
    /// Base environment name.
    pub base: Option<String>,
    /// All environment names.
    pub environments: Vec<String>,
    /// Environment inheritance chain: child -> parent.
    pub inheritance: std::collections::HashMap<String, String>,
}

/// Response containing environment-specific entity state.
#[derive(Debug, Clone, Serialize)]
pub struct EnvironmentEntityStateResponse {
    pub entity_id: String,
    pub entity_type: String,
    /// Fields resolved for this environment (includes inherited values).
    pub fields: serde_json::Map<String, serde_json::Value>,
    /// Which environment each field value came from (base, or an overlay name).
    pub field_sources: std::collections::HashMap<String, String>,
    pub parent_id: Option<String>,
    pub children: Vec<String>,
    pub tombstoned: bool,
}

/// Response containing environment-specific document state.
#[derive(Debug, Clone, Serialize)]
pub struct EnvironmentStateResponse {
    /// The environment this state is for.
    pub environment: String,
    pub entities: Vec<EnvironmentEntityStateResponse>,
    pub root_entities: Vec<String>,
}

/// Query parameters for environment diff.
#[derive(Debug, Clone, Deserialize)]
pub struct EnvDiffParams {
    /// Source environment.
    pub from: String,
    /// Target environment.
    pub to: String,
}

/// A single field difference between environments.
#[derive(Debug, Clone, Serialize)]
pub struct FieldDiff {
    pub entity_id: String,
    pub field: String,
    pub from_value: Option<serde_json::Value>,
    pub to_value: Option<serde_json::Value>,
}

/// Response containing differences between two environments.
#[derive(Debug, Clone, Serialize)]
pub struct EnvironmentDiffResponse {
    pub from_env: String,
    pub to_env: String,
    pub differences: Vec<FieldDiff>,
}

// ============================================================================
// Conversion helpers
// ============================================================================

/// Converts a FieldValue to a JSON value.
pub fn field_value_to_json(value: &FieldValue) -> serde_json::Value {
    match value {
        FieldValue::String(s) => serde_json::json!(s),
        FieldValue::Int(i) => serde_json::json!(i),
        FieldValue::Float(f) => serde_json::json!(f),
        FieldValue::Bool(b) => serde_json::json!(b),
        FieldValue::Duration(d) => serde_json::json!(d),
        FieldValue::Ref(r) => serde_json::json!(r.to_string()),
        FieldValue::List(items) => {
            let arr: Vec<_> = items.iter().map(field_value_to_json).collect();
            serde_json::json!(arr)
        }
        FieldValue::Map(map) => {
            let obj: serde_json::Map<_, _> = map
                .iter()
                .map(|(k, v)| (k.clone(), field_value_to_json(v)))
                .collect();
            serde_json::json!(obj)
        }
        FieldValue::Null => serde_json::Value::Null,
    }
}

/// Converts a JSON value to a FieldValue.
pub fn json_to_field_value(value: &serde_json::Value) -> FieldValue {
    match value {
        serde_json::Value::String(s) => FieldValue::String(s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                FieldValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                FieldValue::Float(f)
            } else {
                FieldValue::Null
            }
        }
        serde_json::Value::Bool(b) => FieldValue::Bool(*b),
        serde_json::Value::Array(arr) => {
            FieldValue::List(arr.iter().map(json_to_field_value).collect())
        }
        serde_json::Value::Object(map) => FieldValue::Map(
            map.iter()
                .map(|(k, v)| (k.clone(), json_to_field_value(v)))
                .collect(),
        ),
        serde_json::Value::Null => FieldValue::Null,
    }
}

/// Converts an OperationKind to the log DTO format.
pub fn operation_kind_to_log(kind: &OperationKind) -> OperationLogKind {
    match kind {
        OperationKind::SetField {
            entity_id,
            field,
            value,
        } => OperationLogKind::SetField {
            entity_id: entity_id.to_string(),
            field: field.clone(),
            value: field_value_to_json(value),
        },
        OperationKind::InsertEntity {
            entity_id,
            entity_type,
            parent_id,
            ..
        } => OperationLogKind::InsertEntity {
            entity_id: entity_id.to_string(),
            entity_type: entity_type.clone(),
            parent_id: parent_id.as_ref().map(|p| p.to_string()),
        },
        OperationKind::RemoveEntity { entity_id } => OperationLogKind::RemoveEntity {
            entity_id: entity_id.to_string(),
        },
        OperationKind::MoveEntity {
            entity_id,
            new_parent_id,
            ..
        } => OperationLogKind::MoveEntity {
            entity_id: entity_id.to_string(),
            new_parent_id: new_parent_id.to_string(),
        },
        OperationKind::SetOverride {
            entity_id,
            field,
            environment,
            value,
        } => OperationLogKind::SetOverride {
            entity_id: entity_id.to_string(),
            field: field.clone(),
            environment: environment.clone(),
            value: field_value_to_json(value),
        },
        OperationKind::ResolveConflict {
            entity_id,
            field,
            environment,
            chosen_value,
        } => OperationLogKind::ResolveConflict {
            entity_id: entity_id.to_string(),
            field: field.clone(),
            environment: environment.clone(),
            chosen_value: field_value_to_json(chosen_value),
        },
    }
}

// ============================================================================
// Webhook DTOs
// ============================================================================

/// Request to register a webhook.
#[derive(Debug, Clone, Deserialize)]
pub struct RegisterWebhookRequest {
    /// URL to deliver events to.
    pub url: String,
    /// Optional filter by entity ID prefix.
    #[serde(default)]
    pub entity_filter: Option<String>,
    /// Optional filter by field name.
    #[serde(default)]
    pub field_filter: Option<String>,
    /// Optional secret for HMAC signing.
    #[serde(default)]
    pub secret: Option<String>,
    /// Event filter (all, state_changes, conflicts).
    #[serde(default)]
    pub event_filter: Option<String>,
    /// Payload format (conflux, gitops).
    #[serde(default)]
    pub format: Option<String>,
}

/// Response after registering a webhook.
#[derive(Debug, Clone, Serialize)]
pub struct RegisterWebhookResponse {
    /// The webhook ID.
    pub id: Uuid,
    /// The URL registered.
    pub url: String,
    /// Whether the webhook is enabled.
    pub enabled: bool,
}

/// Webhook information returned by list and get.
#[derive(Debug, Clone, Serialize)]
pub struct WebhookDto {
    /// Unique webhook ID.
    pub id: Uuid,
    /// URL to deliver events to.
    pub url: String,
    /// Entity filter (if set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_filter: Option<String>,
    /// Field filter (if set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_filter: Option<String>,
    /// Whether the webhook is enabled.
    pub enabled: bool,
    /// Event filter (all, state_changes, conflicts).
    pub event_filter: String,
    /// Payload format (conflux, gitops).
    pub format: String,
    /// Number of consecutive failures.
    pub failure_count: u32,
    /// Last error message (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Response for listing webhooks.
#[derive(Debug, Clone, Serialize)]
pub struct ListWebhooksResponse {
    pub webhooks: Vec<WebhookDto>,
    pub total: usize,
}

/// Request to update a webhook.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateWebhookRequest {
    /// New URL (optional).
    #[serde(default)]
    pub url: Option<String>,
    /// Enable/disable the webhook (optional).
    #[serde(default)]
    pub enabled: Option<bool>,
    /// New entity filter (optional).
    #[serde(default)]
    pub entity_filter: Option<String>,
    /// New field filter (optional).
    #[serde(default)]
    pub field_filter: Option<String>,
    /// New event filter (optional).
    #[serde(default)]
    pub event_filter: Option<String>,
    /// New payload format (optional).
    #[serde(default)]
    pub format: Option<String>,
}

// ============================================================================
// Audit DTOs
// ============================================================================

/// Query parameters for audit export.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AuditExportParams {
    /// Export operations since this timestamp (ISO 8601 or HLC string).
    pub since: Option<String>,
    /// Export operations until this timestamp (ISO 8601 or HLC string).
    pub until: Option<String>,
    /// Filter by actor ID.
    pub actor: Option<String>,
    /// Filter by entity ID prefix.
    pub entity: Option<String>,
    /// Verify signatures during export.
    #[serde(default)]
    pub verify: bool,
}
