//! Actor extractors for Axum handlers.

use super::api_key::{AuthMethod, AuthenticatedActor};
use crate::error::ApiError;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use conflux_core::{ActorClass, ActorId};

/// Header names for legacy actor identity.
pub const ACTOR_HEADER: &str = "x-conflux-actor";
pub const ACTOR_CLASS_HEADER: &str = "x-conflux-actor-class";

/// Extractor for authenticated actor identity.
///
/// This extractor first checks for an `AuthenticatedActor` in request extensions
/// (set by the auth middleware), then falls back to legacy header extraction.
#[derive(Debug, Clone)]
pub struct Actor(pub ActorId);

impl Actor {
    /// Returns the underlying actor ID.
    pub fn into_inner(self) -> ActorId {
        self.0
    }

    /// Returns a reference to the actor ID.
    pub fn as_actor_id(&self) -> &ActorId {
        &self.0
    }
}

impl<S> FromRequestParts<S> for Actor
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // First, check for authenticated actor from middleware
        if let Some(authenticated) = parts.extensions.get::<AuthenticatedActor>() {
            return Ok(Actor(authenticated.actor.clone()));
        }

        // Fall back to legacy header extraction
        extract_actor(&parts.headers)
    }
}

/// Extractor that provides both the actor and authentication metadata.
#[derive(Debug, Clone)]
pub struct AuthorizedActor {
    /// The actor identity.
    pub actor: ActorId,
    /// How the actor was authenticated.
    pub method: AuthMethod,
}

impl AuthorizedActor {
    /// Returns the actor ID.
    pub fn actor_id(&self) -> &ActorId {
        &self.actor
    }

    /// Returns true if this was authenticated via API key.
    pub fn is_api_key_auth(&self) -> bool {
        matches!(self.method, AuthMethod::ApiKey { .. })
    }

    /// Returns true if this was authenticated via mTLS.
    pub fn is_mtls_auth(&self) -> bool {
        matches!(self.method, AuthMethod::Mtls { .. })
    }

    /// Returns true if this was authenticated via legacy headers.
    pub fn is_legacy_auth(&self) -> bool {
        matches!(self.method, AuthMethod::LegacyHeaders)
    }
}

impl<S> FromRequestParts<S> for AuthorizedActor
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // First, check for authenticated actor from middleware
        if let Some(authenticated) = parts.extensions.get::<AuthenticatedActor>() {
            return Ok(AuthorizedActor {
                actor: authenticated.actor.clone(),
                method: authenticated.method.clone(),
            });
        }

        // Fall back to legacy header extraction
        let actor = extract_actor(&parts.headers)?;
        Ok(AuthorizedActor {
            actor: actor.0,
            method: AuthMethod::LegacyHeaders,
        })
    }
}

/// Extracts actor identity from headers.
pub fn extract_actor(headers: &HeaderMap) -> Result<Actor, ApiError> {
    let actor_id = headers
        .get(ACTOR_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::MissingActor("X-Conflux-Actor header required".to_string()))?;

    let actor_class_str = headers
        .get(ACTOR_CLASS_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            ApiError::MissingActor("X-Conflux-Actor-Class header required".to_string())
        })?;

    let actor_class = parse_actor_class(actor_class_str)?;

    Ok(Actor(ActorId::new(actor_id, actor_class)))
}

/// Parses an actor class string.
pub fn parse_actor_class(s: &str) -> Result<ActorClass, ApiError> {
    match s.to_lowercase().as_str() {
        "human" => Ok(ActorClass::Human),
        "pipeline" => Ok(ActorClass::Pipeline),
        "operator" => Ok(ActorClass::Operator),
        "system" => Ok(ActorClass::System),
        _ => Err(ApiError::InvalidActorClass(format!(
            "unknown actor class: {s} (expected: human, pipeline, operator, system)"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn parse_actor_class_valid() {
        assert!(matches!(parse_actor_class("human"), Ok(ActorClass::Human)));
        assert!(matches!(
            parse_actor_class("pipeline"),
            Ok(ActorClass::Pipeline)
        ));
        assert!(matches!(
            parse_actor_class("operator"),
            Ok(ActorClass::Operator)
        ));
        assert!(matches!(
            parse_actor_class("system"),
            Ok(ActorClass::System)
        ));
        assert!(matches!(parse_actor_class("HUMAN"), Ok(ActorClass::Human)));
    }

    #[test]
    fn parse_actor_class_invalid() {
        assert!(parse_actor_class("unknown").is_err());
    }

    #[test]
    fn extract_actor_success() {
        let mut headers = HeaderMap::new();
        headers.insert(ACTOR_HEADER, HeaderValue::from_static("alice"));
        headers.insert(ACTOR_CLASS_HEADER, HeaderValue::from_static("human"));

        let actor = extract_actor(&headers).unwrap();
        assert_eq!(actor.0.id, "alice");
        assert_eq!(actor.0.class, ActorClass::Human);
    }

    #[test]
    fn extract_actor_missing_id() {
        let mut headers = HeaderMap::new();
        headers.insert(ACTOR_CLASS_HEADER, HeaderValue::from_static("human"));

        let result = extract_actor(&headers);
        assert!(result.is_err());
    }

    #[test]
    fn extract_actor_missing_class() {
        let mut headers = HeaderMap::new();
        headers.insert(ACTOR_HEADER, HeaderValue::from_static("alice"));

        let result = extract_actor(&headers);
        assert!(result.is_err());
    }
}
