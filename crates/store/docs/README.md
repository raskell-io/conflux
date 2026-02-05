# conflux-store

Persistent storage layer for Conflux. Implements an append-only operation log, document snapshots, and milestone metadata.

## Overview

`conflux-store` provides durable storage for:

- **Operation Log**: Append-only record of all mutations
- **Snapshots**: Periodic document state for fast recovery
- **Milestones**: Metadata linking document state to git commits

The default (and currently only) backend is SQLite.

## SqliteStore

### Opening a Store

```rust
use conflux_store::SqliteStore;

// Open or create a database file
let store = SqliteStore::open("conflux.db")?;

// In-memory database (useful for testing)
let store = SqliteStore::open_in_memory()?;
```

### Appending Operations

Operations are appended to the log and never modified:

```rust
use conflux_core::Operation;

let op = Operation::set_field(/* ... */);
store.append_operation("doc-1", &op)?;
```

### Querying Operations

Use `OperationQuery` for filtered queries:

```rust
use conflux_store::OperationQuery;

// All operations for a document
let ops = store.query_operations(
    &OperationQuery::new("doc-1")
)?;

// Filter by entity
let ops = store.query_operations(
    &OperationQuery::new("doc-1")
        .for_entity("service.api")
)?;

// Filter by actor
let ops = store.query_operations(
    &OperationQuery::new("doc-1")
        .by_actor("alice")
)?;

// Filter by time range
let ops = store.query_operations(
    &OperationQuery::new("doc-1")
        .since(&start_timestamp)
        .until(&end_timestamp)
)?;

// Filter by operation type
let ops = store.query_operations(
    &OperationQuery::new("doc-1")
        .of_type("set_field")
)?;

// Limit results
let ops = store.query_operations(
    &OperationQuery::new("doc-1")
        .limit(100)
)?;
```

### Operation Count

```rust
let count = store.operation_count("doc-1")?;
println!("Document has {} operations", count);
```

## Snapshots

Snapshots store serialized document state for fast recovery:

```rust
use conflux_core::Document;

// Save a snapshot
store.save_snapshot("doc-1", &document, &hlc_timestamp)?;

// Load the latest snapshot
if let Some(snapshot) = store.latest_snapshot("doc-1")? {
    let document = snapshot.document;
    let as_of = snapshot.hlc_timestamp;
}

// Count operations since last snapshot
let ops_since = store.operations_since_snapshot("doc-1")?;
if ops_since > 1000 {
    // Time for a new snapshot
    store.save_snapshot("doc-1", &current_doc, &now)?;
}
```

### Recovery Pattern

```rust
// Fast document recovery: snapshot + replay
let mut document = if let Some(snapshot) = store.latest_snapshot("doc-1")? {
    snapshot.document
} else {
    Document::new()
};

let ops = store.query_operations(
    &OperationQuery::new("doc-1")
        .since(&snapshot.hlc_timestamp)
)?;

for stored_op in ops {
    document.apply(&stored_op.operation, &schema_info, &clock)?;
}
```

## Milestones

Milestones link document state to git commits:

```rust
// Record a milestone after projecting to git
let milestone_id = store.record_milestone(
    "doc-1",
    Some("abc123def"),  // git commit SHA
    &hlc_start,
    &hlc_end,
    Some("Deploy v2.0"),
)?;

// List milestones (most recent first)
let milestones = store.list_milestones("doc-1")?;
for m in milestones {
    println!("{}: {} ({})", m.id, m.message.unwrap_or_default(), m.git_commit.unwrap_or_default());
}

// Get the latest milestone
if let Some(latest) = store.latest_milestone("doc-1")? {
    println!("Latest: {}", latest.id);
}

// Count operations since last milestone
let ops_since = store.operations_since_milestone("doc-1")?;
println!("{} operations since last milestone", ops_since);
```

## Database Schema

### operations table

| Column | Type | Description |
|--------|------|-------------|
| id | TEXT | Operation UUID (primary key) |
| document_id | TEXT | Document identifier |
| hlc_timestamp | TEXT | HLC timestamp (lexicographically sortable) |
| actor_id | TEXT | Actor identifier |
| actor_class | TEXT | Actor class (human, pipeline, etc.) |
| op_type | TEXT | Operation type (insert_entity, set_field, etc.) |
| entity_id | TEXT | Target entity ID |
| payload | BLOB | Full operation as JSON |
| intent | TEXT | Optional intent message |
| created_at | TEXT | Wall-clock creation time |

### snapshots table

| Column | Type | Description |
|--------|------|-------------|
| id | TEXT | Snapshot UUID (primary key) |
| document_id | TEXT | Document identifier |
| hlc_timestamp | TEXT | HLC timestamp of snapshot |
| data | BLOB | Serialized document as JSON |
| created_at | TEXT | Wall-clock creation time |

### milestones table

| Column | Type | Description |
|--------|------|-------------|
| id | TEXT | Milestone UUID (primary key) |
| document_id | TEXT | Document identifier |
| git_commit | TEXT | Git commit SHA (nullable) |
| hlc_range_start | TEXT | Start of HLC range |
| hlc_range_end | TEXT | End of HLC range |
| message | TEXT | Milestone message (nullable) |
| created_at | TEXT | Wall-clock creation time |

## Indexes

The store creates indexes for efficient queries:

- `(document_id, hlc_timestamp)` - Time-ordered queries
- `(document_id, entity_id)` - Entity filtering
- `(document_id, actor_id)` - Actor filtering
- `(document_id, created_at DESC)` - Recent operations

## Error Handling

```rust
use conflux_store::StoreError;

// Error variants:
// - Database: SQLite error
// - Serialization: JSON serialization error
// - NotFound: Requested item doesn't exist
// - Integrity: Data integrity violation
```

## Thread Safety

`SqliteStore` uses `rusqlite::Connection` internally. For concurrent access:

- Use separate `SqliteStore` instances per thread, or
- Wrap in `Arc<Mutex<SqliteStore>>`

## Dependencies

- `conflux-core`: Core types (Operation, Document, HlcTimestamp)
- `rusqlite`: SQLite database
- `serde_json`: JSON serialization
- `uuid`: Unique identifiers
- `thiserror`: Error handling
