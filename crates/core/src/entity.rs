//! Entity — a node in the document tree with typed fields.

use crate::clock::HlcTimestamp;
use crate::crdt::{CrdtMerge, LwwRegister};
use crate::field::ConflictState;
use crate::field_state::FieldState;
use crate::identity::{ActorId, EntityId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A single entity in the document model.
///
/// Entities have a type (matching the schema), a set of CRDT-backed fields,
/// a position in the document tree (parent + children), and a tombstone flag
/// for soft deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    /// Unique identifier for this entity.
    pub id: EntityId,
    /// The schema-declared entity type (e.g., "route", "service").
    pub entity_type: String,
    /// Per-field CRDT state, keyed by field name.
    pub fields: BTreeMap<String, FieldState>,
    /// Per-environment field overrides: env_name -> field_name -> FieldState.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, BTreeMap<String, FieldState>>,
    /// Parent entity ID, if this entity is a child.
    pub parent_id: Option<EntityId>,
    /// Child entities, keyed by fractional position string for ordering.
    pub children: BTreeMap<String, EntityId>,
    /// Tombstone flag — soft-deleted entities are marked but retained for merge.
    pub tombstone: LwwRegister<bool>,
}

impl Entity {
    /// Creates a new entity with the given ID and type.
    pub fn new(
        id: EntityId,
        entity_type: impl Into<String>,
        timestamp: HlcTimestamp,
        actor: ActorId,
    ) -> Self {
        Self {
            id,
            entity_type: entity_type.into(),
            fields: BTreeMap::new(),
            overrides: BTreeMap::new(),
            parent_id: None,
            children: BTreeMap::new(),
            tombstone: LwwRegister::new(false, timestamp, actor),
        }
    }

    /// Returns whether this entity is tombstoned (soft-deleted).
    pub fn is_tombstoned(&self) -> bool {
        self.tombstone.value
    }

    /// Sets a field to the given CRDT state.
    pub fn set_field(&mut self, name: impl Into<String>, state: FieldState) {
        self.fields.insert(name.into(), state);
    }

    /// Gets the CRDT state for a field.
    pub fn get_field(&self, name: &str) -> Option<&FieldState> {
        self.fields.get(name)
    }

    /// Sets an environment-specific field override.
    pub fn set_override(&mut self, env: impl Into<String>, name: impl Into<String>, state: FieldState) {
        self.overrides
            .entry(env.into())
            .or_default()
            .insert(name.into(), state);
    }

    /// Gets the CRDT state for a field override in a specific environment.
    pub fn get_override(&self, env: &str, name: &str) -> Option<&FieldState> {
        self.overrides.get(env)?.get(name)
    }

    /// Adds a child entity at the given position.
    pub fn add_child(&mut self, position: impl Into<String>, child_id: EntityId) {
        self.children.insert(position.into(), child_id);
    }

    /// Removes a child entity by its ID.
    pub fn remove_child(&mut self, child_id: &EntityId) {
        self.children.retain(|_, id| id != child_id);
    }

    /// Merges this entity with another entity of the same type.
    ///
    /// Fields are merged pairwise using their CRDT merge. The tombstone
    /// is merged via LWW. Children are merged via union. Overrides are
    /// merged per-environment.
    ///
    /// # Panics
    ///
    /// Panics if the entities have different types.
    pub fn merge(&self, other: &Self) -> (Self, Vec<ConflictInfo>) {
        assert_eq!(
            self.entity_type, other.entity_type,
            "cannot merge entities of different types: {} vs {}",
            self.entity_type, other.entity_type
        );

        let mut fields = BTreeMap::new();
        let mut conflicts = Vec::new();

        // Merge fields from both sides
        let all_field_names: std::collections::BTreeSet<&String> =
            self.fields.keys().chain(other.fields.keys()).collect();

        for name in all_field_names {
            let merged_state = match (self.fields.get(name), other.fields.get(name)) {
                (Some(a), Some(b)) => {
                    let merged = a.merge(b);
                    if let ConflictState::NeedsReview {
                        ref contending_values,
                    } = merged.conflict_state()
                    {
                        conflicts.push(ConflictInfo {
                            entity_id: self.id.clone(),
                            field: name.clone(),
                            environment: None,
                            contending_values: contending_values.clone(),
                        });
                    }
                    merged
                }
                (Some(a), None) => a.clone(),
                (None, Some(b)) => b.clone(),
                (None, None) => unreachable!(),
            };
            fields.insert(name.clone(), merged_state);
        }

        // Merge overrides per-environment
        let mut overrides = BTreeMap::new();
        let all_envs: std::collections::BTreeSet<&String> =
            self.overrides.keys().chain(other.overrides.keys()).collect();

        for env in all_envs {
            let self_env = self.overrides.get(env);
            let other_env = other.overrides.get(env);

            let mut env_fields = BTreeMap::new();
            let env_field_names: std::collections::BTreeSet<&String> = self_env
                .map(|m| m.keys())
                .into_iter()
                .flatten()
                .chain(other_env.map(|m| m.keys()).into_iter().flatten())
                .collect();

            for field_name in env_field_names {
                let self_field = self_env.and_then(|m| m.get(field_name));
                let other_field = other_env.and_then(|m| m.get(field_name));

                let merged_state = match (self_field, other_field) {
                    (Some(a), Some(b)) => {
                        let merged = a.merge(b);
                        if let ConflictState::NeedsReview {
                            ref contending_values,
                        } = merged.conflict_state()
                        {
                            conflicts.push(ConflictInfo {
                                entity_id: self.id.clone(),
                                field: field_name.clone(),
                                environment: Some(env.clone()),
                                contending_values: contending_values.clone(),
                            });
                        }
                        merged
                    }
                    (Some(a), None) => a.clone(),
                    (None, Some(b)) => b.clone(),
                    (None, None) => unreachable!(),
                };
                env_fields.insert(field_name.clone(), merged_state);
            }

            if !env_fields.is_empty() {
                overrides.insert(env.clone(), env_fields);
            }
        }

        // Merge tombstone via LWW
        let tombstone = self.tombstone.merge(&other.tombstone);

        // Merge children: union of both maps
        let mut children = self.children.clone();
        for (pos, child_id) in &other.children {
            children
                .entry(pos.clone())
                .or_insert_with(|| child_id.clone());
        }

        let entity = Entity {
            id: self.id.clone(),
            entity_type: self.entity_type.clone(),
            fields,
            overrides,
            parent_id: self.parent_id.clone().or_else(|| other.parent_id.clone()),
            children,
            tombstone,
        };

        (entity, conflicts)
    }
}

/// Information about a conflict detected during merge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConflictInfo {
    pub entity_id: EntityId,
    pub field: String,
    /// Environment name if this conflict is in an override, None for base field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    pub contending_values: Vec<crate::field::FieldValue>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Clock;
    use crate::field::{FieldValue, MergeStrategy};
    use crate::field_state::FieldState;
    use crate::identity::ActorClass;

    fn make_actor(id: &str) -> ActorId {
        ActorId::new(id, ActorClass::Human)
    }

    #[test]
    fn entity_new_is_not_tombstoned() {
        let clock = Clock::new();
        let ts = clock.new_timestamp();
        let actor = make_actor("alice");
        let entity = Entity::new(EntityId::new("route.api"), "route", ts, actor);
        assert!(!entity.is_tombstoned());
    }

    #[test]
    fn entity_set_and_get_field() {
        let clock = Clock::new();
        let ts = clock.new_timestamp();
        let actor = make_actor("alice");
        let mut entity = Entity::new(EntityId::new("route.api"), "route", ts, actor.clone());

        let state = FieldState::new(FieldValue::Int(80), ts, actor, MergeStrategy::Max);
        entity.set_field("weight", state);

        let field = entity.get_field("weight").unwrap();
        assert_eq!(field.value(), FieldValue::Int(80));
    }

    #[test]
    fn entity_merge_different_fields() {
        let clock = Clock::new();
        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();
        let alice = make_actor("alice");
        let bob = make_actor("bob");

        let mut e1 = Entity::new(EntityId::new("route.api"), "route", ts1, alice.clone());
        e1.set_field(
            "weight",
            FieldState::new(FieldValue::Int(80), ts1, alice, MergeStrategy::Max),
        );

        let mut e2 = Entity::new(EntityId::new("route.api"), "route", ts2, bob.clone());
        e2.set_field(
            "timeout",
            FieldState::new(FieldValue::Int(5000), ts2, bob, MergeStrategy::Lww),
        );

        let (merged, conflicts) = e1.merge(&e2);
        assert!(conflicts.is_empty());
        assert_eq!(
            merged.get_field("weight").unwrap().value(),
            FieldValue::Int(80)
        );
        assert_eq!(
            merged.get_field("timeout").unwrap().value(),
            FieldValue::Int(5000)
        );
    }

    #[test]
    fn entity_children_management() {
        let clock = Clock::new();
        let ts = clock.new_timestamp();
        let actor = make_actor("alice");
        let mut entity = Entity::new(EntityId::new("parent"), "container", ts, actor);

        let child_id = EntityId::new("child-1");
        entity.add_child("a", child_id.clone());
        assert_eq!(entity.children.len(), 1);

        entity.remove_child(&child_id);
        assert!(entity.children.is_empty());
    }
}
