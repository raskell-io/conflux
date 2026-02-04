# Conflux Code Patterns

Specific patterns for working with Conflux's codebase.

---

## CRDT Patterns

### LWW-Register

Last-writer-wins register, the most common merge strategy:

```rust
pub struct LwwRegister<T> {
    value: T,
    timestamp: HlcTimestamp,
    actor: ActorId,
}

impl<T: Clone + PartialEq> LwwRegister<T> {
    pub fn merge(&self, other: &Self) -> Self {
        match self.timestamp.cmp(&other.timestamp) {
            Ordering::Greater => self.clone(),
            Ordering::Less => other.clone(),
            Ordering::Equal => {
                // Tiebreak by actor ID for determinism
                if self.actor > other.actor {
                    self.clone()
                } else {
                    other.clone()
                }
            }
        }
    }
}
```

### Max-Register

Numeric field where the highest value wins:

```rust
pub struct MaxRegister {
    value: i64,
    timestamp: HlcTimestamp,
    actor: ActorId,
}

impl MaxRegister {
    pub fn merge(&self, other: &Self) -> Self {
        if self.value >= other.value {
            self.clone()
        } else {
            other.clone()
        }
    }
}
```

### OR-Set (Observed-Remove Set)

Set where concurrent add and remove of the same element resolves to add:

```rust
pub struct OrSet<T> {
    elements: HashMap<T, HashSet<UniqueTag>>,
    tombstones: HashSet<UniqueTag>,
}

impl<T: Hash + Eq + Clone> OrSet<T> {
    pub fn add(&mut self, element: T) -> UniqueTag {
        let tag = UniqueTag::new();
        self.elements.entry(element).or_default().insert(tag);
        tag
    }

    pub fn remove(&mut self, element: &T) {
        if let Some(tags) = self.elements.get(element) {
            self.tombstones.extend(tags.iter().cloned());
        }
    }

    pub fn merge(&self, other: &Self) -> Self {
        // Union of elements, union of tombstones, filter out tombstoned tags
        // ...
    }
}
```

---

## Operation Patterns

### Creating Operations

Always use builder functions that enforce required fields:

```rust
// GOOD: Builder enforces identity
let op = Operation::set_field(
    &entity_id,
    "weight",
    FieldValue::Int(80),
    &actor,
    &clock,
)
.with_intent("load-response");

// BAD: Raw struct construction
let op = Operation::SetField {
    entity_id: entity_id.clone(),
    field_name: "weight".into(),
    value: FieldValue::Int(80),
    actor: actor.clone(),
    timestamp: clock.now(),
    intent: None,
};
```

### Applying Operations

Operations are applied to documents through a single entry point:

```rust
// Apply validates against schema, stamps with HLC, and updates state
let result = document.apply(operation, &schema)?;
match result {
    ApplyResult::Merged => { /* Clean merge */ }
    ApplyResult::Conflict(conflict) => { /* Needs review */ }
}
```

---

## Schema Patterns

### Schema Loading

```rust
let schema = Schema::from_file("schema.kdl")?;
schema.validate()?;  // Check internal consistency

// Use schema for merge
let merged = document.merge(&other, &schema)?;

// Use schema for post-merge validation
schema.validate_document(&merged)?;
```

### Field Type Checking

```rust
impl FieldDef {
    pub fn validate_value(&self, value: &FieldValue) -> Result<(), ValidationError> {
        match (&self.field_type, value) {
            (FieldType::Int, FieldValue::Int(_)) => Ok(()),
            (FieldType::String, FieldValue::String(_)) => Ok(()),
            (FieldType::Int, _) => Err(ValidationError::TypeMismatch {
                field: self.name.clone(),
                expected: "int",
                actual: value.type_name(),
            }),
            // ...
        }
    }
}
```

---

## Storage Patterns

### Operation Log Queries

```rust
// Query by entity
let ops = store.operations()
    .for_entity(&entity_id)
    .since(timestamp)
    .collect()?;

// Query by actor
let ops = store.operations()
    .by_actor(&actor_id)
    .in_range(start, end)
    .collect()?;

// Causal range for milestones
let ops = store.operations()
    .since_milestone(&last_milestone_id)
    .collect()?;
```

### Snapshot Pattern

```rust
// Take periodic snapshots for fast reads
if store.operations_since_snapshot() > SNAPSHOT_THRESHOLD {
    let state = document.materialize();
    store.save_snapshot(&state)?;
}

// Load state: snapshot + replay operations since snapshot
let snapshot = store.latest_snapshot()?;
let ops = store.operations_since(&snapshot.hlc_timestamp)?;
let document = Document::from_snapshot(snapshot);
for op in ops {
    document.apply(op, &schema)?;
}
```

---

## Git Projection Patterns

### Milestone Creation

```rust
let projector = MilestoneProjector::new(&git_repo, &schema);

// Gather operations since last milestone
let ops = store.operations_since_milestone(&last_milestone)?;

// Serialize resolved state per environment
let files = projector.serialize(&document, &schema)?;

// Commit with structured message
let commit = projector.commit(
    &files,
    &ops,
    "post-canary traffic shift",
)?;

store.record_milestone(&commit)?;
```

### Format Serialization

```rust
// Serialize based on schema-declared format
match schema.output_format {
    Format::Yaml => serde_yaml::to_string(&state)?,
    Format::Json => serde_json::to_string_pretty(&state)?,
    Format::Toml => toml::to_string(&state)?,
    Format::Kdl => kdl::serialize(&state)?,
}
```

---

## Error Handling Patterns

### Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum ConfluxError {
    #[error("schema validation failed: {0}")]
    SchemaValidation(#[from] ValidationError),

    #[error("merge failed for entity '{entity_id}', field '{field}': {reason}")]
    MergeFailed {
        entity_id: String,
        field: String,
        reason: String,
    },

    #[error("unknown entity type: {0}")]
    UnknownEntityType(String),

    #[error("actor '{actor}' not authorized for operation on '{entity}'")]
    Unauthorized { actor: String, entity: String },

    #[error(transparent)]
    Store(#[from] StoreError),

    #[error(transparent)]
    Git(#[from] GitError),
}
```

### Error Context

```rust
use anyhow::Context;

let schema = Schema::from_file(&path)
    .with_context(|| format!("Failed to load schema from {}", path.display()))?;
```

---

## Testing Patterns

### CRDT Property Tests

```rust
proptest! {
    #[test]
    fn lww_merge_is_commutative(
        v1 in arb_lww_register(),
        v2 in arb_lww_register()
    ) {
        assert_eq!(v1.merge(&v2), v2.merge(&v1));
    }

    #[test]
    fn lww_merge_is_idempotent(v in arb_lww_register()) {
        assert_eq!(v.merge(&v), v);
    }
}
```

### Multi-Actor Scenario Tests

```rust
#[test]
fn two_actors_changing_different_fields_merge_cleanly() {
    let schema = test_schema();
    let mut doc = Document::new(&schema);

    let op1 = Operation::set_field("route.api", "weight", 80, &actor_a, &clock);
    let op2 = Operation::set_field("route.api", "timeout", 5000, &actor_b, &clock);

    doc.apply(op1, &schema).unwrap();
    doc.apply(op2, &schema).unwrap();

    assert_eq!(doc.get_field("route.api", "weight"), Some(&FieldValue::Int(80)));
    assert_eq!(doc.get_field("route.api", "timeout"), Some(&FieldValue::Int(5000)));
}
```
