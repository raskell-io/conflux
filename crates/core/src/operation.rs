//! Operations — typed mutations applied to the document.

use crate::clock::HlcTimestamp;
use crate::field::FieldValue;
use crate::identity::{ActorId, EntityId};
use crate::signing::OperationSignature;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A typed operation submitted by an actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    /// Unique operation ID.
    pub id: Uuid,
    /// HLC timestamp for causal ordering.
    pub timestamp: HlcTimestamp,
    /// The actor who submitted this operation.
    pub actor: ActorId,
    /// Optional human-readable intent for auditability.
    pub intent: Option<String>,
    /// The kind of mutation.
    pub kind: OperationKind,
    /// Optional cryptographic signature for non-repudiation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<OperationSignature>,
}

/// The specific mutation an operation performs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationKind {
    /// Set a field on an entity.
    SetField {
        entity_id: EntityId,
        field: String,
        value: FieldValue,
    },
    /// Insert a new entity into the document.
    InsertEntity {
        entity_id: EntityId,
        entity_type: String,
        parent_id: Option<EntityId>,
        /// Fractional position string for ordering among siblings.
        position: Option<String>,
    },
    /// Soft-remove an entity (tombstone).
    RemoveEntity { entity_id: EntityId },
    /// Move an entity to a new parent and/or position.
    MoveEntity {
        entity_id: EntityId,
        new_parent_id: EntityId,
        new_position: String,
    },
    /// Set an environment-specific field override.
    SetOverride {
        entity_id: EntityId,
        field: String,
        environment: String,
        value: FieldValue,
    },
    /// Resolve a conflict by explicitly choosing a value.
    ResolveConflict {
        entity_id: EntityId,
        field: String,
        /// Environment if resolving an override conflict, None for base field.
        environment: Option<String>,
        /// The chosen value to resolve the conflict.
        chosen_value: FieldValue,
    },
}

impl Operation {
    /// Creates a SetField operation.
    pub fn set_field(
        entity_id: impl Into<EntityId>,
        field: impl Into<String>,
        value: FieldValue,
        actor: &ActorId,
        timestamp: HlcTimestamp,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp,
            actor: actor.clone(),
            intent: None,
            kind: OperationKind::SetField {
                entity_id: entity_id.into(),
                field: field.into(),
                value,
            },
            signature: None,
        }
    }

    /// Creates an InsertEntity operation.
    pub fn insert_entity(
        entity_id: impl Into<EntityId>,
        entity_type: impl Into<String>,
        parent_id: Option<EntityId>,
        position: Option<String>,
        actor: &ActorId,
        timestamp: HlcTimestamp,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp,
            actor: actor.clone(),
            intent: None,
            kind: OperationKind::InsertEntity {
                entity_id: entity_id.into(),
                entity_type: entity_type.into(),
                parent_id,
                position,
            },
            signature: None,
        }
    }

    /// Creates a RemoveEntity operation.
    pub fn remove_entity(
        entity_id: impl Into<EntityId>,
        actor: &ActorId,
        timestamp: HlcTimestamp,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp,
            actor: actor.clone(),
            intent: None,
            kind: OperationKind::RemoveEntity {
                entity_id: entity_id.into(),
            },
            signature: None,
        }
    }

    /// Creates a MoveEntity operation.
    pub fn move_entity(
        entity_id: impl Into<EntityId>,
        new_parent_id: impl Into<EntityId>,
        new_position: impl Into<String>,
        actor: &ActorId,
        timestamp: HlcTimestamp,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp,
            actor: actor.clone(),
            intent: None,
            kind: OperationKind::MoveEntity {
                entity_id: entity_id.into(),
                new_parent_id: new_parent_id.into(),
                new_position: new_position.into(),
            },
            signature: None,
        }
    }

    /// Creates a SetOverride operation for environment-specific field values.
    pub fn set_override(
        entity_id: impl Into<EntityId>,
        field: impl Into<String>,
        environment: impl Into<String>,
        value: FieldValue,
        actor: &ActorId,
        timestamp: HlcTimestamp,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp,
            actor: actor.clone(),
            intent: None,
            kind: OperationKind::SetOverride {
                entity_id: entity_id.into(),
                field: field.into(),
                environment: environment.into(),
                value,
            },
            signature: None,
        }
    }

    /// Creates a ResolveConflict operation for explicitly resolving a conflict.
    pub fn resolve_conflict(
        entity_id: impl Into<EntityId>,
        field: impl Into<String>,
        environment: Option<String>,
        chosen_value: FieldValue,
        actor: &ActorId,
        timestamp: HlcTimestamp,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp,
            actor: actor.clone(),
            intent: None,
            kind: OperationKind::ResolveConflict {
                entity_id: entity_id.into(),
                field: field.into(),
                environment,
                chosen_value,
            },
            signature: None,
        }
    }

    /// Attaches an intent string to this operation.
    pub fn with_intent(mut self, intent: impl Into<String>) -> Self {
        self.intent = Some(intent.into());
        self
    }

    /// Attaches a cryptographic signature to this operation.
    pub fn with_signature(mut self, signature: OperationSignature) -> Self {
        self.signature = Some(signature);
        self
    }

    /// Returns true if this operation has a signature.
    pub fn is_signed(&self) -> bool {
        self.signature.is_some()
    }
}

/// Result of applying an operation to a document.
#[derive(Debug, Clone, PartialEq)]
pub enum ApplyResult {
    /// The operation was applied cleanly.
    Applied,
    /// The operation was applied but a conflict was detected.
    Conflict(crate::entity::ConflictInfo),
}
