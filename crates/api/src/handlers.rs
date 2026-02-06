//! HTTP request handlers.

use crate::auth::Actor;
use crate::dto::{
    field_value_to_json, json_to_field_value, operation_kind_to_log, BatchOperationRequest,
    BatchOperationResponse, ConflictInfoDto, CreateMilestoneRequest, CreateMilestoneResponse,
    DocumentStateResponse, EntityStateResponse, EnvDiffParams, EnvironmentDiffResponse,
    EnvironmentEntityStateResponse, EnvironmentStateResponse, EnvironmentsResponse, FieldDiff,
    HealthResponse, ListWebhooksResponse, LogQueryParams, MilestoneSummary, OperationDto,
    OperationLogEntry, OperationLogResponse, RegisterWebhookRequest, RegisterWebhookResponse,
    SubmitOperationRequest, SubmitOperationResponse, UpdateWebhookRequest, WebhookDto,
};
use crate::error::ApiError;
use crate::state::AppState;
use crate::webhook::{WebhookConfig, WebhookEventFilter, WebhookFormat};
use axum::extract::{Path, Query, State};
use axum::Json;
use conflux_core::schema_info::SchemaInfo;
use conflux_core::{ApplyResult, EntityId, Operation};
use conflux_store::OperationQuery;
use uuid::Uuid;

// ============================================================================
// Health
// ============================================================================

/// Health check endpoint.
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

// ============================================================================
// Operations
// ============================================================================

/// Submit a single operation.
pub async fn submit_operation(
    State(state): State<AppState>,
    actor: Actor,
    Json(req): Json<SubmitOperationRequest>,
) -> Result<Json<SubmitOperationResponse>, ApiError> {
    let timestamp = state.clock.new_timestamp();

    // Convert DTO to core Operation
    let mut operation = match req.operation {
        OperationDto::SetField {
            entity_id,
            field,
            value,
        } => Operation::set_field(
            entity_id,
            field,
            json_to_field_value(&value),
            &actor.0,
            timestamp,
        ),
        OperationDto::InsertEntity {
            entity_id,
            entity_type,
            parent_id,
            position,
        } => Operation::insert_entity(
            entity_id,
            entity_type,
            parent_id.map(EntityId::new),
            position,
            &actor.0,
            timestamp,
        ),
        OperationDto::RemoveEntity { entity_id } => {
            Operation::remove_entity(entity_id, &actor.0, timestamp)
        }
        OperationDto::MoveEntity {
            entity_id,
            new_parent_id,
            new_position,
        } => Operation::move_entity(entity_id, new_parent_id, new_position, &actor.0, timestamp),
        OperationDto::SetOverride {
            entity_id,
            field,
            environment,
            value,
        } => Operation::set_override(
            entity_id,
            field,
            environment,
            json_to_field_value(&value),
            &actor.0,
            timestamp,
        ),
        OperationDto::ResolveConflict {
            entity_id,
            field,
            environment,
            chosen_value,
        } => Operation::resolve_conflict(
            entity_id,
            field,
            environment,
            json_to_field_value(&chosen_value),
            &actor.0,
            timestamp,
        ),
    };

    if let Some(intent) = req.intent {
        operation = operation.with_intent(intent);
    }

    // Get schema info from the configured schema
    let schema_info = state.schema.as_schema_info();

    // Apply to document
    let (conflict, conflict_info) = {
        let mut doc = state
            .document
            .write()
            .map_err(|e| ApiError::Internal(format!("lock poisoned: {e}")))?;

        match doc.apply(&operation, &schema_info, &state.clock)? {
            ApplyResult::Applied => (false, None),
            ApplyResult::Conflict(info) => (
                true,
                Some(ConflictInfoDto {
                    entity_id: info.entity_id.to_string(),
                    field: info.field,
                    contending_values: info
                        .contending_values
                        .iter()
                        .map(field_value_to_json)
                        .collect(),
                }),
            ),
        }
    };

    // Persist to store
    {
        let store = state
            .store
            .lock()
            .map_err(|e| ApiError::Internal(format!("store lock poisoned: {e}")))?;
        store.append_operation(&state.document_id, &operation)?;
    }

    Ok(Json(SubmitOperationResponse {
        operation_id: operation.id,
        timestamp: operation.timestamp.to_string(),
        conflict,
        conflict_info,
    }))
}

/// Submit multiple operations atomically.
pub async fn batch_operations(
    State(state): State<AppState>,
    actor: Actor,
    Json(req): Json<BatchOperationRequest>,
) -> Result<Json<BatchOperationResponse>, ApiError> {
    let mut results = Vec::with_capacity(req.operations.len());
    let mut conflicts = 0;

    let schema_info = state.schema.as_schema_info();

    for op_req in req.operations {
        let timestamp = state.clock.new_timestamp();

        let mut operation = match op_req.operation {
            OperationDto::SetField {
                entity_id,
                field,
                value,
            } => Operation::set_field(
                entity_id,
                field,
                json_to_field_value(&value),
                &actor.0,
                timestamp,
            ),
            OperationDto::InsertEntity {
                entity_id,
                entity_type,
                parent_id,
                position,
            } => Operation::insert_entity(
                entity_id,
                entity_type,
                parent_id.map(EntityId::new),
                position,
                &actor.0,
                timestamp,
            ),
            OperationDto::RemoveEntity { entity_id } => {
                Operation::remove_entity(entity_id, &actor.0, timestamp)
            }
            OperationDto::MoveEntity {
                entity_id,
                new_parent_id,
                new_position,
            } => {
                Operation::move_entity(entity_id, new_parent_id, new_position, &actor.0, timestamp)
            }
            OperationDto::SetOverride {
                entity_id,
                field,
                environment,
                value,
            } => Operation::set_override(
                entity_id,
                field,
                environment,
                json_to_field_value(&value),
                &actor.0,
                timestamp,
            ),
            OperationDto::ResolveConflict {
                entity_id,
                field,
                environment,
                chosen_value,
            } => Operation::resolve_conflict(
                entity_id,
                field,
                environment,
                json_to_field_value(&chosen_value),
                &actor.0,
                timestamp,
            ),
        };

        if let Some(intent) = op_req.intent {
            operation = operation.with_intent(intent);
        }

        // Apply to document
        let (conflict, conflict_info) = {
            let mut doc = state
                .document
                .write()
                .map_err(|e| ApiError::Internal(format!("lock poisoned: {e}")))?;

            match doc.apply(&operation, &schema_info, &state.clock)? {
                ApplyResult::Applied => (false, None),
                ApplyResult::Conflict(info) => {
                    conflicts += 1;
                    (
                        true,
                        Some(ConflictInfoDto {
                            entity_id: info.entity_id.to_string(),
                            field: info.field,
                            contending_values: info
                                .contending_values
                                .iter()
                                .map(field_value_to_json)
                                .collect(),
                        }),
                    )
                }
            }
        };

        // Persist to store
        {
            let store = state
                .store
                .lock()
                .map_err(|e| ApiError::Internal(format!("store lock poisoned: {e}")))?;
            store.append_operation(&state.document_id, &operation)?;
        }

        results.push(SubmitOperationResponse {
            operation_id: operation.id,
            timestamp: operation.timestamp.to_string(),
            conflict,
            conflict_info,
        });
    }

    let total = results.len();
    Ok(Json(BatchOperationResponse {
        results,
        total,
        conflicts,
    }))
}

// ============================================================================
// State
// ============================================================================

/// Get the full document state.
pub async fn get_document_state(
    State(state): State<AppState>,
) -> Result<Json<DocumentStateResponse>, ApiError> {
    let doc = state
        .document
        .read()
        .map_err(|e| ApiError::Internal(format!("lock poisoned: {e}")))?;

    let mut entities = Vec::new();
    for (entity_id, entity) in &doc.entities {
        let mut fields = serde_json::Map::new();
        for (name, field_state) in &entity.fields {
            fields.insert(name.clone(), field_value_to_json(&field_state.value()));
        }

        entities.push(EntityStateResponse {
            entity_id: entity_id.to_string(),
            entity_type: entity.entity_type.clone(),
            fields,
            parent_id: entity.parent_id.as_ref().map(|p| p.to_string()),
            children: entity.children.values().map(|c| c.to_string()).collect(),
            tombstoned: entity.tombstone.value,
        });
    }

    let root_entities = doc.roots.values().map(|e| e.to_string()).collect();

    Ok(Json(DocumentStateResponse {
        entities,
        root_entities,
    }))
}

/// Get state for a specific entity.
pub async fn get_entity_state(
    State(state): State<AppState>,
    Path(entity_path): Path<String>,
) -> Result<Json<EntityStateResponse>, ApiError> {
    let doc = state
        .document
        .read()
        .map_err(|e| ApiError::Internal(format!("lock poisoned: {e}")))?;

    let entity_id = EntityId::new(&entity_path);
    let entity = doc
        .get_entity(&entity_id)
        .ok_or_else(|| ApiError::EntityNotFound(entity_path))?;

    let mut fields = serde_json::Map::new();
    for (name, field_state) in &entity.fields {
        fields.insert(name.clone(), field_value_to_json(&field_state.value()));
    }

    Ok(Json(EntityStateResponse {
        entity_id: entity_id.to_string(),
        entity_type: entity.entity_type.clone(),
        fields,
        parent_id: entity.parent_id.as_ref().map(|p| p.to_string()),
        children: entity.children.values().map(|c| c.to_string()).collect(),
        tombstoned: entity.tombstone.value,
    }))
}

// ============================================================================
// Milestones
// ============================================================================

/// Create a milestone (project to git).
pub async fn create_milestone(
    State(state): State<AppState>,
    _actor: Actor,
    Json(req): Json<CreateMilestoneRequest>,
) -> Result<Json<CreateMilestoneResponse>, ApiError> {
    let projector = state
        .projector
        .as_ref()
        .ok_or_else(|| ApiError::Internal("git projection not configured".to_string()))?;

    // Get operations since last milestone
    let stored_ops = {
        let store = state
            .store
            .lock()
            .map_err(|e| ApiError::Internal(format!("store lock poisoned: {e}")))?;

        let ops_since = store.operations_since_milestone(&state.document_id)?;
        let query = OperationQuery::new(&state.document_id).limit(ops_since as u32);
        store.query_operations(&query)?
    };

    let operations: Vec<_> = stored_ops.into_iter().map(|s| s.operation).collect();

    // Get current document state
    let doc = state
        .document
        .read()
        .map_err(|e| ApiError::Internal(format!("lock poisoned: {e}")))?;

    // Determine environments
    let environments = if req.environments.is_empty() {
        vec!["production".to_string()]
    } else {
        req.environments
    };

    // Project to git with environment-aware field resolution
    let schema_info = state.schema.as_schema_info();
    let result = projector.project(&doc, operations, &environments, &req.message, &schema_info)?;

    // Record milestone in store
    let milestone_id = {
        let store = state
            .store
            .lock()
            .map_err(|e| ApiError::Internal(format!("store lock poisoned: {e}")))?;

        if let (Some(start), Some(end)) = (result.hlc_start, result.hlc_end) {
            store.record_milestone(
                &state.document_id,
                Some(&result.commit_sha),
                &start,
                &end,
                Some(&req.message),
            )?
        } else {
            // No operations, create a placeholder range
            let now = state.clock.new_timestamp();
            store.record_milestone(
                &state.document_id,
                Some(&result.commit_sha),
                &now,
                &now,
                Some(&req.message),
            )?
        }
    };

    Ok(Json(CreateMilestoneResponse {
        milestone_id,
        git_commit: Some(result.commit_sha),
        operation_count: result.operation_count,
        files_written: result.files_written,
    }))
}

/// List milestones.
pub async fn list_milestones(
    State(state): State<AppState>,
) -> Result<Json<Vec<MilestoneSummary>>, ApiError> {
    let milestones = {
        let store = state
            .store
            .lock()
            .map_err(|e| ApiError::Internal(format!("store lock poisoned: {e}")))?;
        store.list_milestones(&state.document_id)?
    };

    let summaries = milestones
        .into_iter()
        .map(|m| MilestoneSummary {
            id: m.id,
            message: m.message,
            git_commit: m.git_commit,
            created_at: m.created_at,
            hlc_range_start: m.hlc_range_start.to_string(),
            hlc_range_end: m.hlc_range_end.to_string(),
        })
        .collect();

    Ok(Json(summaries))
}

/// Get milestone details.
pub async fn get_milestone(
    State(state): State<AppState>,
    Path(milestone_id): Path<String>,
) -> Result<Json<MilestoneSummary>, ApiError> {
    let milestones = {
        let store = state
            .store
            .lock()
            .map_err(|e| ApiError::Internal(format!("store lock poisoned: {e}")))?;
        store.list_milestones(&state.document_id)?
    };

    let milestone = milestones
        .into_iter()
        .find(|m| m.id.to_string() == milestone_id)
        .ok_or_else(|| ApiError::EntityNotFound(format!("milestone {milestone_id}")))?;

    Ok(Json(MilestoneSummary {
        id: milestone.id,
        message: milestone.message,
        git_commit: milestone.git_commit,
        created_at: milestone.created_at,
        hlc_range_start: milestone.hlc_range_start.to_string(),
        hlc_range_end: milestone.hlc_range_end.to_string(),
    }))
}

// ============================================================================
// Log
// ============================================================================

/// Get operation log.
pub async fn get_operation_log(
    State(state): State<AppState>,
    Query(params): Query<LogQueryParams>,
) -> Result<Json<OperationLogResponse>, ApiError> {
    let (stored_ops, total) = {
        let store = state
            .store
            .lock()
            .map_err(|e| ApiError::Internal(format!("store lock poisoned: {e}")))?;

        let mut query = OperationQuery::new(&state.document_id);

        if let Some(ref entity_id) = params.entity_id {
            query = query.for_entity(entity_id);
        }
        if let Some(ref actor_id) = params.actor_id {
            query = query.by_actor(actor_id);
        }
        if let Some(limit) = params.limit {
            query = query.limit(limit);
        }

        let stored_ops = store.query_operations(&query)?;
        let total = store.operation_count(&state.document_id)?;
        (stored_ops, total)
    };

    let limit = params.limit;
    let operations: Vec<_> = stored_ops
        .into_iter()
        .map(|stored| {
            let op = stored.operation;
            OperationLogEntry {
                id: op.id,
                timestamp: op.timestamp.to_string(),
                actor_id: op.actor.id.clone(),
                actor_class: op.actor.class.to_string(),
                intent: op.intent.clone(),
                operation: operation_kind_to_log(&op.kind),
            }
        })
        .collect();

    let has_more = limit.is_some_and(|l| operations.len() >= l as usize);

    Ok(Json(OperationLogResponse {
        total,
        has_more,
        operations,
    }))
}

/// Get operations for a specific entity.
pub async fn get_entity_log(
    State(state): State<AppState>,
    Path(entity_path): Path<String>,
    Query(params): Query<LogQueryParams>,
) -> Result<Json<OperationLogResponse>, ApiError> {
    let stored_ops = {
        let store = state
            .store
            .lock()
            .map_err(|e| ApiError::Internal(format!("store lock poisoned: {e}")))?;

        let mut query = OperationQuery::new(&state.document_id).for_entity(&entity_path);

        if let Some(ref actor_id) = params.actor_id {
            query = query.by_actor(actor_id);
        }
        if let Some(limit) = params.limit {
            query = query.limit(limit);
        }

        store.query_operations(&query)?
    };

    let limit = params.limit;
    let operations: Vec<_> = stored_ops
        .into_iter()
        .map(|stored| {
            let op = stored.operation;
            OperationLogEntry {
                id: op.id,
                timestamp: op.timestamp.to_string(),
                actor_id: op.actor.id.clone(),
                actor_class: op.actor.class.to_string(),
                intent: op.intent.clone(),
                operation: operation_kind_to_log(&op.kind),
            }
        })
        .collect();

    let total = operations.len() as u64;
    let has_more = limit.is_some_and(|l| operations.len() >= l as usize);

    Ok(Json(OperationLogResponse {
        operations,
        total,
        has_more,
    }))
}

// ============================================================================
// Environments
// ============================================================================

/// List available environments.
pub async fn list_environments(
    State(state): State<AppState>,
) -> Result<Json<EnvironmentsResponse>, ApiError> {
    let schema_info = state.schema.as_schema_info();

    let base = schema_info.base_environment().map(|s| s.to_string());

    // Build list of all environments and inheritance map
    let mut environments = Vec::new();
    let mut inheritance = std::collections::HashMap::new();

    if let Some(ref base_env) = base {
        environments.push(base_env.clone());
    }

    // Get overlays from schema
    if let Some(ref envs) = state.schema.environments {
        for overlay in &envs.overlays {
            environments.push(overlay.name.clone());
            inheritance.insert(overlay.name.clone(), overlay.inherits.clone());
        }
    }

    Ok(Json(EnvironmentsResponse {
        base,
        environments,
        inheritance,
    }))
}

/// Get document state resolved for a specific environment.
pub async fn get_environment_state(
    State(state): State<AppState>,
    Path(env_name): Path<String>,
) -> Result<Json<EnvironmentStateResponse>, ApiError> {
    let schema_info = state.schema.as_schema_info();

    // Verify environment exists
    if !schema_info.has_environment(&env_name) {
        return Err(ApiError::EntityNotFound(format!("environment {env_name}")));
    }

    let doc = state
        .document
        .read()
        .map_err(|e| ApiError::Internal(format!("lock poisoned: {e}")))?;

    let mut entities = Vec::new();
    for (entity_id, entity) in &doc.entities {
        let mut fields = serde_json::Map::new();
        let mut field_sources = std::collections::HashMap::new();

        // Get all field names for this entity type
        if let Some(field_names) = schema_info.entity_fields(&entity.entity_type) {
            for field_name in field_names {
                // Resolve field value for this environment
                if let Some(value) = doc.get_field_for_env(entity_id, field_name, &env_name, &schema_info) {
                    fields.insert(field_name.to_string(), field_value_to_json(&value));

                    // Determine source (which env the value came from)
                    let source = find_field_source(entity, field_name, &env_name, &schema_info);
                    field_sources.insert(field_name.to_string(), source);
                }
            }
        }

        entities.push(EnvironmentEntityStateResponse {
            entity_id: entity_id.to_string(),
            entity_type: entity.entity_type.clone(),
            fields,
            field_sources,
            parent_id: entity.parent_id.as_ref().map(|p| p.to_string()),
            children: entity.children.values().map(|c| c.to_string()).collect(),
            tombstoned: entity.tombstone.value,
        });
    }

    let root_entities = doc.roots.values().map(|e| e.to_string()).collect();

    Ok(Json(EnvironmentStateResponse {
        environment: env_name,
        entities,
        root_entities,
    }))
}

/// Compare field values between two environments.
pub async fn diff_environments(
    State(state): State<AppState>,
    Query(params): Query<EnvDiffParams>,
) -> Result<Json<EnvironmentDiffResponse>, ApiError> {
    let schema_info = state.schema.as_schema_info();

    // Verify both environments exist
    if !schema_info.has_environment(&params.from) {
        return Err(ApiError::EntityNotFound(format!(
            "environment {}",
            params.from
        )));
    }
    if !schema_info.has_environment(&params.to) {
        return Err(ApiError::EntityNotFound(format!(
            "environment {}",
            params.to
        )));
    }

    let doc = state
        .document
        .read()
        .map_err(|e| ApiError::Internal(format!("lock poisoned: {e}")))?;

    let mut differences = Vec::new();

    for (entity_id, entity) in &doc.entities {
        if let Some(field_names) = schema_info.entity_fields(&entity.entity_type) {
            for field_name in field_names {
                let from_value = doc.get_field_for_env(entity_id, field_name, &params.from, &schema_info);
                let to_value = doc.get_field_for_env(entity_id, field_name, &params.to, &schema_info);

                // Check if values differ
                if from_value != to_value {
                    differences.push(FieldDiff {
                        entity_id: entity_id.to_string(),
                        field: field_name.to_string(),
                        from_value: from_value.map(|v| field_value_to_json(&v)),
                        to_value: to_value.map(|v| field_value_to_json(&v)),
                    });
                }
            }
        }
    }

    Ok(Json(EnvironmentDiffResponse {
        from_env: params.from,
        to_env: params.to,
        differences,
    }))
}

/// Helper to find which environment a field value came from.
fn find_field_source(
    entity: &conflux_core::Entity,
    field_name: &str,
    env: &str,
    schema: &dyn SchemaInfo,
) -> String {
    // Walk up the environment inheritance chain
    let mut current_env = Some(env);
    while let Some(e) = current_env {
        if entity.get_override(e, field_name).is_some() {
            return e.to_string();
        }
        current_env = schema.environment_parent(e);
    }
    // Fall back to base
    "base".to_string()
}

// ============================================================================
// Webhooks
// ============================================================================

/// Register a new webhook.
pub async fn register_webhook(
    State(state): State<AppState>,
    Json(req): Json<RegisterWebhookRequest>,
) -> Result<Json<RegisterWebhookResponse>, ApiError> {
    let mut config = WebhookConfig::new(req.url.clone());

    if let Some(filter) = req.entity_filter {
        config = config.with_entity_filter(filter);
    }
    if let Some(filter) = req.field_filter {
        config = config.with_field_filter(filter);
    }
    if let Some(secret) = req.secret {
        config = config.with_secret(secret);
    }
    if let Some(event_filter) = req.event_filter {
        let filter = match event_filter.to_lowercase().as_str() {
            "state_changes" => WebhookEventFilter::StateChanges,
            "conflicts" => WebhookEventFilter::Conflicts,
            _ => WebhookEventFilter::All,
        };
        config = config.with_event_filter(filter);
    }
    if let Some(format) = req.format {
        let fmt = match format.to_lowercase().as_str() {
            "gitops" => WebhookFormat::GitOps,
            _ => WebhookFormat::Conflux,
        };
        config = config.with_format(fmt);
    }

    let id = state.webhooks.register(config).await;

    Ok(Json(RegisterWebhookResponse {
        id,
        url: req.url,
        enabled: true,
    }))
}

/// List all webhooks.
pub async fn list_webhooks(
    State(state): State<AppState>,
) -> Result<Json<ListWebhooksResponse>, ApiError> {
    let webhooks = state.webhooks.list().await;
    let total = webhooks.len();

    let dtos: Vec<WebhookDto> = webhooks
        .into_iter()
        .map(|w| WebhookDto {
            id: w.config.id,
            url: w.config.url,
            entity_filter: w.config.entity_filter,
            field_filter: w.config.field_filter,
            enabled: w.config.enabled,
            event_filter: match w.config.event_filter {
                WebhookEventFilter::All => "all".to_string(),
                WebhookEventFilter::StateChanges => "state_changes".to_string(),
                WebhookEventFilter::Conflicts => "conflicts".to_string(),
            },
            format: match w.config.format {
                WebhookFormat::Conflux => "conflux".to_string(),
                WebhookFormat::GitOps => "gitops".to_string(),
            },
            failure_count: w.failure_count,
            last_error: w.last_error,
        })
        .collect();

    Ok(Json(ListWebhooksResponse {
        webhooks: dtos,
        total,
    }))
}

/// Get a webhook by ID.
pub async fn get_webhook(
    State(state): State<AppState>,
    Path(webhook_id): Path<String>,
) -> Result<Json<WebhookDto>, ApiError> {
    let id = Uuid::parse_str(&webhook_id)
        .map_err(|_| ApiError::EntityNotFound(format!("invalid webhook id: {webhook_id}")))?;

    let webhook = state
        .webhooks
        .get(&id)
        .await
        .ok_or_else(|| ApiError::EntityNotFound(format!("webhook {webhook_id}")))?;

    Ok(Json(WebhookDto {
        id: webhook.config.id,
        url: webhook.config.url,
        entity_filter: webhook.config.entity_filter,
        field_filter: webhook.config.field_filter,
        enabled: webhook.config.enabled,
        event_filter: match webhook.config.event_filter {
            WebhookEventFilter::All => "all".to_string(),
            WebhookEventFilter::StateChanges => "state_changes".to_string(),
            WebhookEventFilter::Conflicts => "conflicts".to_string(),
        },
        format: match webhook.config.format {
            WebhookFormat::Conflux => "conflux".to_string(),
            WebhookFormat::GitOps => "gitops".to_string(),
        },
        failure_count: webhook.failure_count,
        last_error: webhook.last_error,
    }))
}

/// Delete a webhook.
pub async fn delete_webhook(
    State(state): State<AppState>,
    Path(webhook_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let id = Uuid::parse_str(&webhook_id)
        .map_err(|_| ApiError::EntityNotFound(format!("invalid webhook id: {webhook_id}")))?;

    let removed = state.webhooks.unregister(&id).await;

    if removed {
        Ok(Json(serde_json::json!({
            "deleted": true,
            "id": webhook_id
        })))
    } else {
        Err(ApiError::EntityNotFound(format!("webhook {webhook_id}")))
    }
}

/// Update a webhook (placeholder - updates require re-registration for now).
pub async fn update_webhook(
    State(state): State<AppState>,
    Path(webhook_id): Path<String>,
    Json(req): Json<UpdateWebhookRequest>,
) -> Result<Json<WebhookDto>, ApiError> {
    let id = Uuid::parse_str(&webhook_id)
        .map_err(|_| ApiError::EntityNotFound(format!("invalid webhook id: {webhook_id}")))?;

    // Get existing webhook
    let webhook = state
        .webhooks
        .get(&id)
        .await
        .ok_or_else(|| ApiError::EntityNotFound(format!("webhook {webhook_id}")))?;

    // Build updated config
    let mut config = webhook.config.clone();
    config.id = id; // Preserve the ID

    if let Some(url) = req.url {
        config.url = url;
    }
    if let Some(enabled) = req.enabled {
        config.enabled = enabled;
    }
    if let Some(filter) = req.entity_filter {
        config.entity_filter = if filter.is_empty() { None } else { Some(filter) };
    }
    if let Some(filter) = req.field_filter {
        config.field_filter = if filter.is_empty() { None } else { Some(filter) };
    }
    if let Some(event_filter) = req.event_filter {
        config.event_filter = match event_filter.to_lowercase().as_str() {
            "state_changes" => WebhookEventFilter::StateChanges,
            "conflicts" => WebhookEventFilter::Conflicts,
            _ => WebhookEventFilter::All,
        };
    }
    if let Some(format) = req.format {
        config.format = match format.to_lowercase().as_str() {
            "gitops" => WebhookFormat::GitOps,
            _ => WebhookFormat::Conflux,
        };
    }

    // Unregister old, register updated
    state.webhooks.unregister(&id).await;
    state.webhooks.register(config.clone()).await;

    Ok(Json(WebhookDto {
        id: config.id,
        url: config.url,
        entity_filter: config.entity_filter,
        field_filter: config.field_filter,
        enabled: config.enabled,
        event_filter: match config.event_filter {
            WebhookEventFilter::All => "all".to_string(),
            WebhookEventFilter::StateChanges => "state_changes".to_string(),
            WebhookEventFilter::Conflicts => "conflicts".to_string(),
        },
        format: match config.format {
            WebhookFormat::Conflux => "conflux".to_string(),
            WebhookFormat::GitOps => "gitops".to_string(),
        },
        failure_count: 0,
        last_error: None,
    }))
}

// ============================================================================
// Audit
// ============================================================================

/// Export audit log as JSON Lines (NDJSON).
///
/// Returns a streaming response with content-type application/x-ndjson.
pub async fn export_audit(
    State(state): State<AppState>,
    Query(params): Query<crate::dto::AuditExportParams>,
) -> Result<axum::response::Response, ApiError> {
    use axum::body::Body;
    use axum::http::header;
    use conflux_store::audit::{AuditExportQuery, AuditExporter};

    // Build the query
    let mut query = AuditExportQuery::new(&state.document_id);

    if let Some(ref since) = params.since {
        query = query.since_wall_clock(since);
    }
    if let Some(ref until) = params.until {
        query = query.until_wall_clock(until);
    }
    if let Some(ref actor) = params.actor {
        query = query.by_actor(actor);
    }
    if let Some(ref entity) = params.entity {
        query = query.for_entity(entity);
    }
    if params.verify {
        query = query.verify();
    }

    // Get access to the store
    let store = state
        .store
        .lock()
        .map_err(|e| ApiError::Internal(format!("lock poisoned: {e}")))?;

    // Create exporter and export to buffer
    let exporter = AuditExporter::new(&*store);

    let mut output = Vec::new();
    let stats = exporter
        .export(&query, &mut output)
        .map_err(|e| ApiError::Internal(format!("audit export failed: {e}")))?;

    // Build response with NDJSON content type
    let response = axum::response::Response::builder()
        .status(axum::http::StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .header("X-Audit-Operations-Exported", stats.operations_exported.to_string())
        .header("X-Audit-Signature-Failures", stats.signature_failures.to_string())
        .body(Body::from(output))
        .map_err(|e| ApiError::Internal(format!("response build failed: {e}")))?;

    Ok(response)
}
