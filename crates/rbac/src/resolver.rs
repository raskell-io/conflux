//! Role resolution for actors.

use crate::config::RbacConfig;
use conflux_core::{ActorClass, ActorId};
use std::collections::HashSet;

/// Resolves roles for actors.
#[derive(Debug, Clone)]
pub struct RoleResolver {
    config: RbacConfig,
}

impl RoleResolver {
    /// Creates a new role resolver with the given configuration.
    pub fn new(config: RbacConfig) -> Self {
        Self { config }
    }

    /// Returns the roles assigned to an actor.
    ///
    /// This includes:
    /// - Direct role assignments
    /// - Pattern-based assignments
    /// - Default authenticated roles
    pub fn roles_for_actor(&self, actor: &ActorId) -> HashSet<String> {
        let mut roles = HashSet::new();

        // Add default authenticated roles
        for role in &self.config.rbac.default_authenticated {
            roles.insert(role.clone());
        }

        // Check direct assignments and patterns
        for assignment in &self.config.assignment {
            if self.assignment_matches(assignment, actor) {
                for role in &assignment.roles {
                    roles.insert(role.clone());
                }
            }
        }

        // Expand inherited roles
        let mut expanded = HashSet::new();
        for role in &roles {
            self.expand_role(role, &mut expanded);
        }

        expanded
    }

    /// Returns the roles for an anonymous/unauthenticated actor.
    pub fn roles_for_anonymous(&self) -> HashSet<String> {
        let mut roles = HashSet::new();

        for role in &self.config.rbac.default_anonymous {
            roles.insert(role.clone());
        }

        // Expand inherited roles
        let mut expanded = HashSet::new();
        for role in &roles {
            self.expand_role(role, &mut expanded);
        }

        expanded
    }

    /// Checks if an assignment matches an actor.
    fn assignment_matches(
        &self,
        assignment: &crate::config::Assignment,
        actor: &ActorId,
    ) -> bool {
        // Check actor class if specified
        if let Some(ref required_class) = assignment.actor_class {
            let actor_class: ActorClass = (*required_class).into();
            if actor.class != actor_class {
                return false;
            }
        }

        // Check direct actor ID match
        if let Some(ref actor_id) = assignment.actor {
            return actor.id == *actor_id;
        }

        // Check pattern match
        if let Some(ref pattern) = assignment.actor_pattern {
            return pattern_matches(pattern, &actor.id);
        }

        false
    }

    /// Expands a role to include all inherited roles.
    fn expand_role(&self, role_name: &str, result: &mut HashSet<String>) {
        if result.contains(role_name) {
            return;
        }

        result.insert(role_name.to_string());

        if let Some(role) = self.config.role.get(role_name) {
            for parent in &role.inherits {
                self.expand_role(parent, result);
            }
        }
    }
}

/// Matches a glob-style pattern against a string.
///
/// Supports:
/// - `*` suffix for prefix matching
/// - `*` prefix for suffix matching
/// - `*` alone matches everything
fn pattern_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    if pattern.starts_with('*') && pattern.ends_with('*') && pattern.len() > 2 {
        // Contains match
        let needle = &pattern[1..pattern.len() - 1];
        return value.contains(needle);
    }

    if let Some(prefix) = pattern.strip_suffix('*') {
        // Prefix match
        return value.starts_with(prefix);
    }

    if let Some(suffix) = pattern.strip_prefix('*') {
        // Suffix match
        return value.ends_with(suffix);
    }

    // Exact match
    pattern == value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RbacConfig;
    use std::str::FromStr;

    fn test_config() -> RbacConfig {
        let toml = r#"
            [rbac]
            version = "1.0"
            default_authenticated = ["viewer"]
            default_anonymous = []

            [role.viewer]
            permissions = []

            [role.developer]
            inherits = ["viewer"]
            permissions = []

            [role.admin]
            inherits = ["developer"]
            permissions = []

            [[assignment]]
            actor = "alice"
            roles = ["admin"]

            [[assignment]]
            actor_pattern = "ci-*"
            actor_class = "pipeline"
            roles = ["developer"]

            [[assignment]]
            actor_pattern = "*-bot"
            roles = ["viewer"]
        "#;

        RbacConfig::from_str(toml).unwrap()
    }

    #[test]
    fn resolve_direct_assignment() {
        let resolver = RoleResolver::new(test_config());
        let actor = ActorId::new("alice", ActorClass::Human);

        let roles = resolver.roles_for_actor(&actor);

        assert!(roles.contains("admin"));
        assert!(roles.contains("developer")); // inherited
        assert!(roles.contains("viewer")); // inherited + default
    }

    #[test]
    fn resolve_pattern_assignment() {
        let resolver = RoleResolver::new(test_config());
        let actor = ActorId::new("ci-deploy", ActorClass::Pipeline);

        let roles = resolver.roles_for_actor(&actor);

        assert!(roles.contains("developer"));
        assert!(roles.contains("viewer")); // inherited + default
        assert!(!roles.contains("admin"));
    }

    #[test]
    fn resolve_suffix_pattern() {
        let resolver = RoleResolver::new(test_config());
        let actor = ActorId::new("github-bot", ActorClass::System);

        let roles = resolver.roles_for_actor(&actor);

        assert!(roles.contains("viewer"));
        assert!(!roles.contains("developer"));
    }

    #[test]
    fn resolve_default_only() {
        let resolver = RoleResolver::new(test_config());
        let actor = ActorId::new("unknown-user", ActorClass::Human);

        let roles = resolver.roles_for_actor(&actor);

        assert!(roles.contains("viewer")); // default
        assert!(!roles.contains("developer"));
        assert!(!roles.contains("admin"));
    }

    #[test]
    fn actor_class_filter() {
        let resolver = RoleResolver::new(test_config());

        // ci-deploy as Human should not match the Pipeline assignment
        let actor = ActorId::new("ci-deploy", ActorClass::Human);
        let roles = resolver.roles_for_actor(&actor);

        assert!(!roles.contains("developer")); // Pattern matches but class doesn't
    }

    #[test]
    fn pattern_matching() {
        assert!(pattern_matches("*", "anything"));
        assert!(pattern_matches("ci-*", "ci-deploy"));
        assert!(!pattern_matches("ci-*", "deploy-ci"));
        assert!(pattern_matches("*-bot", "github-bot"));
        assert!(!pattern_matches("*-bot", "bot-github"));
        assert!(pattern_matches("*service*", "my-service-name"));
        assert!(pattern_matches("exact", "exact"));
        assert!(!pattern_matches("exact", "different"));
    }

    #[test]
    fn anonymous_roles() {
        let toml = r#"
            [rbac]
            default_anonymous = ["readonly"]

            [role.readonly]
            permissions = []
        "#;

        let config = RbacConfig::from_str(toml).unwrap();
        let resolver = RoleResolver::new(config);

        let roles = resolver.roles_for_anonymous();
        assert!(roles.contains("readonly"));
    }
}
