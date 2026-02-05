# conflux-api

HTTP REST API server for Conflux. Provides endpoints for managing configuration state, operations, and milestones.

## Overview

`conflux-api` exposes Conflux functionality over HTTP, enabling:

- Remote configuration management
- Integration with CI/CD pipelines
- Web-based UIs
- Programmatic access from any language

## Starting the Server

```rust
use conflux_api::{AppState, run_server};
use conflux_schema::Schema;
use conflux_store::SqliteStore;
use std::sync::Arc;

let schema = Schema::from_file("schema.toml")?;
let store = SqliteStore::open("conflux.db")?;

let state = AppState::new(schema, store, "doc-1");

run_server(state, "127.0.0.1:8080").await?;
```

### With Milestone Projector

```rust
use conflux_git::{MilestoneProjector, ProjectorConfig};

let projector = MilestoneProjector::new(projector_config);
let state = AppState::new_with_projector(
    schema,
    store,
    "doc-1",
    Some(Arc::new(projector)),
);
```

## API Endpoints

### Health Check

```
GET /health
```

Returns server health status.

**Response:**
```json
{
  "status": "ok"
}
```

### Entities

#### List Entities

```
GET /entities
```

Returns all entities in the document.

**Response:**
```json
{
  "entities": [
    {
      "id": "service.api",
      "type": "service",
      "fields": {
        "replicas": 3,
        "image": "api:v2.0"
      }
    }
  ]
}
```

#### Get Entity

```
GET /entities/{entity_id}
```

Returns a single entity by ID.

**Response:**
```json
{
  "id": "service.api",
  "type": "service",
  "fields": {
    "replicas": 3,
    "image": "api:v2.0"
  }
}
```

#### Create Entity

```
POST /entities
Content-Type: application/json

{
  "id": "service.worker",
  "type": "service",
  "actor": "alice",
  "actor_class": "human",
  "intent": "Add worker service"
}
```

**Response:**
```json
{
  "id": "service.worker",
  "type": "service",
  "fields": {}
}
```

#### Delete Entity

```
DELETE /entities/{entity_id}
X-Actor: alice
X-Actor-Class: human
```

### Fields

#### Get Field

```
GET /entities/{entity_id}/fields/{field_name}
```

**Response:**
```json
{
  "entity_id": "service.api",
  "field": "replicas",
  "value": 3
}
```

#### Set Field

```
PUT /entities/{entity_id}/fields/{field_name}
Content-Type: application/json
X-Actor: alice
X-Actor-Class: human

{
  "value": 5,
  "intent": "Scale up for traffic"
}
```

**Response:**
```json
{
  "entity_id": "service.api",
  "field": "replicas",
  "value": 5,
  "result": "applied"
}
```

### Operations

#### List Operations

```
GET /operations?entity={entity_id}&actor={actor_id}&limit={n}
```

Query parameters:
- `entity`: Filter by entity ID
- `actor`: Filter by actor ID
- `since`: Filter by HLC timestamp (operations after)
- `until`: Filter by HLC timestamp (operations before)
- `limit`: Maximum number of results

**Response:**
```json
{
  "operations": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "type": "set_field",
      "entity_id": "service.api",
      "field": "replicas",
      "value": 5,
      "actor": "alice",
      "actor_class": "human",
      "timestamp": "2024-01-15T10:30:00.000Z/1/node1",
      "intent": "Scale up for traffic"
    }
  ]
}
```

#### Get Operation

```
GET /operations/{operation_id}
```

### Milestones

#### List Milestones

```
GET /milestones?limit={n}
```

**Response:**
```json
{
  "milestones": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "git_commit": "abc123def",
      "message": "Deploy v2.0",
      "created_at": "2024-01-15T10:35:00Z",
      "hlc_range": {
        "start": "2024-01-15T10:30:00.000Z/1",
        "end": "2024-01-15T10:35:00.000Z/3"
      }
    }
  ]
}
```

#### Create Milestone

```
POST /milestones
Content-Type: application/json

{
  "message": "Deploy v2.0",
  "environments": ["production"]
}
```

**Response:**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "git_commit": "abc123def",
  "files_written": ["production/services/api.yaml"]
}
```

#### Get Milestone

```
GET /milestones/{milestone_id}
```

### Document Status

```
GET /status
```

**Response:**
```json
{
  "document_id": "doc-1",
  "entity_count": 15,
  "operation_count": 234,
  "operations_since_milestone": 5,
  "latest_milestone": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "git_commit": "abc123def"
  }
}
```

## Request Headers

| Header | Description | Required |
|--------|-------------|----------|
| `X-Actor` | Actor identifier | For mutations |
| `X-Actor-Class` | Actor class (human, pipeline, operator, system) | For mutations |
| `Content-Type` | Must be `application/json` for POST/PUT | Yes |

## Error Responses

Errors return appropriate HTTP status codes with JSON bodies:

```json
{
  "error": "entity_not_found",
  "message": "Entity 'service.unknown' does not exist"
}
```

| Status | Meaning |
|--------|---------|
| 400 | Bad request (invalid JSON, missing fields) |
| 404 | Entity or resource not found |
| 409 | Conflict (requires review) |
| 422 | Validation error (schema violation) |
| 500 | Internal server error |

## AppState

The server state holds shared resources:

```rust
pub struct AppState {
    pub schema: Schema,
    pub store: SqliteStore,
    pub document_id: String,
    pub document: RwLock<Document>,
    pub clock: Clock,
    pub projector: Option<Arc<MilestoneProjector>>,
}
```

## Middleware

The API server includes:

- **Tracing**: Request/response logging with `tracing`
- **CORS**: Configurable cross-origin support
- **Error Handling**: Consistent error response format

## Dependencies

- `conflux-core`: Core types
- `conflux-schema`: Schema validation
- `conflux-store`: Persistent storage
- `conflux-git`: Milestone projection (optional)
- `axum`: HTTP framework
- `tokio`: Async runtime
- `tower-http`: HTTP middleware
- `tracing`: Observability
