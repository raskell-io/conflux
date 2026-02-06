//! RBAC configuration parsing.

use crate::error::RbacError;
use crate::types::{Action, ResourcePattern};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

/// Top-level RBAC configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RbacConfig {
    /// RBAC settings.
    #[serde(default)]
    pub rbac: RbacSettings,
    /// Role definitions.
    #[serde(default)]
    pub role: HashMap<String, RoleDef>,
    /// Actor-to-role assignments.
    #[serde(default)]
    pub assignment: Vec<Assignment>,
}

/// RBAC settings.
#[derive(Debug, Clone, Deserialize)]
pub struct RbacSettings {
    /// Configuration version.
    #[serde(default = "default_version")]
    pub version: String,
    /// Roles assigned to all authenticated users.
    #[serde(default)]
    pub default_authenticated: Vec<String>,
    /// Roles assigned to anonymous/unauthenticated users.
    #[serde(default)]
    pub default_anonymous: Vec<String>,
}

fn default_version() -> String {
    "1.0".to_string()
}

impl Default for RbacSettings {
    fn default() -> Self {
        Self {
            version: default_version(),
            default_authenticated: vec!["viewer".to_string()],
            default_anonymous: vec![],
        }
    }
}

/// A role definition.
#[derive(Debug, Clone, Default, Deserialize, serde::Serialize)]
pub struct RoleDef {
    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// Roles this role inherits from.
    #[serde(default)]
    pub inherits: Vec<String>,
    /// Permissions granted by this role.
    #[serde(default)]
    pub permissions: Vec<PermissionDef>,
}

/// A permission definition within a role.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct PermissionDef {
    /// Actions allowed (or "*" for all).
    pub actions: ActionList,
    /// Resource pattern this permission applies to.
    pub resource: ResourcePattern,
}

/// A list of actions, supporting wildcard.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(untagged)]
pub enum ActionList {
    /// Specific actions.
    List(Vec<ActionOrWildcard>),
}

/// An action or wildcard.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(untagged)]
pub enum ActionOrWildcard {
    /// All actions.
    #[serde(deserialize_with = "deserialize_wildcard")]
    Wildcard,
    /// Specific action.
    Action(Action),
}

fn deserialize_wildcard<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s == "*" {
        Ok(())
    } else {
        Err(serde::de::Error::custom("expected '*'"))
    }
}

impl ActionList {
    /// Returns all actions represented by this list.
    pub fn actions(&self) -> Vec<Action> {
        match self {
            ActionList::List(list) => {
                let mut result = Vec::new();
                for item in list {
                    match item {
                        ActionOrWildcard::Wildcard => {
                            return Action::all().to_vec();
                        }
                        ActionOrWildcard::Action(a) => {
                            result.push(*a);
                        }
                    }
                }
                result
            }
        }
    }
}

/// An actor-to-role assignment.
#[derive(Debug, Clone, Deserialize)]
pub struct Assignment {
    /// Specific actor ID (mutually exclusive with actor_pattern).
    pub actor: Option<String>,
    /// Actor ID pattern (glob-style, e.g., "ci-*").
    pub actor_pattern: Option<String>,
    /// Actor class filter.
    pub actor_class: Option<AssignmentActorClass>,
    /// Roles to assign.
    pub roles: Vec<String>,
}

/// Actor class for assignments.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AssignmentActorClass {
    Human,
    Pipeline,
    Operator,
    System,
}

impl From<AssignmentActorClass> for conflux_core::ActorClass {
    fn from(class: AssignmentActorClass) -> Self {
        match class {
            AssignmentActorClass::Human => conflux_core::ActorClass::Human,
            AssignmentActorClass::Pipeline => conflux_core::ActorClass::Pipeline,
            AssignmentActorClass::Operator => conflux_core::ActorClass::Operator,
            AssignmentActorClass::System => conflux_core::ActorClass::System,
        }
    }
}

impl FromStr for RbacConfig {
    type Err = RbacError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: RbacConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }
}

impl RbacConfig {
    /// Loads RBAC configuration from a TOML file.
    pub fn from_file(path: &Path) -> Result<Self, RbacError> {
        let content = std::fs::read_to_string(path)?;
        content.parse()
    }

    /// Validates the configuration for consistency.
    fn validate(&self) -> Result<(), RbacError> {
        // Check that all inherited roles exist
        for (name, role) in &self.role {
            for parent in &role.inherits {
                if !self.role.contains_key(parent) {
                    return Err(RbacError::UnknownRole(format!(
                        "role '{}' inherits from unknown role '{}'",
                        name, parent
                    )));
                }
            }
        }

        // Check for circular inheritance
        for name in self.role.keys() {
            self.check_circular_inheritance(name, &mut vec![])?;
        }

        // Check that assigned roles exist
        for assignment in &self.assignment {
            for role in &assignment.roles {
                if !self.role.contains_key(role) {
                    return Err(RbacError::UnknownRole(format!(
                        "assignment references unknown role '{}'",
                        role
                    )));
                }
            }
        }

        // Check default roles exist
        for role in &self.rbac.default_authenticated {
            if !self.role.contains_key(role) {
                return Err(RbacError::UnknownRole(format!(
                    "default_authenticated references unknown role '{}'",
                    role
                )));
            }
        }

        for role in &self.rbac.default_anonymous {
            if !self.role.contains_key(role) {
                return Err(RbacError::UnknownRole(format!(
                    "default_anonymous references unknown role '{}'",
                    role
                )));
            }
        }

        Ok(())
    }

    fn check_circular_inheritance(
        &self,
        role_name: &str,
        visited: &mut Vec<String>,
    ) -> Result<(), RbacError> {
        if visited.contains(&role_name.to_string()) {
            return Err(RbacError::CircularInheritance(format!(
                "cycle detected: {} -> {}",
                visited.join(" -> "),
                role_name
            )));
        }

        if let Some(role) = self.role.get(role_name) {
            visited.push(role_name.to_string());
            for parent in &role.inherits {
                self.check_circular_inheritance(parent, visited)?;
            }
            visited.pop();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_config() {
        let toml = r#"
            [rbac]
            version = "1.0"
            default_authenticated = ["viewer"]
            default_anonymous = []

            [role.viewer]
            description = "Read-only access"
            permissions = [
                { actions = ["read"], resource = "*" },
            ]

            [role.developer]
            inherits = ["viewer"]
            permissions = [
                { actions = ["write", "create", "delete"], resource = "*@development" },
            ]

            [[assignment]]
            actor = "alice"
            roles = ["developer"]
        "#;

        let config = RbacConfig::from_str(toml).unwrap();
        assert_eq!(config.rbac.version, "1.0");
        assert_eq!(config.role.len(), 2);
        assert_eq!(config.assignment.len(), 1);
    }

    #[test]
    fn parse_wildcard_actions() {
        let toml = r#"
            [rbac]
            default_authenticated = []

            [role.admin]
            permissions = [
                { actions = ["*"], resource = "*" },
            ]
        "#;

        let config = RbacConfig::from_str(toml).unwrap();
        let admin = config.role.get("admin").unwrap();
        let actions = admin.permissions[0].actions.actions();
        assert_eq!(actions.len(), Action::all().len());
    }

    #[test]
    fn parse_pattern_assignment() {
        let toml = r#"
            [rbac]
            default_authenticated = []

            [role.ci]
            permissions = [
                { actions = ["read"], resource = "*" },
            ]

            [[assignment]]
            actor_pattern = "ci-*"
            actor_class = "pipeline"
            roles = ["ci"]
        "#;

        let config = RbacConfig::from_str(toml).unwrap();
        let assignment = &config.assignment[0];
        assert_eq!(assignment.actor_pattern, Some("ci-*".to_string()));
        assert_eq!(assignment.actor_class, Some(AssignmentActorClass::Pipeline));
    }

    #[test]
    fn validate_unknown_role() {
        let toml = r#"
            [[assignment]]
            actor = "alice"
            roles = ["nonexistent"]
        "#;

        let result = RbacConfig::from_str(toml);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown role"));
    }

    #[test]
    fn validate_unknown_inherited_role() {
        let toml = r#"
            [role.child]
            inherits = ["parent"]
            permissions = []
        "#;

        let result = RbacConfig::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn validate_circular_inheritance() {
        let toml = r#"
            [role.a]
            inherits = ["b"]
            permissions = []

            [role.b]
            inherits = ["a"]
            permissions = []
        "#;

        let result = RbacConfig::from_str(toml);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("circular"));
    }
}
