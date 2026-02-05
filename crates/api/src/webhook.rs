//! Webhook infrastructure for notifying external systems of state changes.

use crate::grpc::StateChangeEvent;
use conflux_core::entity::ConflictInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{error, info, warn};
use uuid::Uuid;

/// Types of events that webhooks can subscribe to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WebhookEventFilter {
    /// All events (default).
    #[default]
    All,
    /// Only state change events (operations).
    StateChanges,
    /// Only conflict events.
    Conflicts,
}

/// Format for webhook payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WebhookFormat {
    /// Default Conflux format.
    #[default]
    Conflux,
    /// GitOps-compatible format (Flux/ArgoCD).
    GitOps,
}

/// GitOps-compatible event payload for Flux and ArgoCD reconcilers.
///
/// This format follows conventions expected by GitOps tools:
/// - `kind` identifies the event type (similar to Kubernetes events)
/// - `source` identifies the system
/// - `involvedObject` contains entity/resource reference
/// - `reason` and `message` describe what happened
/// - `metadata` contains additional context
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitOpsPayload {
    /// Event type identifier.
    pub kind: String,

    /// API version for compatibility.
    pub api_version: String,

    /// Source of the event.
    pub source: GitOpsSource,

    /// The object involved in the event.
    pub involved_object: GitOpsInvolvedObject,

    /// Short reason code (e.g., "Updated", "Created", "Conflict").
    pub reason: String,

    /// Human-readable description.
    pub message: String,

    /// Event metadata.
    pub metadata: GitOpsMetadata,
}

/// Source information for GitOps events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitOpsSource {
    /// Component that generated the event.
    pub component: String,

    /// Host/node identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

/// Reference to the involved object for GitOps events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitOpsInvolvedObject {
    /// Kind of object (entity type).
    pub kind: String,

    /// Name/ID of the object.
    pub name: String,

    /// Namespace (environment).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    /// Field path within the object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_path: Option<String>,
}

/// Metadata for GitOps events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitOpsMetadata {
    /// Event timestamp (RFC3339).
    pub timestamp: String,

    /// Operation ID for tracing.
    pub operation_id: String,

    /// Actor who caused the event.
    pub actor: String,

    /// Actor class (human, operator, pipeline, system).
    pub actor_class: String,

    /// HLC timestamp for causal ordering.
    pub hlc_timestamp: String,

    /// Whether the event resulted in a conflict.
    pub has_conflict: bool,

    /// Labels for filtering/selection.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub labels: HashMap<String, String>,
}

/// Webhook configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// Unique webhook ID.
    pub id: Uuid,

    /// URL to deliver events to.
    pub url: String,

    /// Optional filter by entity ID prefix.
    pub entity_filter: Option<String>,

    /// Optional filter by field name.
    pub field_filter: Option<String>,

    /// Optional secret for HMAC signing.
    pub secret: Option<String>,

    /// Whether the webhook is enabled.
    pub enabled: bool,

    /// Event filter - which types of events to deliver.
    #[serde(default)]
    pub event_filter: WebhookEventFilter,

    /// Payload format (conflux or gitops).
    #[serde(default)]
    pub format: WebhookFormat,
}

impl WebhookConfig {
    /// Creates a new webhook configuration.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            url: url.into(),
            entity_filter: None,
            field_filter: None,
            secret: None,
            enabled: true,
            event_filter: WebhookEventFilter::All,
            format: WebhookFormat::Conflux,
        }
    }

    /// Sets an entity filter.
    pub fn with_entity_filter(mut self, filter: impl Into<String>) -> Self {
        self.entity_filter = Some(filter.into());
        self
    }

    /// Sets a field filter.
    pub fn with_field_filter(mut self, filter: impl Into<String>) -> Self {
        self.field_filter = Some(filter.into());
        self
    }

    /// Sets a secret for HMAC signing.
    pub fn with_secret(mut self, secret: impl Into<String>) -> Self {
        self.secret = Some(secret.into());
        self
    }

    /// Sets the event filter.
    pub fn with_event_filter(mut self, filter: WebhookEventFilter) -> Self {
        self.event_filter = filter;
        self
    }

    /// Sets the payload format.
    pub fn with_format(mut self, format: WebhookFormat) -> Self {
        self.format = format;
        self
    }
}

/// A webhook delivery payload.
#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayload {
    /// Event type.
    pub event_type: String,

    /// Entity ID.
    pub entity_id: String,

    /// Field name (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,

    /// New value as JSON string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_json: Option<String>,

    /// Actor ID.
    pub actor_id: String,

    /// Actor class.
    pub actor_class: String,

    /// HLC timestamp.
    pub timestamp: String,

    /// Intent (if provided).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,

    /// Operation ID.
    pub operation_id: String,

    /// Whether this operation resulted in a conflict.
    pub has_conflict: bool,
}

/// A conflict notification payload.
#[derive(Debug, Clone, Serialize)]
pub struct ConflictPayload {
    /// Event type - always "conflict_detected".
    pub event_type: String,

    /// Entity ID with the conflict.
    pub entity_id: String,

    /// Field with the conflict.
    pub field: String,

    /// Environment (if conflict is in an override).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,

    /// Number of contending values.
    pub contending_value_count: usize,

    /// The contending values as JSON.
    pub contending_values: Vec<serde_json::Value>,

    /// Timestamp when the conflict was detected.
    pub detected_at: String,
}

/// Stored webhook entry.
#[derive(Debug, Clone)]
pub struct Webhook {
    /// Configuration.
    pub config: WebhookConfig,

    /// Number of consecutive failures.
    pub failure_count: u32,

    /// Last error message.
    pub last_error: Option<String>,
}

impl Webhook {
    /// Creates a new webhook from configuration.
    pub fn new(config: WebhookConfig) -> Self {
        Self {
            config,
            failure_count: 0,
            last_error: None,
        }
    }
}

/// Manages webhook registrations and delivery.
pub struct WebhookManager {
    webhooks: RwLock<HashMap<Uuid, Webhook>>,
    client: reqwest::Client,
}

impl WebhookManager {
    /// Creates a new webhook manager.
    pub fn new() -> Self {
        Self {
            webhooks: RwLock::new(HashMap::new()),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    /// Registers a new webhook.
    pub async fn register(&self, config: WebhookConfig) -> Uuid {
        let id = config.id;
        let webhook = Webhook::new(config);
        self.webhooks.write().await.insert(id, webhook);
        info!("Registered webhook {}", id);
        id
    }

    /// Unregisters a webhook.
    pub async fn unregister(&self, id: &Uuid) -> bool {
        let removed = self.webhooks.write().await.remove(id).is_some();
        if removed {
            info!("Unregistered webhook {}", id);
        }
        removed
    }

    /// Lists all webhooks.
    pub async fn list(&self) -> Vec<Webhook> {
        self.webhooks.read().await.values().cloned().collect()
    }

    /// Gets a webhook by ID.
    pub async fn get(&self, id: &Uuid) -> Option<Webhook> {
        self.webhooks.read().await.get(id).cloned()
    }

    /// Starts the webhook delivery loop.
    pub fn start_delivery_loop(
        self: Arc<Self>,
        mut rx: broadcast::Receiver<StateChangeEvent>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        self.deliver_event(&event).await;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Webhook delivery lagged by {} events", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Webhook delivery channel closed");
                        break;
                    }
                }
            }
        })
    }

    /// Delivers an event to all matching webhooks.
    async fn deliver_event(&self, event: &StateChangeEvent) {
        let webhooks = self.webhooks.read().await;
        let payload = Self::event_to_payload(event);

        for (id, webhook) in webhooks.iter() {
            if !webhook.config.enabled {
                continue;
            }

            // Check event filter
            match webhook.config.event_filter {
                WebhookEventFilter::Conflicts => continue, // State changes filtered out
                WebhookEventFilter::All | WebhookEventFilter::StateChanges => {}
            }

            // Apply filters
            if let Some(ref filter) = webhook.config.entity_filter {
                if !payload.entity_id.starts_with(filter) {
                    continue;
                }
            }
            if let Some(ref filter) = webhook.config.field_filter {
                if payload.field.as_ref().map(|f| f != filter).unwrap_or(true) {
                    continue;
                }
            }

            // Deliver with the appropriate format
            let id = *id;
            let url = webhook.config.url.clone();
            let client = self.client.clone();
            let format = webhook.config.format;

            // Convert to the appropriate payload format
            let json_payload: serde_json::Value = match format {
                WebhookFormat::Conflux => serde_json::to_value(&payload).unwrap_or_default(),
                WebhookFormat::GitOps => {
                    let gitops = Self::event_to_gitops_payload(event);
                    serde_json::to_value(&gitops).unwrap_or_default()
                }
            };

            // Fire and forget - don't block other deliveries
            tokio::spawn(async move {
                match client.post(&url).json(&json_payload).send().await {
                    Ok(response) => {
                        if response.status().is_success() {
                            info!("Delivered webhook {} to {}", id, url);
                        } else {
                            warn!(
                                "Webhook {} delivery failed: {} {}",
                                id,
                                response.status(),
                                response.text().await.unwrap_or_default()
                            );
                        }
                    }
                    Err(e) => {
                        error!("Webhook {} delivery error: {}", id, e);
                    }
                }
            });
        }
    }

    /// Delivers a conflict notification to all matching webhooks.
    pub async fn deliver_conflict(&self, conflict: &ConflictInfo) {
        let webhooks = self.webhooks.read().await;
        let payload = Self::conflict_to_payload(conflict);

        for (id, webhook) in webhooks.iter() {
            if !webhook.config.enabled {
                continue;
            }

            // Check event filter
            match webhook.config.event_filter {
                WebhookEventFilter::StateChanges => continue, // Conflicts filtered out
                WebhookEventFilter::All | WebhookEventFilter::Conflicts => {}
            }

            // Apply entity filter
            if let Some(ref filter) = webhook.config.entity_filter {
                if !payload.entity_id.starts_with(filter) {
                    continue;
                }
            }

            // Apply field filter
            if let Some(ref filter) = webhook.config.field_filter {
                if &payload.field != filter {
                    continue;
                }
            }

            // Deliver
            let id = *id;
            let url = webhook.config.url.clone();
            let payload = payload.clone();
            let client = self.client.clone();

            tokio::spawn(async move {
                match client.post(&url).json(&payload).send().await {
                    Ok(response) => {
                        if response.status().is_success() {
                            info!("Delivered conflict webhook {} to {}", id, url);
                        } else {
                            warn!(
                                "Conflict webhook {} delivery failed: {} {}",
                                id,
                                response.status(),
                                response.text().await.unwrap_or_default()
                            );
                        }
                    }
                    Err(e) => {
                        error!("Conflict webhook {} delivery error: {}", id, e);
                    }
                }
            });
        }
    }

    fn conflict_to_payload(conflict: &ConflictInfo) -> ConflictPayload {
        let contending_values: Vec<serde_json::Value> = conflict
            .contending_values
            .iter()
            .map(|v| serde_json::to_value(v).unwrap_or(serde_json::Value::Null))
            .collect();

        ConflictPayload {
            event_type: "conflict_detected".to_string(),
            entity_id: conflict.entity_id.to_string(),
            field: conflict.field.clone(),
            environment: conflict.environment.clone(),
            contending_value_count: conflict.contending_values.len(),
            contending_values,
            detected_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn event_to_payload(event: &StateChangeEvent) -> WebhookPayload {
        use conflux_core::OperationKind;

        let op = &event.operation;
        let (event_type, entity_id, field, value_json) = match &op.kind {
            OperationKind::SetField {
                entity_id,
                field,
                value,
            } => (
                "set_field",
                entity_id.to_string(),
                Some(field.clone()),
                Some(serde_json::to_string(value).unwrap_or_default()),
            ),
            OperationKind::InsertEntity { entity_id, .. } => {
                ("insert_entity", entity_id.to_string(), None, None)
            }
            OperationKind::RemoveEntity { entity_id } => {
                ("remove_entity", entity_id.to_string(), None, None)
            }
            OperationKind::MoveEntity { entity_id, .. } => {
                ("move_entity", entity_id.to_string(), None, None)
            }
            OperationKind::SetOverride {
                entity_id,
                field,
                value,
                ..
            } => (
                "set_override",
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
                "resolve_conflict",
                entity_id.to_string(),
                Some(field.clone()),
                Some(serde_json::to_string(chosen_value).unwrap_or_default()),
            ),
        };

        WebhookPayload {
            event_type: event_type.to_string(),
            entity_id,
            field,
            value_json,
            actor_id: op.actor.id.clone(),
            actor_class: op.actor.class.to_string(),
            timestamp: op.timestamp.to_string(),
            intent: op.intent.clone(),
            operation_id: op.id.to_string(),
            has_conflict: event.has_conflict,
        }
    }

    fn event_to_gitops_payload(event: &StateChangeEvent) -> GitOpsPayload {
        use conflux_core::OperationKind;

        let op = &event.operation;
        let (reason, message, entity_type, entity_id, field_path, namespace) = match &op.kind {
            OperationKind::SetField {
                entity_id, field, value, ..
            } => (
                "Updated",
                format!("Field '{}' updated to {:?}", field, value),
                "Entity",
                entity_id.to_string(),
                Some(format!(".{}", field)),
                None,
            ),
            OperationKind::InsertEntity {
                entity_id,
                entity_type,
                ..
            } => (
                "Created",
                format!("Entity '{}' of type '{}' created", entity_id, entity_type),
                entity_type.as_str(),
                entity_id.to_string(),
                None,
                None,
            ),
            OperationKind::RemoveEntity { entity_id } => (
                "Deleted",
                format!("Entity '{}' deleted", entity_id),
                "Entity",
                entity_id.to_string(),
                None,
                None,
            ),
            OperationKind::MoveEntity {
                entity_id,
                new_parent_id,
                ..
            } => (
                "Moved",
                format!("Entity '{}' moved to '{}'", entity_id, new_parent_id),
                "Entity",
                entity_id.to_string(),
                None,
                None,
            ),
            OperationKind::SetOverride {
                entity_id,
                field,
                environment,
                value,
            } => (
                "OverrideUpdated",
                format!("Field '{}' override for '{}' set to {:?}", field, environment, value),
                "Entity",
                entity_id.to_string(),
                Some(format!(".{}", field)),
                Some(environment.clone()),
            ),
            OperationKind::ResolveConflict {
                entity_id,
                field,
                environment,
                ..
            } => {
                let env_str = environment.as_ref().map(|e| format!(" in {}", e)).unwrap_or_default();
                (
                    "ConflictResolved",
                    format!("Conflict on field '{}'{} resolved", field, env_str),
                    "Entity",
                    entity_id.to_string(),
                    Some(format!(".{}", field)),
                    environment.clone(),
                )
            }
        };

        let mut labels = HashMap::new();
        labels.insert("app.kubernetes.io/managed-by".to_string(), "conflux".to_string());
        if let Some(intent) = &op.intent {
            labels.insert("conflux.io/intent".to_string(), intent.clone());
        }
        if event.has_conflict {
            labels.insert("conflux.io/has-conflict".to_string(), "true".to_string());
        }

        GitOpsPayload {
            kind: "ConfluxEvent".to_string(),
            api_version: "conflux.io/v1".to_string(),
            source: GitOpsSource {
                component: "conflux".to_string(),
                host: None,
            },
            involved_object: GitOpsInvolvedObject {
                kind: entity_type.to_string(),
                name: entity_id,
                namespace,
                field_path,
            },
            reason: reason.to_string(),
            message,
            metadata: GitOpsMetadata {
                timestamp: chrono::Utc::now().to_rfc3339(),
                operation_id: op.id.to_string(),
                actor: op.actor.id.clone(),
                actor_class: op.actor.class.to_string(),
                hlc_timestamp: op.timestamp.to_string(),
                has_conflict: event.has_conflict,
                labels,
            },
        }
    }
}

impl Default for WebhookManager {
    fn default() -> Self {
        Self::new()
    }
}
