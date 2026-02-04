# conflux-core Architecture

## Overview

The core crate implements the CRDT document model that underpins Conflux. Every config state is represented as a tree of typed entities, where each field carries its own merge semantics and causal history.

## Key Types

### `Document`

The top-level container for a config state. A document holds a tree of `Entity` nodes, each identified by a stable `EntityId`. Documents are the unit of merge — two documents derived from the same schema can always be merged deterministically.

### `Entity`

A named, typed node in the config tree (e.g., a route, an upstream, a listener). Entities contain fields and may have ordered or unordered children.

### `Field`

A single typed value within an entity. Each field carries:
- Current value (`FieldValue`)
- Merge strategy (`MergeStrategy`)
- Causal metadata (`HlcTimestamp`, `ActorId`)
- Conflict state (resolved or pending review)

### `Operation`

An atomic write to the document. Operations are the unit of persistence and replication:
- `SetField` — Update a field value
- `InsertEntity` — Add a new entity to the tree
- `RemoveEntity` — Remove an entity (tombstoned, not deleted)
- `MoveEntity` — Reorder within a parent's children
- `SetOverride` — Set an environment-specific override

### `MergeStrategy`

Per-field merge behavior:

| Strategy | Semantics | CRDT Backing |
|----------|-----------|--------------|
| `Lww` | Last writer wins (HLC + actor tiebreak) | LWW-Register |
| `Max` | Highest numeric value wins | Max-Register |
| `Min` | Lowest numeric value wins | Min-Register |
| `GrowSet` | Elements can be added, never removed | G-Set |
| `OrSet` | Add and remove, concurrent add wins | OR-Set |
| `Review` | Concurrent writes flagged for human review | LWW + conflict flag |

## Causal Ordering

All operations are stamped with a Hybrid Logical Clock (HLC) timestamp. This provides:
- Causal consistency (if A happened before B, A's timestamp < B's timestamp)
- Wall-clock approximation (timestamps are close to real time)
- Total ordering (ties broken by actor ID)

## Merge Algorithm

1. Operations from both sides are sorted by HLC timestamp
2. For each field, the merge strategy determines the winner
3. Entity-level operations (insert, remove, move) use a tree CRDT algorithm
4. Conflicts (where strategy is `Review`) are flagged but not auto-resolved
5. The result is a new document state plus a list of unresolved conflicts
