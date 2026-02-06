//! Authentication middleware for Tower/Axum.

use super::api_key::{AuthMethod, AuthenticatedActor, ApiKeyAuthenticator};
use super::config::{AuthConfig, AuthMode};
use super::extractor::parse_actor_class;
use super::mtls::{extract_cert_info_from_headers, MtlsAuthenticator};
use crate::error::ApiError;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Response};
use conflux_core::ActorId;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::{Layer, Service};

/// Header name for API key authentication.
pub const API_KEY_HEADER: &str = "x-api-key";

/// Legacy header names for backward compatibility.
pub const LEGACY_ACTOR_HEADER: &str = "x-conflux-actor";
pub const LEGACY_ACTOR_CLASS_HEADER: &str = "x-conflux-actor-class";

/// Authentication layer for Tower services.
#[derive(Clone)]
pub struct AuthLayer {
    config: Arc<AuthConfig>,
    api_key_auth: Option<Arc<ApiKeyAuthenticator>>,
    mtls_auth: Option<Arc<MtlsAuthenticator>>,
}

impl AuthLayer {
    /// Creates a new auth layer with the given configuration.
    pub fn new(
        config: AuthConfig,
        api_key_auth: Option<ApiKeyAuthenticator>,
        mtls_auth: Option<MtlsAuthenticator>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            api_key_auth: api_key_auth.map(Arc::new),
            mtls_auth: mtls_auth.map(Arc::new),
        }
    }

    /// Creates a permissive auth layer that allows all requests (for testing).
    pub fn permissive() -> Self {
        Self {
            config: Arc::new(AuthConfig {
                mode: AuthMode::None,
                allow_unauthenticated: true,
                api_keys: None,
                mtls: None,
            }),
            api_key_auth: None,
            mtls_auth: None,
        }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthMiddleware {
            inner,
            config: Arc::clone(&self.config),
            api_key_auth: self.api_key_auth.clone(),
            mtls_auth: self.mtls_auth.clone(),
        }
    }
}

/// Authentication middleware service.
#[derive(Clone)]
pub struct AuthMiddleware<S> {
    inner: S,
    config: Arc<AuthConfig>,
    api_key_auth: Option<Arc<ApiKeyAuthenticator>>,
    mtls_auth: Option<Arc<MtlsAuthenticator>>,
}

impl<S> Service<Request<Body>> for AuthMiddleware<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let config = Arc::clone(&self.config);
        let api_key_auth = self.api_key_auth.clone();
        let mtls_auth = self.mtls_auth.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Skip auth for health endpoint
            if req.uri().path() == "/health" {
                return inner.call(req).await;
            }

            // If auth mode is None and unauthenticated is allowed, skip auth
            if config.mode == AuthMode::None && config.allow_unauthenticated {
                return inner.call(req).await;
            }

            // Try to authenticate
            let auth_result =
                authenticate(&req, &config, api_key_auth.as_deref(), mtls_auth.as_deref());

            match auth_result {
                Ok(authenticated) => {
                    // Store the authenticated actor in request extensions
                    req.extensions_mut().insert(authenticated);
                    inner.call(req).await
                }
                Err(e) => {
                    // Check if unauthenticated requests are allowed
                    if config.allow_unauthenticated {
                        // Try legacy header auth
                        if let Some(authenticated) = try_legacy_auth(&req) {
                            req.extensions_mut().insert(authenticated);
                            return inner.call(req).await;
                        }
                    }
                    Ok(e.into_response())
                }
            }
        })
    }
}

/// Attempts authentication using configured methods.
fn authenticate(
    req: &Request<Body>,
    config: &AuthConfig,
    api_key_auth: Option<&ApiKeyAuthenticator>,
    mtls_auth: Option<&MtlsAuthenticator>,
) -> Result<AuthenticatedActor, ApiError> {
    let headers = req.headers();

    match config.mode {
        AuthMode::None => Err(ApiError::MissingActor(
            "authentication not configured".to_string(),
        )),

        AuthMode::ApiKey => {
            // Try API key authentication
            if let Some(auth) = api_key_auth {
                if let Some(key) = headers.get(API_KEY_HEADER).and_then(|v| v.to_str().ok()) {
                    if let Some(authenticated) = auth.authenticate(key) {
                        return Ok(authenticated);
                    }
                }
            }
            Err(ApiError::MissingActor(
                "invalid or missing API key".to_string(),
            ))
        }

        AuthMode::Mtls => {
            // Try mTLS authentication
            if let Some(auth) = mtls_auth {
                if let Some(cert_info) = extract_cert_info_from_headers(headers) {
                    if let Some(authenticated) = auth.authenticate(&cert_info) {
                        return Ok(authenticated);
                    }
                }
            }
            Err(ApiError::MissingActor(
                "invalid or missing client certificate".to_string(),
            ))
        }

        AuthMode::Both => {
            // Try API key first, then mTLS
            if let Some(auth) = api_key_auth {
                if let Some(key) = headers.get(API_KEY_HEADER).and_then(|v| v.to_str().ok()) {
                    if let Some(authenticated) = auth.authenticate(key) {
                        return Ok(authenticated);
                    }
                }
            }

            if let Some(auth) = mtls_auth {
                if let Some(cert_info) = extract_cert_info_from_headers(headers) {
                    if let Some(authenticated) = auth.authenticate(&cert_info) {
                        return Ok(authenticated);
                    }
                }
            }

            Err(ApiError::MissingActor(
                "invalid or missing authentication credentials".to_string(),
            ))
        }
    }
}

/// Attempts legacy header-based authentication.
fn try_legacy_auth(req: &Request<Body>) -> Option<AuthenticatedActor> {
    let headers = req.headers();

    let actor_id = headers
        .get(LEGACY_ACTOR_HEADER)
        .and_then(|v| v.to_str().ok())?;

    let actor_class_str = headers
        .get(LEGACY_ACTOR_CLASS_HEADER)
        .and_then(|v| v.to_str().ok())?;

    let actor_class = parse_actor_class(actor_class_str).ok()?;

    Some(AuthenticatedActor {
        actor: ActorId::new(actor_id, actor_class),
        method: AuthMethod::LegacyHeaders,
    })
}

impl AuthenticatedActor {
    /// Returns a 401 Unauthorized response.
    pub fn unauthorized(message: &str) -> Response {
        (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({
                "error": message,
                "code": "unauthorized"
            })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::config::ApiKeyActorClass;
    use crate::auth::config::ApiKeyEntry;
    use axum::http::HeaderValue;

    fn test_api_key_authenticator() -> ApiKeyAuthenticator {
        use crate::auth::api_key::format_hash;

        let entries = vec![ApiKeyEntry {
            key_hash: format_hash("test-key"),
            actor_id: "test-actor".to_string(),
            actor_class: ApiKeyActorClass::Pipeline,
            display_name: Some("Test Key".to_string()),
        }];

        ApiKeyAuthenticator::from_entries(&entries)
    }

    #[test]
    fn authenticate_with_api_key() {
        let config = AuthConfig {
            mode: AuthMode::ApiKey,
            allow_unauthenticated: false,
            api_keys: None,
            mtls: None,
        };
        let api_key_auth = test_api_key_authenticator();

        let mut req = Request::builder()
            .uri("/v1/state")
            .body(Body::empty())
            .unwrap();
        req.headers_mut().insert(
            API_KEY_HEADER,
            HeaderValue::from_static("test-key"),
        );

        let result = authenticate(&req, &config, Some(&api_key_auth), None);
        assert!(result.is_ok());

        let authenticated = result.unwrap();
        assert_eq!(authenticated.actor.id, "test-actor");
    }

    #[test]
    fn authenticate_with_invalid_api_key() {
        let config = AuthConfig {
            mode: AuthMode::ApiKey,
            allow_unauthenticated: false,
            api_keys: None,
            mtls: None,
        };
        let api_key_auth = test_api_key_authenticator();

        let mut req = Request::builder()
            .uri("/v1/state")
            .body(Body::empty())
            .unwrap();
        req.headers_mut().insert(
            API_KEY_HEADER,
            HeaderValue::from_static("wrong-key"),
        );

        let result = authenticate(&req, &config, Some(&api_key_auth), None);
        assert!(result.is_err());
    }

    #[test]
    fn legacy_auth_fallback() {
        let mut req = Request::builder()
            .uri("/v1/state")
            .body(Body::empty())
            .unwrap();
        req.headers_mut().insert(
            LEGACY_ACTOR_HEADER,
            HeaderValue::from_static("legacy-user"),
        );
        req.headers_mut().insert(
            LEGACY_ACTOR_CLASS_HEADER,
            HeaderValue::from_static("human"),
        );

        let result = try_legacy_auth(&req);
        assert!(result.is_some());

        let authenticated = result.unwrap();
        assert_eq!(authenticated.actor.id, "legacy-user");
        assert!(matches!(authenticated.method, AuthMethod::LegacyHeaders));
    }

    #[test]
    fn legacy_auth_missing_headers() {
        let req = Request::builder()
            .uri("/v1/state")
            .body(Body::empty())
            .unwrap();

        let result = try_legacy_auth(&req);
        assert!(result.is_none());
    }
}
