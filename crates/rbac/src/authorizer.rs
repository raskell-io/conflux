//! Authorization checks.

use crate::config::RbacConfig;
use std::str::FromStr;
use crate::resolver::RoleResolver;
use crate::types::{Action, Resource};
use conflux_core::ActorId;
use std::sync::Arc;

/// Context for an authorization check.
#[derive(Debug, Clone)]
pub struct AuthzContext {
    /// The actor requesting access.
    pub actor: ActorId,
    /// The action being performed.
    pub action: Action,
    /// The resource being accessed.
    pub resource: Resource,
}

impl AuthzContext {
    /// Creates a new authorization context.
    pub fn new(actor: ActorId, action: Action, resource: Resource) -> Self {
        Self {
            actor,
            action,
            resource,
        }
    }
}

/// Result of an authorization check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthzResult {
    /// Access is allowed.
    Allowed,
    /// Access is denied.
    Denied(DenialReason),
}

/// Reason for access denial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenialReason {
    /// Human-readable explanation.
    pub message: String,
    /// The action that was denied.
    pub action: Action,
    /// The resource that was denied.
    pub resource: String,
}

impl std::fmt::Display for DenialReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Trait for authorization implementations.
pub trait Authorizer: Send + Sync {
    /// Checks if the given context is authorized.
    fn authorize(&self, ctx: &AuthzContext) -> AuthzResult;
}

/// RBAC-based authorizer.
#[derive(Debug, Clone)]
pub struct RbacAuthorizer {
    config: Arc<RbacConfig>,
    resolver: RoleResolver,
}

impl RbacAuthorizer {
    /// Creates a new RBAC authorizer.
    pub fn new(config: RbacConfig) -> Self {
        let resolver = RoleResolver::new(config.clone());
        Self {
            config: Arc::new(config),
            resolver,
        }
    }

    /// Creates an authorizer that allows all actions (for testing or disabled RBAC).
    pub fn allow_all() -> Self {
        let toml = r#"
            [rbac]
            default_authenticated = ["admin"]

            [role.admin]
            permissions = [
                { actions = ["*"], resource = "*" },
            ]
        "#;

        let config = RbacConfig::from_str(toml).expect("built-in config should be valid");
        Self::new(config)
    }

    /// Returns a reference to the resolver.
    pub fn resolver(&self) -> &RoleResolver {
        &self.resolver
    }
}

impl Authorizer for RbacAuthorizer {
    fn authorize(&self, ctx: &AuthzContext) -> AuthzResult {
        let roles = self.resolver.roles_for_actor(&ctx.actor);

        tracing::debug!(
            actor = %ctx.actor.id,
            action = %ctx.action,
            resource = %ctx.resource,
            roles = ?roles,
            "checking authorization"
        );

        // Check each role's permissions
        for role_name in &roles {
            if let Some(role) = self.config.role.get(role_name) {
                for perm in &role.permissions {
                    let actions = perm.actions.actions();
                    if actions.contains(&ctx.action) && perm.resource.matches(&ctx.resource) {
                        tracing::debug!(
                            role = %role_name,
                            pattern = %perm.resource,
                            "access granted"
                        );
                        return AuthzResult::Allowed;
                    }
                }
            }
        }

        // Deny by default
        AuthzResult::Denied(DenialReason {
            message: format!(
                "actor '{}' is not authorized to {} on {}",
                ctx.actor.id, ctx.action, ctx.resource
            ),
            action: ctx.action,
            resource: ctx.resource.to_string(),
        })
    }
}

/// A composable authorizer that checks multiple authorizers.
pub struct ChainAuthorizer {
    authorizers: Vec<Box<dyn Authorizer>>,
}

impl ChainAuthorizer {
    /// Creates a new chain authorizer.
    pub fn new() -> Self {
        Self {
            authorizers: Vec::new(),
        }
    }

    /// Adds an authorizer to the chain.
    pub fn with_authorizer<A: Authorizer + 'static>(mut self, authorizer: A) -> Self {
        self.authorizers.push(Box::new(authorizer));
        self
    }
}

impl Default for ChainAuthorizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Authorizer for ChainAuthorizer {
    fn authorize(&self, ctx: &AuthzContext) -> AuthzResult {
        // First authorizer to allow wins
        for authorizer in &self.authorizers {
            if let AuthzResult::Allowed = authorizer.authorize(ctx) {
                return AuthzResult::Allowed;
            }
        }

        // If no authorizer allows, deny
        AuthzResult::Denied(DenialReason {
            message: format!(
                "no authorizer allows {} on {}",
                ctx.action, ctx.resource
            ),
            action: ctx.action,
            resource: ctx.resource.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conflux_core::ActorClass;

    fn test_authorizer() -> RbacAuthorizer {
        let toml = r#"
            [rbac]
            version = "1.0"
            default_authenticated = ["viewer"]

            [role.viewer]
            permissions = [
                { actions = ["read"], resource = "*" },
            ]

            [role.developer]
            inherits = ["viewer"]
            permissions = [
                { actions = ["write", "create", "delete"], resource = "*@development" },
            ]

            [role.operator]
            inherits = ["developer"]
            permissions = [
                { actions = ["write", "override"], resource = "*@staging" },
                { actions = ["promote"], resource = "*@production" },
            ]

            [role.admin]
            permissions = [
                { actions = ["*"], resource = "*" },
            ]

            [[assignment]]
            actor = "alice"
            roles = ["admin"]

            [[assignment]]
            actor = "bob"
            roles = ["operator"]

            [[assignment]]
            actor_pattern = "ci-*"
            actor_class = "pipeline"
            roles = ["developer"]
        "#;

        RbacAuthorizer::new(RbacConfig::from_str(toml).unwrap())
    }

    #[test]
    fn admin_can_do_anything() {
        let authz = test_authorizer();
        let actor = ActorId::new("alice", ActorClass::Human);

        let ctx = AuthzContext::new(
            actor,
            Action::Admin,
            Resource::global(),
        );

        assert_eq!(authz.authorize(&ctx), AuthzResult::Allowed);
    }

    #[test]
    fn viewer_can_read() {
        let authz = test_authorizer();
        let actor = ActorId::new("random-user", ActorClass::Human);

        let ctx = AuthzContext::new(
            actor,
            Action::Read,
            Resource::entity("service", "api-gateway"),
        );

        assert_eq!(authz.authorize(&ctx), AuthzResult::Allowed);
    }

    #[test]
    fn viewer_cannot_write() {
        let authz = test_authorizer();
        let actor = ActorId::new("random-user", ActorClass::Human);

        let ctx = AuthzContext::new(
            actor,
            Action::Write,
            Resource::entity("service", "api-gateway").in_env("production"),
        );

        assert!(matches!(authz.authorize(&ctx), AuthzResult::Denied(_)));
    }

    #[test]
    fn developer_can_write_in_development() {
        let authz = test_authorizer();
        let actor = ActorId::new("ci-deploy", ActorClass::Pipeline);

        let ctx = AuthzContext::new(
            actor,
            Action::Write,
            Resource::entity("service", "api-gateway").in_env("development"),
        );

        assert_eq!(authz.authorize(&ctx), AuthzResult::Allowed);
    }

    #[test]
    fn developer_cannot_write_in_production() {
        let authz = test_authorizer();
        let actor = ActorId::new("ci-deploy", ActorClass::Pipeline);

        let ctx = AuthzContext::new(
            actor,
            Action::Write,
            Resource::entity("service", "api-gateway").in_env("production"),
        );

        assert!(matches!(authz.authorize(&ctx), AuthzResult::Denied(_)));
    }

    #[test]
    fn operator_can_promote_to_production() {
        let authz = test_authorizer();
        let actor = ActorId::new("bob", ActorClass::Human);

        let ctx = AuthzContext::new(
            actor,
            Action::Promote,
            Resource::entity("service", "api-gateway").in_env("production"),
        );

        assert_eq!(authz.authorize(&ctx), AuthzResult::Allowed);
    }

    #[test]
    fn operator_can_write_in_staging() {
        let authz = test_authorizer();
        let actor = ActorId::new("bob", ActorClass::Human);

        let ctx = AuthzContext::new(
            actor,
            Action::Write,
            Resource::entity("service", "api-gateway").in_env("staging"),
        );

        assert_eq!(authz.authorize(&ctx), AuthzResult::Allowed);
    }

    #[test]
    fn allow_all_authorizer() {
        let authz = RbacAuthorizer::allow_all();
        let actor = ActorId::new("anyone", ActorClass::Human);

        let ctx = AuthzContext::new(
            actor,
            Action::Admin,
            Resource::global(),
        );

        assert_eq!(authz.authorize(&ctx), AuthzResult::Allowed);
    }

    #[test]
    fn chain_authorizer() {
        let chain = ChainAuthorizer::new()
            .with_authorizer(test_authorizer());

        let actor = ActorId::new("alice", ActorClass::Human);
        let ctx = AuthzContext::new(actor, Action::Admin, Resource::global());

        assert_eq!(chain.authorize(&ctx), AuthzResult::Allowed);
    }
}
