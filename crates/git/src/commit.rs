//! Commit message builder for structured milestone commits.

use conflux_core::entity::ConflictInfo;
use conflux_core::{HlcTimestamp, Operation, OperationKind};

/// Builder for structured milestone commit messages.
///
/// Creates commit messages following the format:
/// ```text
/// milestone: <message>
///
/// Operations since last milestone:
///   [<actor>] <entity>.<field>: <value> (<intent>)
///   ...
///
/// Conflicts:
///   <entity>.<field>: <count> contending values
///   ...
///
/// Environments affected: <list>
/// Causal range: hlc:<start> -> hlc:<end>
/// Operations: <count>
/// Conflicts: <count>
/// ```
#[derive(Debug, Clone)]
pub struct CommitBuilder {
    message: String,
    operations: Vec<Operation>,
    environments: Vec<String>,
    conflicts: Vec<ConflictInfo>,
    hlc_start: Option<HlcTimestamp>,
    hlc_end: Option<HlcTimestamp>,
}

impl CommitBuilder {
    /// Creates a new commit builder with the given milestone message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            operations: Vec::new(),
            environments: Vec::new(),
            conflicts: Vec::new(),
            hlc_start: None,
            hlc_end: None,
        }
    }

    /// Adds operations to include in the commit message.
    pub fn with_operations(mut self, operations: Vec<Operation>) -> Self {
        if !operations.is_empty() {
            // Track causal range from operations
            let first = operations.first().map(|op| op.timestamp);
            let last = operations.last().map(|op| op.timestamp);

            if self.hlc_start.is_none() || first < self.hlc_start {
                self.hlc_start = first;
            }
            if self.hlc_end.is_none() || last > self.hlc_end {
                self.hlc_end = last;
            }
        }
        self.operations = operations;
        self
    }

    /// Sets the environments affected by this milestone.
    pub fn with_environments(mut self, environments: Vec<String>) -> Self {
        self.environments = environments;
        self
    }

    /// Sets the causal range explicitly (overrides operation-derived range).
    pub fn with_causal_range(mut self, start: HlcTimestamp, end: HlcTimestamp) -> Self {
        self.hlc_start = Some(start);
        self.hlc_end = Some(end);
        self
    }

    /// Adds conflict information to the commit.
    pub fn with_conflicts(mut self, conflicts: Vec<ConflictInfo>) -> Self {
        self.conflicts = conflicts;
        self
    }

    /// Builds the structured commit message.
    pub fn build(self) -> String {
        let mut msg = format!("milestone: {}\n", self.message);

        // Operations section
        if !self.operations.is_empty() {
            msg.push_str("\nOperations since last milestone:\n");
            for op in &self.operations {
                let line = format_operation(op);
                msg.push_str(&format!("  {line}\n"));
            }
        }

        // Conflicts section
        if !self.conflicts.is_empty() {
            msg.push_str("\nConflicts requiring review:\n");
            for conflict in &self.conflicts {
                let env_suffix = conflict
                    .environment
                    .as_ref()
                    .map(|e| format!("@{e}"))
                    .unwrap_or_default();
                msg.push_str(&format!(
                    "  {}.{}{}: {} contending values\n",
                    conflict.entity_id,
                    conflict.field,
                    env_suffix,
                    conflict.contending_values.len()
                ));
            }
        }

        // Metadata section
        msg.push('\n');

        if !self.environments.is_empty() {
            msg.push_str(&format!(
                "Environments affected: {}\n",
                self.environments.join(", ")
            ));
        }

        if let (Some(start), Some(end)) = (self.hlc_start, self.hlc_end) {
            msg.push_str(&format!("Causal range: hlc:{start} -> hlc:{end}\n"));
        }

        msg.push_str(&format!("Operations: {}\n", self.operations.len()));

        if !self.conflicts.is_empty() {
            msg.push_str(&format!("Conflicts: {}\n", self.conflicts.len()));
        }

        msg
    }
}

/// Formats a single operation for the commit message.
fn format_operation(op: &Operation) -> String {
    let actor = &op.actor.id;
    let intent = op
        .intent
        .as_ref()
        .map(|i| format!(" ({i})"))
        .unwrap_or_default();

    match &op.kind {
        OperationKind::SetField {
            entity_id, field, ..
        } => {
            format!("[{actor}] {entity_id}.{field}: set{intent}")
        }
        OperationKind::InsertEntity {
            entity_id,
            entity_type,
            ..
        } => {
            format!("[{actor}] {entity_id}: insert {entity_type}{intent}")
        }
        OperationKind::RemoveEntity { entity_id } => {
            format!("[{actor}] {entity_id}: remove{intent}")
        }
        OperationKind::MoveEntity {
            entity_id,
            new_parent_id,
            ..
        } => {
            format!("[{actor}] {entity_id}: move to {new_parent_id}{intent}")
        }
        OperationKind::SetOverride {
            entity_id,
            field,
            environment,
            ..
        } => {
            format!("[{actor}] {entity_id}.{field}@{environment}: override{intent}")
        }
        OperationKind::ResolveConflict {
            entity_id,
            field,
            environment,
            ..
        } => {
            if let Some(env) = environment {
                format!("[{actor}] {entity_id}.{field}@{env}: resolve{intent}")
            } else {
                format!("[{actor}] {entity_id}.{field}: resolve{intent}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conflux_core::{ActorClass, ActorId, Clock, FieldValue};

    fn test_actor(name: &str) -> ActorId {
        ActorId::new(name, ActorClass::Human)
    }

    #[test]
    fn commit_message_basic() {
        let msg = CommitBuilder::new("deploy v1.2.3").build();
        assert!(msg.starts_with("milestone: deploy v1.2.3\n"));
        assert!(msg.contains("Operations: 0"));
    }

    #[test]
    fn commit_message_with_operations() {
        let clock = Clock::new();
        let actor = test_actor("alice");
        let ops = vec![
            Operation::set_field(
                "route.api",
                "weight",
                FieldValue::Int(80),
                &actor,
                clock.new_timestamp(),
            )
            .with_intent("load test"),
            Operation::insert_entity(
                "route.web",
                "route",
                None,
                None,
                &actor,
                clock.new_timestamp(),
            ),
        ];

        let msg = CommitBuilder::new("weekly release")
            .with_operations(ops)
            .with_environments(vec!["production".into(), "staging".into()])
            .build();

        assert!(msg.contains("milestone: weekly release"));
        assert!(msg.contains("[alice] route.api.weight: set (load test)"));
        assert!(msg.contains("[alice] route.web: insert route"));
        assert!(msg.contains("Environments affected: production, staging"));
        assert!(msg.contains("Operations: 2"));
        assert!(msg.contains("Causal range: hlc:"));
    }

    #[test]
    fn commit_message_with_explicit_range() {
        let clock = Clock::new();
        let ts1 = clock.new_timestamp();
        let ts2 = clock.new_timestamp();

        let msg = CommitBuilder::new("test")
            .with_causal_range(ts1, ts2)
            .build();

        assert!(msg.contains("Causal range: hlc:"));
    }

    #[test]
    fn commit_message_with_conflicts() {
        use conflux_core::entity::ConflictInfo;
        use conflux_core::{EntityId, FieldValue};

        let conflicts = vec![
            ConflictInfo {
                entity_id: EntityId::new("route.api"),
                field: "weight".to_string(),
                environment: None,
                contending_values: vec![FieldValue::Int(80), FieldValue::Int(90)],
            },
            ConflictInfo {
                entity_id: EntityId::new("route.web"),
                field: "timeout".to_string(),
                environment: Some("staging".to_string()),
                contending_values: vec![FieldValue::Int(5000), FieldValue::Int(3000)],
            },
        ];

        let msg = CommitBuilder::new("release with conflicts")
            .with_conflicts(conflicts)
            .build();

        assert!(msg.contains("milestone: release with conflicts"));
        assert!(msg.contains("Conflicts requiring review:"));
        assert!(msg.contains("route.api.weight: 2 contending values"));
        assert!(msg.contains("route.web.timeout@staging: 2 contending values"));
        assert!(msg.contains("Conflicts: 2"));
    }
}
