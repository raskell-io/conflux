# Storage

## Overview

The store crate persists Conflux state: the operation log, resolved document snapshots, and actor metadata. The default backend is SQLite for single-node deployments.

## Data Model

### Operations Table

Every operation is stored immutably in arrival order:

| Column | Type | Description |
|--------|------|-------------|
| `id` | UUID | Operation ID |
| `document_id` | UUID | Parent document |
| `hlc_timestamp` | INTEGER | HLC timestamp (packed) |
| `actor_id` | TEXT | Actor who issued the operation |
| `actor_class` | TEXT | human, pipeline, operator, system |
| `op_type` | TEXT | set_field, insert_entity, remove_entity, etc. |
| `payload` | BLOB | Serialized operation data |
| `intent` | TEXT | Optional human-readable reason |
| `created_at` | TIMESTAMP | Wall clock time of receipt |

### Snapshots Table

Periodic materialized snapshots of the resolved document state, used for fast reads and as a base for incremental merge.

### Milestones Table

Records of git milestone projections:

| Column | Type | Description |
|--------|------|-------------|
| `id` | UUID | Milestone ID |
| `document_id` | UUID | Document |
| `git_commit` | TEXT | Git commit SHA |
| `hlc_range_start` | INTEGER | First operation included |
| `hlc_range_end` | INTEGER | Last operation included |
| `message` | TEXT | Milestone message |
| `created_at` | TIMESTAMP | When milestone was created |

## Operation Log

The operation log is append-only. Operations are never modified or deleted. This provides:

- Full audit trail
- Ability to rebuild state from any point
- Causal range queries for milestone projection
