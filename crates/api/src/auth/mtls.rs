//! mTLS authentication.
//!
//! This module provides client certificate extraction and actor mapping
//! for mTLS-based authentication. The actual TLS termination is handled
//! by the server or a reverse proxy.

use super::api_key::{AuthMethod, AuthenticatedActor};
use super::config::{CertificateMapping, MtlsConfig};
use crate::error::ApiError;
use conflux_core::{ActorClass, ActorId};
use std::collections::HashMap;
use std::sync::Arc;

/// mTLS authenticator.
///
/// Extracts actor identity from client certificate information
/// passed via headers (when behind a TLS-terminating proxy) or
/// from the TLS connection directly.
#[derive(Debug, Clone)]
pub struct MtlsAuthenticator {
    /// Certificate mapping strategy.
    mapping: CertificateMapping,
    /// Custom mappings for fingerprint -> actor.
    custom_mappings: Arc<HashMap<String, ActorMapping>>,
}

/// Mapping from certificate to actor.
#[derive(Debug, Clone)]
pub struct ActorMapping {
    /// The actor ID.
    pub actor_id: String,
    /// The actor class.
    pub actor_class: ActorClass,
}

/// Client certificate information.
#[derive(Debug, Clone)]
pub struct ClientCertInfo {
    /// The certificate fingerprint (SHA-256).
    pub fingerprint: String,
    /// The Common Name (CN) from the subject.
    pub common_name: Option<String>,
    /// Subject Alternative Names (SANs).
    pub san: Vec<String>,
}

impl MtlsAuthenticator {
    /// Creates a new mTLS authenticator with the given configuration.
    pub fn new(config: &MtlsConfig) -> Result<Self, ApiError> {
        // In a full implementation, we would load custom mappings here
        Ok(Self {
            mapping: config.certificate_mapping.clone(),
            custom_mappings: Arc::new(HashMap::new()),
        })
    }

    /// Creates an authenticator for testing.
    pub fn for_testing(mapping: CertificateMapping) -> Self {
        Self {
            mapping,
            custom_mappings: Arc::new(HashMap::new()),
        }
    }

    /// Authenticates based on client certificate information.
    ///
    /// Returns the actor identity if the certificate maps to a valid actor.
    pub fn authenticate(&self, cert_info: &ClientCertInfo) -> Option<AuthenticatedActor> {
        // First, check custom mappings by fingerprint
        if let Some(mapping) = self.custom_mappings.get(&cert_info.fingerprint) {
            return Some(AuthenticatedActor {
                actor: ActorId::new(&mapping.actor_id, mapping.actor_class),
                method: AuthMethod::Mtls {
                    cert_fingerprint: cert_info.fingerprint.clone(),
                },
            });
        }

        // Otherwise, derive actor ID from certificate fields
        let actor_id = match self.mapping {
            CertificateMapping::Cn => cert_info.common_name.clone()?,
            CertificateMapping::San => cert_info.san.first().cloned()?,
            CertificateMapping::Mapping => {
                // Custom mapping mode requires an entry in custom_mappings
                return None;
            }
        };

        Some(AuthenticatedActor {
            actor: ActorId::new(&actor_id, ActorClass::System),
            method: AuthMethod::Mtls {
                cert_fingerprint: cert_info.fingerprint.clone(),
            },
        })
    }

    /// Adds a custom fingerprint -> actor mapping.
    pub fn add_mapping(&mut self, fingerprint: String, actor_id: String, actor_class: ActorClass) {
        Arc::make_mut(&mut self.custom_mappings).insert(
            fingerprint,
            ActorMapping {
                actor_id,
                actor_class,
            },
        );
    }
}

/// Header names for client certificate info (from TLS-terminating proxy).
pub mod headers {
    /// Client certificate fingerprint.
    pub const CLIENT_CERT_FINGERPRINT: &str = "x-client-cert-fingerprint";
    /// Client certificate Common Name.
    pub const CLIENT_CERT_CN: &str = "x-client-cert-cn";
    /// Client certificate SAN (comma-separated).
    pub const CLIENT_CERT_SAN: &str = "x-client-cert-san";
}

/// Extracts client certificate info from request headers.
///
/// This is used when running behind a TLS-terminating reverse proxy
/// that passes certificate information via headers.
pub fn extract_cert_info_from_headers(
    headers: &axum::http::HeaderMap,
) -> Option<ClientCertInfo> {
    let fingerprint = headers
        .get(headers::CLIENT_CERT_FINGERPRINT)
        .and_then(|v| v.to_str().ok())?
        .to_string();

    let common_name = headers
        .get(headers::CLIENT_CERT_CN)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let san = headers
        .get(headers::CLIENT_CERT_SAN)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    Some(ClientCertInfo {
        fingerprint,
        common_name,
        san,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authenticate_by_cn() {
        let auth = MtlsAuthenticator::for_testing(CertificateMapping::Cn);

        let cert_info = ClientCertInfo {
            fingerprint: "abc123".to_string(),
            common_name: Some("service-account".to_string()),
            san: vec![],
        };

        let result = auth.authenticate(&cert_info);
        assert!(result.is_some());

        let authenticated = result.unwrap();
        assert_eq!(authenticated.actor.id, "service-account");
        assert_eq!(authenticated.actor.class, ActorClass::System);
    }

    #[test]
    fn authenticate_by_san() {
        let auth = MtlsAuthenticator::for_testing(CertificateMapping::San);

        let cert_info = ClientCertInfo {
            fingerprint: "abc123".to_string(),
            common_name: Some("ignored".to_string()),
            san: vec!["dns:service.example.com".to_string()],
        };

        let result = auth.authenticate(&cert_info);
        assert!(result.is_some());

        let authenticated = result.unwrap();
        assert_eq!(authenticated.actor.id, "dns:service.example.com");
    }

    #[test]
    fn authenticate_missing_cn() {
        let auth = MtlsAuthenticator::for_testing(CertificateMapping::Cn);

        let cert_info = ClientCertInfo {
            fingerprint: "abc123".to_string(),
            common_name: None,
            san: vec!["alt-name".to_string()],
        };

        let result = auth.authenticate(&cert_info);
        assert!(result.is_none());
    }

    #[test]
    fn custom_mapping_takes_precedence() {
        let mut auth = MtlsAuthenticator::for_testing(CertificateMapping::Cn);
        auth.add_mapping(
            "special-fingerprint".to_string(),
            "admin".to_string(),
            ActorClass::Human,
        );

        let cert_info = ClientCertInfo {
            fingerprint: "special-fingerprint".to_string(),
            common_name: Some("different-cn".to_string()),
            san: vec![],
        };

        let result = auth.authenticate(&cert_info);
        assert!(result.is_some());

        let authenticated = result.unwrap();
        assert_eq!(authenticated.actor.id, "admin");
        assert_eq!(authenticated.actor.class, ActorClass::Human);
    }

    #[test]
    fn extract_cert_info_from_headers_success() {
        use axum::http::{HeaderMap, HeaderValue};

        let mut headers = HeaderMap::new();
        headers.insert(
            headers::CLIENT_CERT_FINGERPRINT,
            HeaderValue::from_static("sha256:abc123"),
        );
        headers.insert(
            headers::CLIENT_CERT_CN,
            HeaderValue::from_static("test-service"),
        );
        headers.insert(
            headers::CLIENT_CERT_SAN,
            HeaderValue::from_static("dns:a.example.com, dns:b.example.com"),
        );

        let info = extract_cert_info_from_headers(&headers).unwrap();
        assert_eq!(info.fingerprint, "sha256:abc123");
        assert_eq!(info.common_name, Some("test-service".to_string()));
        assert_eq!(info.san.len(), 2);
        assert_eq!(info.san[0], "dns:a.example.com");
        assert_eq!(info.san[1], "dns:b.example.com");
    }

    #[test]
    fn extract_cert_info_missing_fingerprint() {
        use axum::http::HeaderMap;

        let headers = HeaderMap::new();
        let info = extract_cert_info_from_headers(&headers);
        assert!(info.is_none());
    }
}
