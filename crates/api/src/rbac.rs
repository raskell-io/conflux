//! RBAC integration helpers.

use crate::dto::OperationDto;
use crate::error::ApiError;
use conflux_core::ActorId;
use conflux_rbac::{Action, AuthzContext, AuthzResult, Authorizer, RbacAuthorizer, Resource};

/// Converts an operation DTO to the corresponding RBAC action and resource.
pub fn operation_to_authz(op: &OperationDto) -> (Action, Resource) {
    match op {
        OperationDto::SetField {
            entity_id, field, ..
        } => {
            let parts = parse_entity_id(entity_id);
            (
                Action::Write,
                Resource {
                    entity_type: parts.0,
                    entity_id: parts.1,
                    field: Some(field.clone()),
                    environment: None,
                },
            )
        }
        OperationDto::InsertEntity {
            entity_id,
            entity_type,
            ..
        } => (
            Action::Create,
            Resource {
                entity_type: Some(entity_type.clone()),
                entity_id: Some(entity_id.clone()),
                field: None,
                environment: None,
            },
        ),
        OperationDto::RemoveEntity { entity_id } => {
            let parts = parse_entity_id(entity_id);
            (
                Action::Delete,
                Resource {
                    entity_type: parts.0,
                    entity_id: parts.1,
                    field: None,
                    environment: None,
                },
            )
        }
        OperationDto::MoveEntity { entity_id, .. } => {
            let parts = parse_entity_id(entity_id);
            (
                Action::Move,
                Resource {
                    entity_type: parts.0,
                    entity_id: parts.1,
                    field: None,
                    environment: None,
                },
            )
        }
        OperationDto::SetOverride {
            entity_id,
            field,
            environment,
            ..
        } => {
            let parts = parse_entity_id(entity_id);
            (
                Action::Override,
                Resource {
                    entity_type: parts.0,
                    entity_id: parts.1,
                    field: Some(field.clone()),
                    environment: Some(environment.clone()),
                },
            )
        }
        OperationDto::ResolveConflict {
            entity_id,
            field,
            environment,
            ..
        } => {
            let parts = parse_entity_id(entity_id);
            (
                Action::Resolve,
                Resource {
                    entity_type: parts.0,
                    entity_id: parts.1,
                    field: Some(field.clone()),
                    environment: environment.clone(),
                },
            )
        }
    }
}

/// Parses an entity ID to extract type and ID.
///
/// Entity IDs can be:
/// - "type.id" (e.g., "service.api-gateway")
/// - "id" (type unknown)
fn parse_entity_id(entity_id: &str) -> (Option<String>, Option<String>) {
    if let Some(idx) = entity_id.find('.') {
        let entity_type = &entity_id[..idx];
        let id = &entity_id[idx + 1..];
        (Some(entity_type.to_string()), Some(id.to_string()))
    } else {
        (None, Some(entity_id.to_string()))
    }
}

/// Checks authorization for an operation.
///
/// Returns `Ok(())` if authorized, `Err(ApiError::Forbidden)` if denied.
pub fn check_operation_authz(
    authorizer: &RbacAuthorizer,
    actor: &ActorId,
    operation: &OperationDto,
) -> Result<(), ApiError> {
    let (action, resource) = operation_to_authz(operation);
    let ctx = AuthzContext::new(actor.clone(), action, resource);

    match authorizer.authorize(&ctx) {
        AuthzResult::Allowed => Ok(()),
        AuthzResult::Denied(reason) => Err(reason.into()),
    }
}

/// Checks authorization for a read operation.
pub fn check_read_authz(
    authorizer: &RbacAuthorizer,
    actor: &ActorId,
    entity_type: Option<&str>,
    entity_id: Option<&str>,
) -> Result<(), ApiError> {
    let resource = Resource {
        entity_type: entity_type.map(|s| s.to_string()),
        entity_id: entity_id.map(|s| s.to_string()),
        field: None,
        environment: None,
    };

    let ctx = AuthzContext::new(actor.clone(), Action::Read, resource);

    match authorizer.authorize(&ctx) {
        AuthzResult::Allowed => Ok(()),
        AuthzResult::Denied(reason) => Err(reason.into()),
    }
}

/// Checks authorization for creating a milestone.
pub fn check_milestone_authz(authorizer: &RbacAuthorizer, actor: &ActorId) -> Result<(), ApiError> {
    let resource = Resource::global();
    let ctx = AuthzContext::new(actor.clone(), Action::Milestone, resource);

    match authorizer.authorize(&ctx) {
        AuthzResult::Allowed => Ok(()),
        AuthzResult::Denied(reason) => Err(reason.into()),
    }
}

/// Checks authorization for promoting between environments.
pub fn check_promote_authz(
    authorizer: &RbacAuthorizer,
    actor: &ActorId,
    to_env: &str,
) -> Result<(), ApiError> {
    let resource = Resource {
        entity_type: None,
        entity_id: None,
        field: None,
        environment: Some(to_env.to_string()),
    };

    let ctx = AuthzContext::new(actor.clone(), Action::Promote, resource);

    match authorizer.authorize(&ctx) {
        AuthzResult::Allowed => Ok(()),
        AuthzResult::Denied(reason) => Err(reason.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_entity_id_with_type() {
        let (entity_type, id) = parse_entity_id("service.api-gateway");
        assert_eq!(entity_type, Some("service".to_string()));
        assert_eq!(id, Some("api-gateway".to_string()));
    }

    #[test]
    fn parse_entity_id_without_type() {
        let (entity_type, id) = parse_entity_id("standalone");
        assert!(entity_type.is_none());
        assert_eq!(id, Some("standalone".to_string()));
    }

    #[test]
    fn operation_to_authz_set_field() {
        let op = OperationDto::SetField {
            entity_id: "service.api-gateway".to_string(),
            field: "replicas".to_string(),
            value: serde_json::json!(3),
        };

        let (action, resource) = operation_to_authz(&op);

        assert_eq!(action, Action::Write);
        assert_eq!(resource.entity_type, Some("service".to_string()));
        assert_eq!(resource.entity_id, Some("api-gateway".to_string()));
        assert_eq!(resource.field, Some("replicas".to_string()));
    }

    #[test]
    fn operation_to_authz_insert_entity() {
        let op = OperationDto::InsertEntity {
            entity_id: "route.v2".to_string(),
            entity_type: "route".to_string(),
            parent_id: None,
            position: None,
        };

        let (action, resource) = operation_to_authz(&op);

        assert_eq!(action, Action::Create);
        assert_eq!(resource.entity_type, Some("route".to_string()));
    }

    #[test]
    fn operation_to_authz_set_override() {
        let op = OperationDto::SetOverride {
            entity_id: "service.api".to_string(),
            field: "replicas".to_string(),
            environment: "staging".to_string(),
            value: serde_json::json!(5),
        };

        let (action, resource) = operation_to_authz(&op);

        assert_eq!(action, Action::Override);
        assert_eq!(resource.environment, Some("staging".to_string()));
    }
}
