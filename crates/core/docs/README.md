# conflux-core

CRDT document model, typed operations, per-field merge semantics, and causal ordering via HLC.

## Overview

`conflux-core` is the foundational crate that implements the conflict-free replicated data types (CRDTs) and operation model that power Conflux. It provides deterministic merge semantics for concurrent configuration changes from multiple actors.

## Key Types

### Document

The `Document` is the central data structure — a flat collection of entities with typed fields:

```rust
use conflux_core::{Document, EntityId};

let mut doc = Document::new();
let entity = doc.get_entity(&EntityId::new("service.api"));
```

### Entity

An `Entity` represents a single configuration object (e.g., a service, route, or policy):

```rust
use conflux_core::{Entity, EntityId};

// Entities have:
// - entity_id: Unique identifier (e.g., "service.api")
// - entity_type: Schema type (e.g., "service")
// - fields: Map of field name to FieldState
// - tombstone: Soft-delete marker
```

### Operations

All mutations flow through typed `Operation` values:

```rust
use conflux_core::{Operation, FieldValue, ActorId, Clock};

let clock = Clock::new();
let actor = ActorId::new("alice", ActorClass::Human);

// Insert a new entity
let op = Operation::insert_entity(
    "service.api",
    "service",
    None,  // parent
    None,  // position
    &actor,
    clock.new_timestamp(),
);

// Set a field value
let op = Operation::set_field(
    "service.api",
    "replicas",
    FieldValue::Int(3),
    &actor,
    clock.new_timestamp(),
);
```

### FieldValue

Typed field values matching schema field types:

```rust
use conflux_core::FieldValue;

FieldValue::String("hello".into())
FieldValue::Int(42)
FieldValue::Float(3.14)
FieldValue::Bool(true)
FieldValue::List(vec![FieldValue::String("a".into())])
FieldValue::Map(HashMap::new())
FieldValue::Null
```

## CRDT Primitives

Each field uses a CRDT merge strategy defined in the schema:

| Strategy | Type | Behavior |
|----------|------|----------|
| `LwwRegister<T>` | Any | Last-writer-wins by HLC timestamp |
| `MaxRegister` | Numeric | Highest value wins |
| `MinRegister` | Numeric | Lowest value wins |
| `GrowOnlySet<T>` | Set | Elements can only be added |
| `ObservedRemoveSet<T>` | Set | Add/remove with add-wins semantics |
| `ReviewRegister` | Any | Concurrent writes require human review |

### Merge Properties

All CRDT merges satisfy:

- **Commutativity**: `merge(a, b) == merge(b, a)`
- **Associativity**: `merge(merge(a, b), c) == merge(a, merge(b, c))`
- **Idempotency**: `merge(a, a) == a`

These properties are enforced via property-based tests using `proptest`.

## Causal Ordering

Operations are ordered using Hybrid Logical Clocks (HLC):

```rust
use conflux_core::{Clock, HlcTimestamp};

let clock = Clock::new();
let ts1 = clock.new_timestamp();
let ts2 = clock.new_timestamp();

assert!(ts2 > ts1);  // Timestamps are totally ordered
```

HLC timestamps combine:
- Physical time (wall clock)
- Logical counter (for ordering within the same millisecond)
- Node identifier (for tiebreaking)

## Actor Identity

Every operation carries actor identity:

```rust
use conflux_core::{ActorId, ActorClass};

let actor = ActorId::new("deploy-pipeline", ActorClass::Pipeline);

// Actor classes:
// - Human: Interactive user
// - Pipeline: CI/CD automation
// - Operator: Kubernetes operator or controller
// - System: Internal Conflux operations
```

## Applying Operations

Operations are applied to documents through the schema:

```rust
use conflux_core::{Document, Clock, ApplyResult};

let clock = Clock::new();
let mut doc = Document::new();

match doc.apply(&operation, &schema_info, &clock)? {
    ApplyResult::Applied => {
        // Clean merge, no conflicts
    }
    ApplyResult::Conflict(info) => {
        // Field marked for review
        println!("Contending values: {:?}", info.contending_values);
    }
}
```

## Error Handling

```rust
use conflux_core::ConfluxError;

// Error variants:
// - InvalidOperation: Malformed operation
// - EntityNotFound: Target entity doesn't exist
// - TypeMismatch: Value doesn't match field type
// - SchemaViolation: Operation violates schema constraints
```

## Feature Flags

- `serde`: Enable serialization (enabled by default)

## Dependencies

`conflux-core` has no internal Conflux dependencies. It depends on:
- `uhlc`: Hybrid Logical Clocks
- `uuid`: Unique identifiers
- `serde`: Serialization
- `thiserror`: Error handling
