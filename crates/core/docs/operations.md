# Operations

## Overview

Operations are the atomic unit of mutation in Conflux. Every change to a document — whether from a human operator, a CI pipeline, or an autoscaler — is expressed as one or more operations.

## Operation Types

### `SetField`

Updates a single field on an entity.

```
SetField {
    entity_id: EntityId,
    field_name: String,
    value: FieldValue,
    actor: ActorId,
    intent: Option<String>,
    timestamp: HlcTimestamp,
}
```

### `InsertEntity`

Adds a new entity as a child of an existing entity.

```
InsertEntity {
    parent_id: EntityId,
    entity_id: EntityId,
    entity_type: String,
    position: InsertPosition,  // Before, After, or Append
    actor: ActorId,
    timestamp: HlcTimestamp,
}
```

### `RemoveEntity`

Marks an entity as removed (tombstone). The entity remains in the document for merge purposes but is excluded from materialized views.

### `MoveEntity`

Changes an entity's position within its parent's children list. Uses a list CRDT (fractional indexing) to handle concurrent moves without conflicts.

### `SetOverride`

Sets an environment-specific value for a field, layered on top of the base value.

## Actor Identity

Every operation carries an `ActorId` that identifies who or what made the change:

```
ActorId {
    id: String,           // Unique identifier
    class: ActorClass,    // Human, Pipeline, Operator, System
    display_name: String, // Human-readable name
}
```

Actor class is used by merge strategies that take actor priority into account.

## Intent Tracking

Operations may carry an optional `intent` string that describes why the change was made. This is surfaced in milestone commit messages and audit logs.
