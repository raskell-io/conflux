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
        OperationKind::SetOverride { entity_id, .. } => {
            // Treat as SetField for logging purposes
            OperationLogKind::SetField {
                entity_id: entity_id.to_string(),
                field: "(override)".to_string(),
                value: serde_json::Value::Null,
            }
        }
    }
}
