//! Actor identity handling for CLI commands.

use anyhow::Result;
use conflux_core::{ActorClass, ActorId};

/// Resolves the actor identity for CLI operations.
///
/// Priority:
/// 1. Explicit `--actor` flag
/// 2. `CONFLUX_ACTOR` environment variable
/// 3. Current user from the system
pub fn resolve_actor(explicit_actor: Option<&str>, explicit_class: Option<&str>) -> Result<ActorId> {
    let actor_id = if let Some(actor) = explicit_actor {
        actor.to_string()
    } else if let Ok(actor) = std::env::var("CONFLUX_ACTOR") {
        actor
    } else {
        whoami::username()
    };

    let class = if let Some(class_str) = explicit_class {
        parse_actor_class(class_str)?
    } else if let Ok(class_str) = std::env::var("CONFLUX_ACTOR_CLASS") {
        parse_actor_class(&class_str)?
    } else {
        ActorClass::Human
    };

    Ok(ActorId::new(&actor_id, class))
}

/// Parses an actor class from a string.
fn parse_actor_class(s: &str) -> Result<ActorClass> {
    match s.to_lowercase().as_str() {
        "human" => Ok(ActorClass::Human),
        "pipeline" => Ok(ActorClass::Pipeline),
        "operator" => Ok(ActorClass::Operator),
        "system" => Ok(ActorClass::System),
        _ => anyhow::bail!(
            "invalid actor class '{}': expected human, pipeline, operator, or system",
            s
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_actor_classes() {
        assert!(matches!(
            parse_actor_class("human").unwrap(),
            ActorClass::Human
        ));
        assert!(matches!(
            parse_actor_class("PIPELINE").unwrap(),
            ActorClass::Pipeline
        ));
        assert!(matches!(
            parse_actor_class("Operator").unwrap(),
            ActorClass::Operator
        ));
        assert!(matches!(
            parse_actor_class("system").unwrap(),
            ActorClass::System
        ));
    }

    #[test]
    fn parse_invalid_actor_class() {
        assert!(parse_actor_class("unknown").is_err());
    }

    #[test]
    fn resolve_actor_explicit() {
        let actor = resolve_actor(Some("alice"), Some("pipeline")).unwrap();
        assert_eq!(actor.id, "alice");
        assert_eq!(actor.class, ActorClass::Pipeline);
    }

    #[test]
    fn resolve_actor_defaults_to_human() {
        let actor = resolve_actor(Some("bob"), None).unwrap();
        assert_eq!(actor.id, "bob");
        assert_eq!(actor.class, ActorClass::Human);
    }
}
