//! gRPC service implementation for Conflux.
//!
//! Provides streaming state watching and operation submission over gRPC.

use crate::state::AppState;
use conflux_core::{ActorClass, ActorId, Clock, Document, EntityId, FieldValue, Operation};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use tonic::{Request, Response, Status};

/// Generated protobuf types.
pub mod pb {
    tonic::include_proto!("conflux.v1");
}

use pb::conflux_service_server::ConfluxService;
use pb::{
    ChangeType, GetEntityRequest, GetEntityResponse, GetStateRequest, GetStateResponse,
    StateChange, SubmitRequest, SubmitResponse, WatchRequest,
};

/// State change event for broadcasting to watchers.
#[derive(Debug, Clone)]
pub struct StateChangeEvent {
    pub operation: Operation,
    pub has_conflict: bool,
}

/// gRPC service implementation.
pub struct ConfluxGrpcService {
    state: Arc<AppState>,
    clock: Clock,
    tx: broadcast::Sender<StateChangeEvent>,
}

impl ConfluxGrpcService {
    /// Creates a new gRPC service instance.
    pub fn new(state: Arc<AppState>, tx: broadcast::Sender<StateChangeEvent>) -> Self {
        Self {
            state,
            clock: Clock::new(),
            tx,
        }
    }

    /// Broadcasts a state change to all watchers.
    pub fn broadcast(&self, event: StateChangeEvent) {
        // Ignore send errors (no receivers)
        let _ = self.tx.send(event);
    }

    fn operation_to_state_change(op: &Operation, _has_conflict: bool) -> StateChange {
        use conflux_core::OperationKind;

        let (change_type, entity_id, field, value_json) = match &op.kind {
            OperationKind::SetField {
                entity_id,
                field,
                value,
            } => (
                ChangeType::SetField,
                entity_id.to_string(),
                Some(field.clone()),
                Some(serde_json::to_string(value).unwrap_or_default()),
            ),
            OperationKind::InsertEntity { entity_id, .. } => (
                ChangeType::InsertEntity,
                entity_id.to_string(),
                None,
                None,
            ),
            OperationKind::RemoveEntity { entity_id } => (
                ChangeType::RemoveEntity,
                entity_id.to_string(),
                None,
                None,
            ),
            OperationKind::MoveEntity { entity_id, .. } => (
                ChangeType::MoveEntity,
                entity_id.to_string(),
                None,
                None,
            ),
            OperationKind::SetOverride {
                entity_id,
                field,
                value,
                ..
            } => (
                ChangeType::SetOverride,
                entity_id.to_string(),
                Some(field.clone()),
                Some(serde_json::to_string(value).unwrap_or_default()),
            ),
            OperationKind::ResolveConflict {
                entity_id,
                field,
                chosen_value,
                ..
            } => (
                ChangeType::ResolveConflict,
                entity_id.to_string(),
                Some(field.clone()),
                Some(serde_json::to_string(chosen_value).unwrap_or_default()),
            ),
        };

        StateChange {
            change_type: change_type.into(),
            entity_id,
            field,
            value_json,
            actor_id: op.actor.id.clone(),
            actor_class: op.actor.class.to_string(),
            timestamp: op.timestamp.to_string(),
            intent: op.intent.clone(),
            operation_id: op.id.to_string(),
        }
    }
}

#[tonic::async_trait]
impl ConfluxService for ConfluxGrpcService {
    type WatchStateStream =
        Pin<Box<dyn Stream<Item = Result<StateChange, Status>> + Send + 'static>>;

    async fn watch_state(
        &self,
        request: Request<WatchRequest>,
    ) -> Result<Response<Self::WatchStateStream>, Status> {
        let req = request.into_inner();
        let entity_filter = req.entity_filter;
        let field_filter = req.field_filter;

        // Subscribe to broadcast channel
        let rx = self.tx.subscribe();

        // Convert broadcast receiver to a stream with filtering
        let stream = BroadcastStream::new(rx)
            .filter_map(move |result| {
                let entity_filter = entity_filter.clone();
                let field_filter = field_filter.clone();

                match result {
                    Ok(event) => {
                        let change = ConfluxGrpcService::operation_to_state_change(
                            &event.operation,
                            event.has_conflict,
                        );

                        // Apply filters
                        if let Some(ref filter) = entity_filter {
                            if !change.entity_id.starts_with(filter) {
                                return None;
                            }
                        }
                        if let Some(ref filter) = field_filter {
                            if change.field.as_ref().map(|f| f != filter).unwrap_or(true) {
                                return None;
                            }
                        }

                        Some(Ok(change))
                    }
                    Err(_) => None, // Lagged, skip
                }
            });

        Ok(Response::new(Box::pin(stream)))
    }

    async fn submit_operation(
        &self,
        request: Request<SubmitRequest>,
    ) -> Result<Response<SubmitResponse>, Status> {
        let req = request.into_inner();

        let actor = ActorId::new(
            &req.actor_id,
            parse_actor_class(&req.actor_class)
                .map_err(|e| Status::invalid_argument(e.to_string()))?,
        );

        let timestamp = self.clock.new_timestamp();

        // Build operation based on type
        let mut op = match req.operation {
            Some(pb::submit_request::Operation::SetField(set_op)) => {
                let value = parse_json_value(&set_op.value_json)
                    .map_err(|e| Status::invalid_argument(format!("invalid value: {}", e)))?;
                Operation::set_field(
                    EntityId::new(&set_op.entity_id),
                    &set_op.field,
                    value,
                    &actor,
                    timestamp,
                )
            }
            Some(pb::submit_request::Operation::InsertEntity(insert_op)) => {
                // Note: initial_fields_json in proto is ignored for now
                // Core operation doesn't support initial fields in InsertEntity
                Operation::insert_entity(
                    EntityId::new(&insert_op.entity_id),
                    &insert_op.entity_type,
                    insert_op.parent_id.map(|p| EntityId::new(&p)),
                    insert_op.position.map(|p| p.to_string()),
                    &actor,
                    timestamp,
                )
            }
            Some(pb::submit_request::Operation::RemoveEntity(remove_op)) => {
                Operation::remove_entity(EntityId::new(&remove_op.entity_id), &actor, timestamp)
            }
            Some(pb::submit_request::Operation::SetOverride(override_op)) => {
                let value = parse_json_value(&override_op.value_json)
                    .map_err(|e| Status::invalid_argument(format!("invalid value: {}", e)))?;
                Operation::set_override(
                    EntityId::new(&override_op.entity_id),
                    &override_op.field,
                    &override_op.environment,
                    value,
                    &actor,
                    timestamp,
                )
            }
            Some(pb::submit_request::Operation::ResolveConflict(resolve_op)) => {
                let value = parse_json_value(&resolve_op.value_json)
                    .map_err(|e| Status::invalid_argument(format!("invalid value: {}", e)))?;
                Operation::resolve_conflict(
                    EntityId::new(&resolve_op.entity_id),
                    &resolve_op.field,
                    resolve_op.environment,
                    value,
                    &actor,
                    timestamp,
                )
            }
            None => return Err(Status::invalid_argument("operation required")),
        };

        if let Some(intent) = req.intent {
            op = op.with_intent(intent);
        }

        // Apply operation
        let schema_info = self.state.schema.as_schema_info();
        let mut document = Document::new();

        // Load current state
        let stored_ops = {
            let store = self.state.store.lock().map_err(|_| Status::internal("lock error"))?;
            store
                .query_operations(
                    &conflux_store::OperationQuery::new(&self.state.document_id).limit(10000),
                )
                .map_err(|e| Status::internal(format!("store error: {}", e)))?
        };

        for stored in stored_ops {
            let _ = document.apply(&stored.operation, &schema_info, &self.clock);
        }

        // Apply new operation
        let result = document
            .apply(&op, &schema_info, &self.clock)
            .map_err(|e| Status::invalid_argument(format!("apply error: {}", e)))?;

        let has_conflict = matches!(result, conflux_core::ApplyResult::Conflict(_));

        // Store operation
        {
            let store = self.state.store.lock().map_err(|_| Status::internal("lock error"))?;
            store
                .append_operation(&self.state.document_id, &op)
                .map_err(|e| Status::internal(format!("store error: {}", e)))?;
        }

        // Broadcast change
        self.broadcast(StateChangeEvent {
            operation: op.clone(),
            has_conflict,
        });

        Ok(Response::new(SubmitResponse {
            operation_id: op.id.to_string(),
            timestamp: op.timestamp.to_string(),
            has_conflict,
        }))
    }

    async fn get_state(
        &self,
        request: Request<GetStateRequest>,
    ) -> Result<Response<GetStateResponse>, Status> {
        let _req = request.into_inner();

        let schema_info = self.state.schema.as_schema_info();
        let mut document = Document::new();
        let clock = Clock::new();

        // Load current state
        let stored_ops = {
            let store = self.state.store.lock().map_err(|_| Status::internal("lock error"))?;
            store
                .query_operations(
                    &conflux_store::OperationQuery::new(&self.state.document_id).limit(10000),
                )
                .map_err(|e| Status::internal(format!("store error: {}", e)))?
        };

        let mut latest_timestamp = None;
        for stored in stored_ops {
            latest_timestamp = Some(stored.operation.timestamp.to_string());
            let _ = document.apply(&stored.operation, &schema_info, &clock);
        }

        // Serialize state (note: environment resolution is deferred to milestone projection)
        let state_json = serde_json::to_string(&document)
            .map_err(|e| Status::internal(format!("serialize error: {}", e)))?;

        Ok(Response::new(GetStateResponse {
            state_json,
            latest_timestamp: latest_timestamp.unwrap_or_default(),
        }))
    }

    async fn get_entity(
        &self,
        request: Request<GetEntityRequest>,
    ) -> Result<Response<GetEntityResponse>, Status> {
        let req = request.into_inner();
        let entity_id = EntityId::new(&req.entity_id);

        let schema_info = self.state.schema.as_schema_info();
        let mut document = Document::new();
        let clock = Clock::new();

        // Load current state
        let stored_ops = {
            let store = self.state.store.lock().map_err(|_| Status::internal("lock error"))?;
            store
                .query_operations(
                    &conflux_store::OperationQuery::new(&self.state.document_id).limit(10000),
                )
                .map_err(|e| Status::internal(format!("store error: {}", e)))?
        };

        for stored in stored_ops {
            let _ = document.apply(&stored.operation, &schema_info, &clock);
        }

        // Get entity
        let entity = document.get_entity(&entity_id);
        match entity {
            Some(ent) => {
                let entity_json = serde_json::to_string(ent)
                    .map_err(|e| Status::internal(format!("serialize error: {}", e)))?;
                Ok(Response::new(GetEntityResponse {
                    entity_json,
                    exists: true,
                }))
            }
            None => Ok(Response::new(GetEntityResponse {
                entity_json: "null".to_string(),
                exists: false,
            })),
        }
    }
}

fn parse_actor_class(s: &str) -> Result<ActorClass, &'static str> {
    match s.to_lowercase().as_str() {
        "human" => Ok(ActorClass::Human),
        "pipeline" => Ok(ActorClass::Pipeline),
        "operator" => Ok(ActorClass::Operator),
        "system" => Ok(ActorClass::System),
        _ => Err("invalid actor class"),
    }
}

fn parse_json_value(s: &str) -> Result<FieldValue, serde_json::Error> {
    let json: serde_json::Value = serde_json::from_str(s)?;
    Ok(json_to_field_value(&json))
}

fn json_to_field_value(json: &serde_json::Value) -> FieldValue {
    match json {
        serde_json::Value::Null => FieldValue::Null,
        serde_json::Value::Bool(b) => FieldValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                FieldValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                FieldValue::Float(f)
            } else {
                FieldValue::Null
            }
        }
        serde_json::Value::String(s) => FieldValue::String(s.clone()),
        serde_json::Value::Array(arr) => {
            FieldValue::List(arr.iter().map(json_to_field_value).collect())
        }
        serde_json::Value::Object(obj) => {
            let map: std::collections::BTreeMap<String, FieldValue> = obj
                .iter()
                .map(|(k, v)| (k.clone(), json_to_field_value(v)))
                .collect();
            FieldValue::Map(map)
        }
    }
}
