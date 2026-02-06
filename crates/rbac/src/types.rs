//! Core RBAC types.

use crate::error::RbacError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Actions that can be performed on resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Read entity or field state.
    Read,
    /// Write/update field values.
    Write,
    /// Create new entities.
    Create,
    /// Remove entities.
    Delete,
    /// Move entities in the hierarchy.
    Move,
    /// Override values in non-production environments.
    Override,
    /// Resolve conflicts.
    Resolve,
    /// Promote values between environments.
    Promote,
    /// Create milestones.
    Milestone,
    /// Administrative actions (key management, RBAC changes).
    Admin,
}

impl Action {
    /// Returns all actions (for wildcard matching).
    pub fn all() -> &'static [Action] {
        &[
            Action::Read,
            Action::Write,
            Action::Create,
            Action::Delete,
            Action::Move,
            Action::Override,
            Action::Resolve,
            Action::Promote,
            Action::Milestone,
            Action::Admin,
        ]
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Read => write!(f, "read"),
            Action::Write => write!(f, "write"),
            Action::Create => write!(f, "create"),
            Action::Delete => write!(f, "delete"),
            Action::Move => write!(f, "move"),
            Action::Override => write!(f, "override"),
            Action::Resolve => write!(f, "resolve"),
            Action::Promote => write!(f, "promote"),
            Action::Milestone => write!(f, "milestone"),
            Action::Admin => write!(f, "admin"),
        }
    }
}

/// A resource being accessed.
///
/// Resources are identified by entity type, entity ID, field, and environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    /// Entity type (e.g., "service", "route").
    pub entity_type: Option<String>,
    /// Specific entity ID.
    pub entity_id: Option<String>,
    /// Specific field name.
    pub field: Option<String>,
    /// Environment context.
    pub environment: Option<String>,
}

impl Resource {
    /// Creates a resource for an entity.
    pub fn entity(entity_type: &str, entity_id: &str) -> Self {
        Self {
            entity_type: Some(entity_type.to_string()),
            entity_id: Some(entity_id.to_string()),
            field: None,
            environment: None,
        }
    }

    /// Creates a resource for a field.
    pub fn field(entity_type: &str, entity_id: &str, field: &str) -> Self {
        Self {
            entity_type: Some(entity_type.to_string()),
            entity_id: Some(entity_id.to_string()),
            field: Some(field.to_string()),
            environment: None,
        }
    }

    /// Adds an environment to this resource.
    pub fn in_env(mut self, env: &str) -> Self {
        self.environment = Some(env.to_string());
        self
    }

    /// Creates a global resource (e.g., for admin actions).
    pub fn global() -> Self {
        Self {
            entity_type: None,
            entity_id: None,
            field: None,
            environment: None,
        }
    }
}

impl fmt::Display for Resource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();

        if let Some(ref et) = self.entity_type {
            if let Some(ref eid) = self.entity_id {
                parts.push(format!("{}/{}", et, eid));
            } else {
                parts.push(et.clone());
            }
        } else {
            parts.push("*".to_string());
        }

        if let Some(ref field) = self.field {
            parts.push(format!(".{}", field));
        }

        if let Some(ref env) = self.environment {
            parts.push(format!("@{}", env));
        }

        write!(f, "{}", parts.join(""))
    }
}

/// A pattern for matching resources.
///
/// Pattern syntax: `<entity_type>[/<entity_id>][.<field>][@<environment>]`
///
/// Examples:
/// - `*` — matches all resources
/// - `service` — matches all service entities
/// - `service/api-gateway` — matches specific service
/// - `service.replicas` — matches replicas field on all services
/// - `*@production` — matches all resources in production
/// - `service/api-*` — matches services with ID prefix
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ResourcePattern {
    /// Entity type pattern (None = wildcard).
    pub entity_type: Option<String>,
    /// Entity ID pattern (None = wildcard, supports * suffix).
    pub entity_id: Option<String>,
    /// Field name pattern (None = all fields).
    pub field: Option<String>,
    /// Environment pattern (None = all environments).
    pub environment: Option<String>,
}

impl ResourcePattern {
    /// Creates a wildcard pattern that matches everything.
    pub fn any() -> Self {
        Self {
            entity_type: None,
            entity_id: None,
            field: None,
            environment: None,
        }
    }

    /// Parses a pattern from a string.
    pub fn parse(s: &str) -> Result<Self, RbacError> {
        if s == "*" {
            return Ok(Self::any());
        }

        let mut pattern = Self {
            entity_type: None,
            entity_id: None,
            field: None,
            environment: None,
        };

        // Split off environment (@env)
        let (rest, env) = if let Some(idx) = s.rfind('@') {
            (&s[..idx], Some(&s[idx + 1..]))
        } else {
            (s, None)
        };
        pattern.environment = env.map(|s| s.to_string());

        // Split off field (.field)
        let (rest, field) = if let Some(idx) = rest.rfind('.') {
            (&rest[..idx], Some(&rest[idx + 1..]))
        } else {
            (rest, None)
        };
        pattern.field = field.map(|s| s.to_string());

        // Parse entity type and ID
        if !rest.is_empty() && rest != "*" {
            if let Some(idx) = rest.find('/') {
                let entity_type = &rest[..idx];
                let entity_id = &rest[idx + 1..];
                if entity_type != "*" {
                    pattern.entity_type = Some(entity_type.to_string());
                }
                if !entity_id.is_empty() && entity_id != "*" {
                    pattern.entity_id = Some(entity_id.to_string());
                }
            } else {
                pattern.entity_type = Some(rest.to_string());
            }
        }

        Ok(pattern)
    }

    /// Checks if this pattern matches the given resource.
    pub fn matches(&self, resource: &Resource) -> bool {
        // Check entity type
        if let Some(ref pattern_type) = self.entity_type {
            match &resource.entity_type {
                Some(res_type) if pattern_type != res_type => return false,
                None => return false,
                _ => {}
            }
        }

        // Check entity ID (supports prefix matching with *)
        if let Some(ref pattern_id) = self.entity_id {
            match &resource.entity_id {
                Some(res_id) => {
                    if pattern_id.ends_with('*') {
                        let prefix = &pattern_id[..pattern_id.len() - 1];
                        if !res_id.starts_with(prefix) {
                            return false;
                        }
                    } else if pattern_id != res_id {
                        return false;
                    }
                }
                None => return false,
            }
        }

        // Check field
        if let Some(ref pattern_field) = self.field {
            match &resource.field {
                Some(res_field) if pattern_field != res_field => return false,
                None => return false,
                _ => {}
            }
        }

        // Check environment
        if let Some(ref pattern_env) = self.environment {
            match &resource.environment {
                Some(res_env) if pattern_env != res_env => return false,
                None => return false,
                _ => {}
            }
        }

        true
    }
}

impl TryFrom<String> for ResourcePattern {
    type Error = RbacError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        ResourcePattern::parse(&s)
    }
}

impl From<ResourcePattern> for String {
    fn from(pattern: ResourcePattern) -> String {
        pattern.to_string()
    }
}

impl fmt::Display for ResourcePattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut result = String::new();

        // Entity type and ID
        match (&self.entity_type, &self.entity_id) {
            (Some(et), Some(eid)) => result.push_str(&format!("{}/{}", et, eid)),
            (Some(et), None) => result.push_str(et),
            (None, Some(eid)) => result.push_str(&format!("*/{}", eid)),
            (None, None) => result.push('*'),
        }

        // Field
        if let Some(ref field) = self.field {
            result.push_str(&format!(".{}", field));
        }

        // Environment
        if let Some(ref env) = self.environment {
            result.push_str(&format!("@{}", env));
        }

        write!(f, "{}", result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_wildcard_pattern() {
        let pattern = ResourcePattern::parse("*").unwrap();
        assert!(pattern.entity_type.is_none());
        assert!(pattern.entity_id.is_none());
        assert!(pattern.field.is_none());
        assert!(pattern.environment.is_none());
    }

    #[test]
    fn parse_entity_type_only() {
        let pattern = ResourcePattern::parse("service").unwrap();
        assert_eq!(pattern.entity_type, Some("service".to_string()));
        assert!(pattern.entity_id.is_none());
    }

    #[test]
    fn parse_entity_type_and_id() {
        let pattern = ResourcePattern::parse("service/api-gateway").unwrap();
        assert_eq!(pattern.entity_type, Some("service".to_string()));
        assert_eq!(pattern.entity_id, Some("api-gateway".to_string()));
    }

    #[test]
    fn parse_with_field() {
        let pattern = ResourcePattern::parse("service.replicas").unwrap();
        assert_eq!(pattern.entity_type, Some("service".to_string()));
        assert_eq!(pattern.field, Some("replicas".to_string()));
    }

    #[test]
    fn parse_with_environment() {
        let pattern = ResourcePattern::parse("*@production").unwrap();
        assert!(pattern.entity_type.is_none());
        assert_eq!(pattern.environment, Some("production".to_string()));
    }

    #[test]
    fn parse_full_pattern() {
        let pattern = ResourcePattern::parse("service/api-gateway.replicas@staging").unwrap();
        assert_eq!(pattern.entity_type, Some("service".to_string()));
        assert_eq!(pattern.entity_id, Some("api-gateway".to_string()));
        assert_eq!(pattern.field, Some("replicas".to_string()));
        assert_eq!(pattern.environment, Some("staging".to_string()));
    }

    #[test]
    fn pattern_matches_wildcard() {
        let pattern = ResourcePattern::any();
        let resource = Resource::entity("service", "api-gateway");
        assert!(pattern.matches(&resource));
    }

    #[test]
    fn pattern_matches_entity_type() {
        let pattern = ResourcePattern::parse("service").unwrap();

        let matching = Resource::entity("service", "api-gateway");
        let not_matching = Resource::entity("route", "api-route");

        assert!(pattern.matches(&matching));
        assert!(!pattern.matches(&not_matching));
    }

    #[test]
    fn pattern_matches_prefix() {
        let pattern = ResourcePattern::parse("service/api-*").unwrap();

        let matching1 = Resource::entity("service", "api-gateway");
        let matching2 = Resource::entity("service", "api-server");
        let not_matching = Resource::entity("service", "backend-worker");

        assert!(pattern.matches(&matching1));
        assert!(pattern.matches(&matching2));
        assert!(!pattern.matches(&not_matching));
    }

    #[test]
    fn pattern_matches_environment() {
        let pattern = ResourcePattern::parse("*@production").unwrap();

        let matching = Resource::entity("service", "api-gateway").in_env("production");
        let not_matching = Resource::entity("service", "api-gateway").in_env("staging");

        assert!(pattern.matches(&matching));
        assert!(!pattern.matches(&not_matching));
    }

    #[test]
    fn resource_display() {
        let resource = Resource::field("service", "api-gateway", "replicas").in_env("production");
        assert_eq!(resource.to_string(), "service/api-gateway.replicas@production");
    }

    #[test]
    fn pattern_display() {
        let pattern = ResourcePattern::parse("service/api-*.replicas@staging").unwrap();
        assert_eq!(pattern.to_string(), "service/api-*.replicas@staging");
    }
}
