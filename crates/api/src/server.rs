//! HTTP server setup and routing.

use crate::handlers;
use crate::state::AppState;
use axum::routing::{delete, get, post, put};
use axum::Router;
use std::net::SocketAddr;
use tokio::net::TcpListener;

/// Default HTTP port.
pub const DEFAULT_HTTP_PORT: u16 = 9400;

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// HTTP listen address.
    pub http_addr: SocketAddr,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            http_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_HTTP_PORT)),
        }
    }
}

impl ServerConfig {
    /// Creates a new server config with the default HTTP port.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the HTTP listen address.
    pub fn with_http_addr(mut self, addr: SocketAddr) -> Self {
        self.http_addr = addr;
        self
    }

    /// Sets the HTTP port (binds to 0.0.0.0).
    pub fn with_http_port(mut self, port: u16) -> Self {
        self.http_addr = SocketAddr::from(([0, 0, 0, 0], port));
        self
    }
}

/// Builds the HTTP router with all routes.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        // Health
        .route("/health", get(handlers::health))
        // Operations
        .route("/v1/ops", post(handlers::submit_operation))
        .route("/v1/ops/batch", post(handlers::batch_operations))
        // State
        .route("/v1/state", get(handlers::get_document_state))
        .route("/v1/state/{entity_path}", get(handlers::get_entity_state))
        // Milestones
        .route("/v1/milestones", post(handlers::create_milestone))
        .route("/v1/milestones", get(handlers::list_milestones))
        .route(
            "/v1/milestones/{milestone_id}",
            get(handlers::get_milestone),
        )
        // Log
        .route("/v1/log", get(handlers::get_operation_log))
        .route("/v1/log/{entity_path}", get(handlers::get_entity_log))
        // Environments
        .route("/v1/envs", get(handlers::list_environments))
        .route("/v1/envs/diff", get(handlers::diff_environments))
        .route("/v1/envs/{env_name}/state", get(handlers::get_environment_state))
        // Webhooks
        .route("/v1/webhooks", post(handlers::register_webhook))
        .route("/v1/webhooks", get(handlers::list_webhooks))
        .route("/v1/webhooks/{webhook_id}", get(handlers::get_webhook))
        .route("/v1/webhooks/{webhook_id}", put(handlers::update_webhook))
        .route("/v1/webhooks/{webhook_id}", delete(handlers::delete_webhook))
        // Audit
        .route("/v1/audit/export", get(handlers::export_audit))
        // State
        .with_state(state)
}

/// Runs the HTTP server.
///
/// # Errors
///
/// Returns an error if binding or serving fails.
pub async fn run_server(config: ServerConfig, state: AppState) -> Result<(), std::io::Error> {
    let router = build_router(state);
    let listener = TcpListener::bind(config.http_addr).await?;

    tracing::info!("Conflux API server listening on {}", config.http_addr);

    axum::serve(listener, router).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use conflux_schema::Schema;
    use conflux_store::SqliteStore;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        use conflux_core::MergeStrategy;
        use conflux_schema::{ConflictMode, EntityDef, FieldDef, FieldType};
        use std::collections::HashMap;

        // Create a schema with a "route" entity type
        let mut fields = HashMap::new();
        fields.insert(
            "weight".to_string(),
            FieldDef {
                name: "weight".to_string(),
                field_type: FieldType::Int,
                merge_strategy: MergeStrategy::Lww,
                conflict: ConflictMode::Auto,
                target: None,
                required: false,
                range: None,
                allowed_values: None,
                pattern: None,
            },
        );

        let mut entities = HashMap::new();
        entities.insert(
            "route".to_string(),
            EntityDef {
                name: "route".to_string(),
                fields,
                children: HashMap::new(),
            },
        );

        let schema = Schema {
            name: "test".to_string(),
            version: "1.0".to_string(),
            entities,
            environments: None,
        };
        let store = SqliteStore::open_in_memory().unwrap();
        AppState::new(schema, store, "test-doc")
    }

    #[tokio::test]
    async fn health_endpoint() {
        let state = test_state();
        let router = build_router(state);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn state_endpoint_empty_document() {
        let state = test_state();
        let router = build_router(state);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn submit_operation_missing_actor() {
        let state = test_state();
        let router = build_router(state);

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/ops")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"type": "set_field", "entity_id": "test", "field": "name", "value": "foo"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn submit_operation_success() {
        let state = test_state();
        let router = build_router(state);

        // First insert the entity
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/ops")
                    .header("content-type", "application/json")
                    .header("x-conflux-actor", "test-user")
                    .header("x-conflux-actor-class", "human")
                    .body(Body::from(
                        r#"{"type": "insert_entity", "entity_id": "route.api", "entity_type": "route"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn milestones_list_empty() {
        let state = test_state();
        let router = build_router(state);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/milestones")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn log_empty() {
        let state = test_state();
        let router = build_router(state);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/log")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    fn test_state_with_envs() -> AppState {
        use conflux_core::MergeStrategy;
        use conflux_schema::{ConflictMode, EntityDef, EnvironmentsDef, FieldDef, FieldType, OverlayDef};
        use std::collections::HashMap;

        let mut fields = HashMap::new();
        fields.insert(
            "weight".to_string(),
            FieldDef {
                name: "weight".to_string(),
                field_type: FieldType::Int,
                merge_strategy: MergeStrategy::Max,
                conflict: ConflictMode::Auto,
                target: None,
                required: false,
                range: None,
                allowed_values: None,
                pattern: None,
            },
        );

        let mut entities = HashMap::new();
        entities.insert(
            "route".to_string(),
            EntityDef {
                name: "route".to_string(),
                fields,
                children: HashMap::new(),
            },
        );

        let environments = Some(EnvironmentsDef {
            base: "production".to_string(),
            overlays: vec![
                OverlayDef {
                    name: "staging".to_string(),
                    inherits: "production".to_string(),
                },
                OverlayDef {
                    name: "development".to_string(),
                    inherits: "staging".to_string(),
                },
            ],
        });

        let schema = Schema {
            name: "test-envs".to_string(),
            version: "1.0".to_string(),
            entities,
            environments,
        };
        let store = SqliteStore::open_in_memory().unwrap();
        AppState::new(schema, store, "test-doc")
    }

    #[tokio::test]
    async fn list_environments() {
        let state = test_state_with_envs();
        let router = build_router(state);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/envs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_environment_state() {
        let state = test_state_with_envs();
        let router = build_router(state);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/envs/staging/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_environment_state_not_found() {
        let state = test_state_with_envs();
        let router = build_router(state);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/envs/nonexistent/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn diff_environments() {
        let state = test_state_with_envs();
        let router = build_router(state);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/envs/diff?from=production&to=staging")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn submit_set_override() {
        let state = test_state_with_envs();
        let router = build_router(state);

        // First insert the entity
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/ops")
                    .header("content-type", "application/json")
                    .header("x-conflux-actor", "test-user")
                    .header("x-conflux-actor-class", "human")
                    .body(Body::from(
                        r#"{"type": "insert_entity", "entity_id": "route.api", "entity_type": "route"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Set a staging override
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/ops")
                    .header("content-type", "application/json")
                    .header("x-conflux-actor", "test-user")
                    .header("x-conflux-actor-class", "human")
                    .body(Body::from(
                        r#"{"type": "set_override", "entity_id": "route.api", "field": "weight", "environment": "staging", "value": 50}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn audit_export_empty() {
        let state = test_state();
        let router = build_router(state);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/audit/export")
                    .header("x-conflux-actor", "admin")
                    .header("x-conflux-actor-class", "human")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/x-ndjson"
        );
    }

    #[tokio::test]
    async fn audit_export_with_operations() {
        let state = test_state();
        let router = build_router(state);

        // First insert an entity
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/ops")
                    .header("content-type", "application/json")
                    .header("x-conflux-actor", "test-user")
                    .header("x-conflux-actor-class", "human")
                    .body(Body::from(
                        r#"{"type": "insert_entity", "entity_id": "route.api", "entity_type": "route"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Export the audit log
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/audit/export")
                    .header("x-conflux-actor", "admin")
                    .header("x-conflux-actor-class", "human")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Check we got operations exported
        let export_count = response
            .headers()
            .get("x-audit-operations-exported")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        assert!(export_count >= 1);
    }

    #[tokio::test]
    async fn audit_export_with_filters() {
        let state = test_state();
        let router = build_router(state);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/audit/export?actor=test-user&entity=route")
                    .header("x-conflux-actor", "admin")
                    .header("x-conflux-actor-class", "human")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
