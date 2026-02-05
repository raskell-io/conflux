//! Document — the top-level container of all entities.
//!
//! Provides `apply()` for applying operations and `merge()` for merging
//! two independently-modified documents.

use crate::clock::{Clock, HlcTimestamp};
use crate::crdt::{CrdtMerge, LwwRegister};
use crate::entity::{ConflictInfo, Entity};
use crate::error::{MergeError, OperationError};
use crate::field_state::FieldState;
use crate::identity::EntityId;
use crate::operation::{ApplyResult, Operation, OperationKind};
use crate::schema_info::SchemaInfo;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// The top-level document: a flat collection of entities with root references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    /// All entities, keyed by EntityId.
    pub entities: HashMap<EntityId, Entity>,
    /// Root entities (entities without a parent), keyed by position string.
    pub roots: BTreeMap<String, EntityId>,
}

impl Document {
    /// Creates an empty document.
    pub fn new() -> Self {
        Self {
            entities: HashMap::new(),
            roots: BTreeMap::new(),
        }
    }

    /// Returns a reference to an entity by ID.
    pub fn get_entity(&self, id: &EntityId) -> Option<&Entity> {
        self.entities.get(id)
    }

    /// Returns a mutable reference to an entity by ID.
    pub fn get_entity_mut(&mut self, id: &EntityId) -> Option<&mut Entity> {
        self.entities.get_mut(id)
    }

    /// Gets a field value from an entity (base value, no environment resolution).
    pub fn get_field(&self, entity_id: &EntityId, field: &str) -> Option<crate::field::FieldValue> {
        self.entities
            .get(entity_id)?
            .get_field(field)
            .map(|state| state.value())
    }

    /// Gets a field value for a specific environment, walking up the inheritance chain.
    ///
    /// Resolution order:
    /// 1. Check for override in the specified environment
    /// 2. Walk up to parent environments via inheritance chain
    /// 3. Fall back to base field value
    pub fn get_field_for_env(
        &self,
        entity_id: &EntityId,
        field: &str,
        env: &str,
        schema: &dyn SchemaInfo,
    ) -> Option<crate::field::FieldValue> {
        let entity = self.entities.get(entity_id)?;

        // Walk up the environment inheritance chain
        let mut current_env = Some(env);
        while let Some(e) = current_env {
            if let Some(state) = entity.get_override(e, field) {
                return Some(state.value());
            }
            // Move to parent environment
            current_env = schema.environment_parent(e);
        }

        // Fall back to base field value
        entity.get_field(field).map(|state| state.value())
    }

    /// Applies an operation to this document.
    ///
    /// # Errors
    ///
    /// Returns `OperationError` if the operation cannot be applied (e.g.,
    /// entity not found, duplicate entity, unknown field).
    pub fn apply(
        &mut self,
        op: &Operation,
        schema: &dyn SchemaInfo,
        clock: &Clock,
    ) -> Result<ApplyResult, OperationError> {
        // Update clock with incoming timestamp
        let _ = clock.update_with_timestamp(&op.timestamp);

        match &op.kind {
            OperationKind::SetField {
                entity_id,
                field,
                value,
            } => self.apply_set_field(entity_id, field, value, &op.timestamp, &op.actor, schema),
            OperationKind::InsertEntity {
                entity_id,
                entity_type,
                parent_id,
                position,
            } => self.apply_insert_entity(
                entity_id,
                entity_type,
                parent_id.as_ref(),
                position.as_deref(),
                &op.timestamp,
                &op.actor,
                schema,
            ),
            OperationKind::RemoveEntity { entity_id } => {
                self.apply_remove_entity(entity_id, &op.timestamp, &op.actor)
            }
            OperationKind::MoveEntity {
                entity_id,
                new_parent_id,
                new_position,
            } => self.apply_move_entity(
                entity_id,
                new_parent_id,
                new_position,
                &op.timestamp,
                &op.actor,
            ),
            OperationKind::SetOverride {
                entity_id,
                field,
                environment,
                value,
            } => self.apply_set_override(
                entity_id,
                field,
                environment,
                value,
                &op.timestamp,
                &op.actor,
                schema,
            ),
            OperationKind::ResolveConflict {
                entity_id,
                field,
                environment,
                chosen_value,
            } => self.apply_resolve_conflict(
                entity_id,
                field,
                environment.as_deref(),
                chosen_value,
                &op.timestamp,
                &op.actor,
            ),
        }
    }

    fn apply_set_field(
        &mut self,
        entity_id: &EntityId,
        field: &str,
        value: &crate::field::FieldValue,
        timestamp: &HlcTimestamp,
        actor: &crate::identity::ActorId,
        schema: &dyn SchemaInfo,
    ) -> Result<ApplyResult, OperationError> {
        let entity = self
            .entities
            .get_mut(entity_id)
            .ok_or_else(|| OperationError::EntityNotFound(entity_id.to_string()))?;

        let field_schema = schema
            .field_schema(&entity.entity_type, field)
            .ok_or_else(|| OperationError::UnknownField {
                entity_type: entity.entity_type.clone(),
                field: field.to_string(),
            })?;

        let incoming = FieldState::new(
            value.clone(),
            *timestamp,
            actor.clone(),
            field_schema.merge_strategy,
        );

        let result = if let Some(existing) = entity.fields.get(field) {
            let merged = existing.merge(&incoming);
            let conflict_state = merged.conflict_state();
            entity.fields.insert(field.to_string(), merged);

            if let crate::field::ConflictState::NeedsReview { contending_values } = conflict_state {
                ApplyResult::Conflict(ConflictInfo {
                    entity_id: entity_id.clone(),
                    field: field.to_string(),
                    environment: None,
                    contending_values,
                })
            } else {
                ApplyResult::Applied
            }
        } else {
            entity.fields.insert(field.to_string(), incoming);
            ApplyResult::Applied
        };

        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_insert_entity(
        &mut self,
        entity_id: &EntityId,
        entity_type: &str,
        parent_id: Option<&EntityId>,
        position: Option<&str>,
        timestamp: &HlcTimestamp,
        actor: &crate::identity::ActorId,
        schema: &dyn SchemaInfo,
    ) -> Result<ApplyResult, OperationError> {
        if self.entities.contains_key(entity_id) {
            return Err(OperationError::DuplicateEntity(entity_id.to_string()));
        }

        if !schema.has_entity_type(entity_type) {
            return Err(OperationError::UnknownEntityType(entity_type.to_string()));
        }

        // Verify parent exists if specified
        if let Some(pid) = parent_id {
            if !self.entities.contains_key(pid) {
                return Err(OperationError::ParentNotFound(pid.to_string()));
            }
        }

        let mut entity = Entity::new(entity_id.clone(), entity_type, *timestamp, actor.clone());
        entity.parent_id = parent_id.cloned();

        let pos = position
            .map(|s| s.to_string())
            .unwrap_or_else(|| entity_id.to_string());

        // Add to parent's children or to roots
        if let Some(pid) = parent_id {
            if let Some(parent) = self.entities.get_mut(pid) {
                parent.add_child(&pos, entity_id.clone());
            }
        } else {
            self.roots.insert(pos, entity_id.clone());
        }

        self.entities.insert(entity_id.clone(), entity);
        Ok(ApplyResult::Applied)
    }

    fn apply_remove_entity(
        &mut self,
        entity_id: &EntityId,
        timestamp: &HlcTimestamp,
        actor: &crate::identity::ActorId,
    ) -> Result<ApplyResult, OperationError> {
        let entity = self
            .entities
            .get_mut(entity_id)
            .ok_or_else(|| OperationError::EntityNotFound(entity_id.to_string()))?;

        let new_tombstone = LwwRegister::new(true, *timestamp, actor.clone());
        entity.tombstone = entity.tombstone.merge(&new_tombstone);

        Ok(ApplyResult::Applied)
    }

    fn apply_move_entity(
        &mut self,
        entity_id: &EntityId,
        new_parent_id: &EntityId,
        new_position: &str,
        timestamp: &HlcTimestamp,
        _actor: &crate::identity::ActorId,
    ) -> Result<ApplyResult, OperationError> {
        // Verify entity exists
        if !self.entities.contains_key(entity_id) {
            return Err(OperationError::EntityNotFound(entity_id.to_string()));
        }
        // Verify new parent exists
        if !self.entities.contains_key(new_parent_id) {
            return Err(OperationError::ParentNotFound(new_parent_id.to_string()));
        }

        // Remove from old parent or roots
        let old_parent_id = self
            .entities
            .get(entity_id)
            .and_then(|e| e.parent_id.clone());

        if let Some(ref old_pid) = old_parent_id {
            if let Some(old_parent) = self.entities.get_mut(old_pid) {
                old_parent.remove_child(entity_id);
            }
        } else {
            self.roots.retain(|_, id| id != entity_id);
        }

        // Add to new parent
        if let Some(new_parent) = self.entities.get_mut(new_parent_id) {
            new_parent.add_child(new_position, entity_id.clone());
        }

        // Update entity's parent_id
        if let Some(entity) = self.entities.get_mut(entity_id) {
            entity.parent_id = Some(new_parent_id.clone());
        }

        let _ = timestamp; // Used for future causal ordering

        Ok(ApplyResult::Applied)
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_set_override(
        &mut self,
        entity_id: &EntityId,
        field: &str,
        environment: &str,
        value: &crate::field::FieldValue,
        timestamp: &HlcTimestamp,
        actor: &crate::identity::ActorId,
        schema: &dyn SchemaInfo,
    ) -> Result<ApplyResult, OperationError> {
        // Verify entity exists
        let entity = self
            .entities
            .get_mut(entity_id)
            .ok_or_else(|| OperationError::EntityNotFound(entity_id.to_string()))?;

        // Verify field exists in schema
        let field_schema = schema
            .field_schema(&entity.entity_type, field)
            .ok_or_else(|| OperationError::UnknownField {
                entity_type: entity.entity_type.clone(),
                field: field.to_string(),
            })?;

        // Verify environment exists in schema
        if !schema.has_environment(environment) {
            return Err(OperationError::UnknownEnvironment(environment.to_string()));
        }

        let incoming = FieldState::new(
            value.clone(),
            *timestamp,
            actor.clone(),
            field_schema.merge_strategy,
        );

        let result = if let Some(existing) = entity.get_override(environment, field) {
            let merged = existing.merge(&incoming);
            let conflict_state = merged.conflict_state();
            entity.set_override(environment, field, merged);

            if let crate::field::ConflictState::NeedsReview { contending_values } = conflict_state {
                ApplyResult::Conflict(ConflictInfo {
                    entity_id: entity_id.clone(),
                    field: field.to_string(),
                    environment: Some(environment.to_string()),
                    contending_values,
                })
            } else {
                ApplyResult::Applied
            }
        } else {
            entity.set_override(environment, field, incoming);
            ApplyResult::Applied
        };

        Ok(result)
    }

    fn apply_resolve_conflict(
        &mut self,
        entity_id: &EntityId,
        field: &str,
        environment: Option<&str>,
        chosen_value: &crate::field::FieldValue,
        timestamp: &HlcTimestamp,
        actor: &crate::identity::ActorId,
    ) -> Result<ApplyResult, OperationError> {
        let entity = self
            .entities
            .get_mut(entity_id)
            .ok_or_else(|| OperationError::EntityNotFound(entity_id.to_string()))?;

        if let Some(env) = environment {
            // Resolve override conflict
            let existing = entity
                .get_override(env, field)
                .ok_or_else(|| OperationError::UnknownField {
                    entity_type: entity.entity_type.clone(),
                    field: field.to_string(),
                })?;

            // Only ReviewRegister can have conflicts
            if let FieldState::Review(reg) = existing {
                let resolved = reg.resolve(chosen_value.clone(), *timestamp, actor.clone());
                entity.set_override(env, field, FieldState::Review(resolved));
            } else {
                return Err(OperationError::NotAConflict {
                    entity_id: entity_id.to_string(),
                    field: field.to_string(),
                });
            }
        } else {
            // Resolve base field conflict
            let existing = entity
                .get_field(field)
                .ok_or_else(|| OperationError::UnknownField {
                    entity_type: entity.entity_type.clone(),
                    field: field.to_string(),
                })?;

            // Only ReviewRegister can have conflicts
            if let FieldState::Review(reg) = existing {
                let resolved = reg.resolve(chosen_value.clone(), *timestamp, actor.clone());
                entity.set_field(field, FieldState::Review(resolved));
            } else {
                return Err(OperationError::NotAConflict {
                    entity_id: entity_id.to_string(),
                    field: field.to_string(),
                });
            }
        }

        Ok(ApplyResult::Applied)
    }

    /// Lists all current conflicts in the document.
    ///
    /// Scans all entities and fields (including overrides) for fields with
    /// `ConflictState::NeedsReview`.
    pub fn list_conflicts(&self) -> Vec<ConflictInfo> {
        let mut conflicts = Vec::new();

        for (entity_id, entity) in &self.entities {
            // Skip tombstoned entities
            if entity.is_tombstoned() {
                continue;
            }

            // Check base fields
            for (field_name, field_state) in &entity.fields {
                if let crate::field::ConflictState::NeedsReview { ref contending_values } =
                    field_state.conflict_state()
                {
                    conflicts.push(ConflictInfo {
                        entity_id: entity_id.clone(),
                        field: field_name.clone(),
                        environment: None,
                        contending_values: contending_values.clone(),
                    });
                }
            }

            // Check override fields
            for (env_name, env_fields) in &entity.overrides {
                for (field_name, field_state) in env_fields {
                    if let crate::field::ConflictState::NeedsReview { ref contending_values } =
                        field_state.conflict_state()
                    {
                        conflicts.push(ConflictInfo {
                            entity_id: entity_id.clone(),
                            field: field_name.clone(),
                            environment: Some(env_name.clone()),
                            contending_values: contending_values.clone(),
                        });
                    }
                }
            }
        }

        conflicts
    }

    /// Merges two documents, producing a new document and a list of conflicts.
    ///
    /// # Errors
    ///
    /// Returns `MergeError::EntityTypeMismatch` if the same entity ID has
    /// different types in the two documents.
    pub fn merge(
        left: &Document,
        right: &Document,
    ) -> Result<(Document, Vec<ConflictInfo>), MergeError> {
        let mut entities = HashMap::new();
        let mut all_conflicts = Vec::new();

        // Collect all entity IDs from both documents
        let all_ids: std::collections::HashSet<&EntityId> =
            left.entities.keys().chain(right.entities.keys()).collect();

        for id in all_ids {
            match (left.entities.get(id), right.entities.get(id)) {
                (Some(left_entity), Some(right_entity)) => {
                    // Verify same entity type
                    if left_entity.entity_type != right_entity.entity_type {
                        return Err(MergeError::EntityTypeMismatch {
                            expected: left_entity.entity_type.clone(),
                            actual: right_entity.entity_type.clone(),
                        });
                    }
                    let (merged, conflicts) = left_entity.merge(right_entity);
                    all_conflicts.extend(conflicts);
                    entities.insert(id.clone(), merged);
                }
                (Some(entity), None) | (None, Some(entity)) => {
                    entities.insert(id.clone(), entity.clone());
                }
                (None, None) => unreachable!(),
            }
        }

        // Merge roots: union
        let mut roots = left.roots.clone();
        for (pos, id) in &right.roots {
            roots.entry(pos.clone()).or_insert_with(|| id.clone());
        }

        let doc = Document { entities, roots };
        Ok((doc, all_conflicts))
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Clock;
    use crate::field::{FieldValue, MergeStrategy};
    use crate::identity::{ActorClass, ActorId};
    use crate::schema_info::SimpleSchemaInfo;

    fn test_schema() -> SimpleSchemaInfo {
        let mut schema = SimpleSchemaInfo::new();
        schema.add_field("route", "weight", "int", MergeStrategy::Max);
        schema.add_field("route", "timeout", "int", MergeStrategy::Lww);
        schema.add_field("route", "path", "string", MergeStrategy::Lww);
        schema.add_field("route", "tags", "list", MergeStrategy::GrowSet);
        schema.add_field("route", "status", "string", MergeStrategy::Review);
        schema
    }

    fn make_actor(id: &str) -> ActorId {
        ActorId::new(id, ActorClass::Human)
    }

    #[test]
    fn apply_insert_and_set_field() {
        let schema = test_schema();
        let clock = Clock::new();
        let actor = make_actor("alice");
        let mut doc = Document::new();

        let entity_id = EntityId::new("route.api");
        let ts = clock.new_timestamp();

        // Insert entity
        let insert = Operation::insert_entity(entity_id.clone(), "route", None, None, &actor, ts);
        let result = doc.apply(&insert, &schema, &clock).unwrap();
        assert_eq!(result, ApplyResult::Applied);

        // Set field
        let ts2 = clock.new_timestamp();
        let set = Operation::set_field(
            entity_id.clone(),
            "weight",
            FieldValue::Int(80),
            &actor,
            ts2,
        );
        let result = doc.apply(&set, &schema, &clock).unwrap();
        assert_eq!(result, ApplyResult::Applied);

        assert_eq!(
            doc.get_field(&entity_id, "weight"),
            Some(FieldValue::Int(80))
        );
    }

    #[test]
    fn apply_set_field_entity_not_found() {
        let schema = test_schema();
        let clock = Clock::new();
        let actor = make_actor("alice");
        let mut doc = Document::new();

        let ts = clock.new_timestamp();
        let op = Operation::set_field("nonexistent", "weight", FieldValue::Int(80), &actor, ts);
        let err = doc.apply(&op, &schema, &clock).unwrap_err();
        assert!(matches!(err, OperationError::EntityNotFound(_)));
    }

    #[test]
    fn apply_insert_duplicate_entity() {
        let schema = test_schema();
        let clock = Clock::new();
        let actor = make_actor("alice");
        let mut doc = Document::new();

        let ts = clock.new_timestamp();
        let insert = Operation::insert_entity("route.api", "route", None, None, &actor, ts);
        doc.apply(&insert, &schema, &clock).unwrap();

        let ts2 = clock.new_timestamp();
        let insert2 = Operation::insert_entity("route.api", "route", None, None, &actor, ts2);
        let err = doc.apply(&insert2, &schema, &clock).unwrap_err();
        assert!(matches!(err, OperationError::DuplicateEntity(_)));
    }

    #[test]
    fn apply_remove_entity_sets_tombstone() {
        let schema = test_schema();
        let clock = Clock::new();
        let actor = make_actor("alice");
        let mut doc = Document::new();

        let ts = clock.new_timestamp();
        let insert = Operation::insert_entity("route.api", "route", None, None, &actor, ts);
        doc.apply(&insert, &schema, &clock).unwrap();

        let entity_id = EntityId::new("route.api");
        assert!(!doc.get_entity(&entity_id).unwrap().is_tombstoned());

        let ts2 = clock.new_timestamp();
        let remove = Operation::remove_entity("route.api", &actor, ts2);
        doc.apply(&remove, &schema, &clock).unwrap();

        assert!(doc.get_entity(&entity_id).unwrap().is_tombstoned());
    }

    #[test]
    fn two_actors_different_fields_merge_cleanly() {
        let schema = test_schema();
        let clock = Clock::new();
        let alice = make_actor("alice");
        let bob = make_actor("bob");

        let entity_id = EntityId::new("route.api");

        // Doc A: alice sets weight
        let mut doc_a = Document::new();
        let ts = clock.new_timestamp();
        doc_a
            .apply(
                &Operation::insert_entity(entity_id.clone(), "route", None, None, &alice, ts),
                &schema,
                &clock,
            )
            .unwrap();
        let ts = clock.new_timestamp();
        doc_a
            .apply(
                &Operation::set_field(entity_id.clone(), "weight", FieldValue::Int(80), &alice, ts),
                &schema,
                &clock,
            )
            .unwrap();

        // Doc B: bob sets timeout
        let mut doc_b = Document::new();
        let ts = clock.new_timestamp();
        doc_b
            .apply(
                &Operation::insert_entity(entity_id.clone(), "route", None, None, &bob, ts),
                &schema,
                &clock,
            )
            .unwrap();
        let ts = clock.new_timestamp();
        doc_b
            .apply(
                &Operation::set_field(
                    entity_id.clone(),
                    "timeout",
                    FieldValue::Int(5000),
                    &bob,
                    ts,
                ),
                &schema,
                &clock,
            )
            .unwrap();

        // Merge
        let (merged, conflicts) = Document::merge(&doc_a, &doc_b).unwrap();
        assert!(conflicts.is_empty());
        assert_eq!(
            merged.get_field(&entity_id, "weight"),
            Some(FieldValue::Int(80))
        );
        assert_eq!(
            merged.get_field(&entity_id, "timeout"),
            Some(FieldValue::Int(5000))
        );
    }

    #[test]
    fn merge_is_commutative() {
        let schema = test_schema();
        let clock = Clock::new();
        let alice = make_actor("alice");
        let bob = make_actor("bob");

        let entity_id = EntityId::new("route.api");

        let mut doc_a = Document::new();
        let ts = clock.new_timestamp();
        doc_a
            .apply(
                &Operation::insert_entity(entity_id.clone(), "route", None, None, &alice, ts),
                &schema,
                &clock,
            )
            .unwrap();
        let ts = clock.new_timestamp();
        doc_a
            .apply(
                &Operation::set_field(entity_id.clone(), "weight", FieldValue::Int(80), &alice, ts),
                &schema,
                &clock,
            )
            .unwrap();

        let mut doc_b = Document::new();
        let ts = clock.new_timestamp();
        doc_b
            .apply(
                &Operation::insert_entity(entity_id.clone(), "route", None, None, &bob, ts),
                &schema,
                &clock,
            )
            .unwrap();
        let ts = clock.new_timestamp();
        doc_b
            .apply(
                &Operation::set_field(entity_id.clone(), "weight", FieldValue::Int(60), &bob, ts),
                &schema,
                &clock,
            )
            .unwrap();

        let (ab, _) = Document::merge(&doc_a, &doc_b).unwrap();
        let (ba, _) = Document::merge(&doc_b, &doc_a).unwrap();

        // Same weight (max strategy picks higher value)
        assert_eq!(
            ab.get_field(&entity_id, "weight"),
            ba.get_field(&entity_id, "weight")
        );
    }

    #[test]
    fn review_strategy_flags_concurrent_writes() {
        let schema = test_schema();
        let clock = Clock::new();
        let alice = make_actor("alice");
        let bob = make_actor("bob");

        let entity_id = EntityId::new("route.api");

        let mut doc_a = Document::new();
        let ts = clock.new_timestamp();
        doc_a
            .apply(
                &Operation::insert_entity(entity_id.clone(), "route", None, None, &alice, ts),
                &schema,
                &clock,
            )
            .unwrap();
        let ts = clock.new_timestamp();
        doc_a
            .apply(
                &Operation::set_field(
                    entity_id.clone(),
                    "status",
                    FieldValue::String("active".into()),
                    &alice,
                    ts,
                ),
                &schema,
                &clock,
            )
            .unwrap();

        let mut doc_b = Document::new();
        let ts = clock.new_timestamp();
        doc_b
            .apply(
                &Operation::insert_entity(entity_id.clone(), "route", None, None, &bob, ts),
                &schema,
                &clock,
            )
            .unwrap();
        let ts = clock.new_timestamp();
        doc_b
            .apply(
                &Operation::set_field(
                    entity_id.clone(),
                    "status",
                    FieldValue::String("draining".into()),
                    &bob,
                    ts,
                ),
                &schema,
                &clock,
            )
            .unwrap();

        let (_, conflicts) = Document::merge(&doc_a, &doc_b).unwrap();
        assert!(!conflicts.is_empty());
        assert_eq!(conflicts[0].field, "status");
    }

    #[test]
    fn apply_move_entity() {
        let schema = test_schema();
        let clock = Clock::new();
        let actor = make_actor("alice");
        let mut doc = Document::new();

        // Insert parent entities
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::insert_entity("parent-a", "route", None, None, &actor, ts),
            &schema,
            &clock,
        )
        .unwrap();
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::insert_entity("parent-b", "route", None, None, &actor, ts),
            &schema,
            &clock,
        )
        .unwrap();

        // Insert child under parent-a
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::insert_entity(
                "child",
                "route",
                Some(EntityId::new("parent-a")),
                Some("a".into()),
                &actor,
                ts,
            ),
            &schema,
            &clock,
        )
        .unwrap();

        assert!(doc
            .get_entity(&EntityId::new("parent-a"))
            .unwrap()
            .children
            .values()
            .any(|id| id == &EntityId::new("child")));

        // Move child to parent-b
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::move_entity("child", "parent-b", "b", &actor, ts),
            &schema,
            &clock,
        )
        .unwrap();

        // Child should be under parent-b now
        assert!(!doc
            .get_entity(&EntityId::new("parent-a"))
            .unwrap()
            .children
            .values()
            .any(|id| id == &EntityId::new("child")));
        assert!(doc
            .get_entity(&EntityId::new("parent-b"))
            .unwrap()
            .children
            .values()
            .any(|id| id == &EntityId::new("child")));
    }

    fn test_schema_with_envs() -> SimpleSchemaInfo {
        let mut schema = test_schema();
        schema
            .set_base_environment("production")
            .add_environment("staging", "production")
            .add_environment("development", "staging");
        schema
    }

    #[test]
    fn apply_set_override() {
        let schema = test_schema_with_envs();
        let clock = Clock::new();
        let actor = make_actor("alice");
        let mut doc = Document::new();

        let entity_id = EntityId::new("route.api");
        let ts = clock.new_timestamp();

        // Insert entity
        doc.apply(
            &Operation::insert_entity(entity_id.clone(), "route", None, None, &actor, ts),
            &schema,
            &clock,
        )
        .unwrap();

        // Set base field
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_field(entity_id.clone(), "weight", FieldValue::Int(80), &actor, ts),
            &schema,
            &clock,
        )
        .unwrap();

        // Set staging override
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_override(
                entity_id.clone(),
                "weight",
                "staging",
                FieldValue::Int(50),
                &actor,
                ts,
            ),
            &schema,
            &clock,
        )
        .unwrap();

        // Base field still returns 80
        assert_eq!(doc.get_field(&entity_id, "weight"), Some(FieldValue::Int(80)));

        // Staging returns override
        assert_eq!(
            doc.get_field_for_env(&entity_id, "weight", "staging", &schema),
            Some(FieldValue::Int(50))
        );

        // Production returns base (no override)
        assert_eq!(
            doc.get_field_for_env(&entity_id, "weight", "production", &schema),
            Some(FieldValue::Int(80))
        );
    }

    #[test]
    fn environment_inheritance_chain() {
        let schema = test_schema_with_envs();
        let clock = Clock::new();
        let actor = make_actor("alice");
        let mut doc = Document::new();

        let entity_id = EntityId::new("route.api");
        let ts = clock.new_timestamp();

        doc.apply(
            &Operation::insert_entity(entity_id.clone(), "route", None, None, &actor, ts),
            &schema,
            &clock,
        )
        .unwrap();

        // Set base field
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_field(entity_id.clone(), "weight", FieldValue::Int(100), &actor, ts),
            &schema,
            &clock,
        )
        .unwrap();

        // Set staging override
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_override(
                entity_id.clone(),
                "weight",
                "staging",
                FieldValue::Int(50),
                &actor,
                ts,
            ),
            &schema,
            &clock,
        )
        .unwrap();

        // Development inherits from staging, so it should get 50
        assert_eq!(
            doc.get_field_for_env(&entity_id, "weight", "development", &schema),
            Some(FieldValue::Int(50))
        );

        // Staging returns its own override
        assert_eq!(
            doc.get_field_for_env(&entity_id, "weight", "staging", &schema),
            Some(FieldValue::Int(50))
        );

        // Production returns base
        assert_eq!(
            doc.get_field_for_env(&entity_id, "weight", "production", &schema),
            Some(FieldValue::Int(100))
        );
    }

    #[test]
    fn development_override_shadows_staging() {
        let schema = test_schema_with_envs();
        let clock = Clock::new();
        let actor = make_actor("alice");
        let mut doc = Document::new();

        let entity_id = EntityId::new("route.api");
        let ts = clock.new_timestamp();

        doc.apply(
            &Operation::insert_entity(entity_id.clone(), "route", None, None, &actor, ts),
            &schema,
            &clock,
        )
        .unwrap();

        // Set base, staging, and development overrides
        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_field(entity_id.clone(), "weight", FieldValue::Int(100), &actor, ts),
            &schema,
            &clock,
        )
        .unwrap();

        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_override(entity_id.clone(), "weight", "staging", FieldValue::Int(50), &actor, ts),
            &schema,
            &clock,
        )
        .unwrap();

        let ts = clock.new_timestamp();
        doc.apply(
            &Operation::set_override(entity_id.clone(), "weight", "development", FieldValue::Int(10), &actor, ts),
            &schema,
            &clock,
        )
        .unwrap();

        // Development returns its own override (10), not staging's (50)
        assert_eq!(
            doc.get_field_for_env(&entity_id, "weight", "development", &schema),
            Some(FieldValue::Int(10))
        );
    }

    #[test]
    fn set_override_unknown_environment_fails() {
        let schema = test_schema_with_envs();
        let clock = Clock::new();
        let actor = make_actor("alice");
        let mut doc = Document::new();

        let entity_id = EntityId::new("route.api");
        let ts = clock.new_timestamp();

        doc.apply(
            &Operation::insert_entity(entity_id.clone(), "route", None, None, &actor, ts),
            &schema,
            &clock,
        )
        .unwrap();

        let ts = clock.new_timestamp();
        let result = doc.apply(
            &Operation::set_override(
                entity_id.clone(),
                "weight",
                "nonexistent",
                FieldValue::Int(50),
                &actor,
                ts,
            ),
            &schema,
            &clock,
        );

        assert!(matches!(result, Err(OperationError::UnknownEnvironment(_))));
    }
}
