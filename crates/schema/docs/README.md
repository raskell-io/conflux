# conflux-schema

Schema definition language for Conflux. Declares entity types, field types, merge strategies, and environment overlay rules.

## Overview

`conflux-schema` parses TOML schema files that define the structure and merge behavior of configuration documents. The schema controls:

- What entity types exist
- What fields each entity type has
- How concurrent changes to each field are merged
- Which environments exist and how they inherit

## Schema File Format

Schemas are defined in TOML:

```toml
name = "my-infrastructure"
version = "1.0.0"

[environments]
base = "production"
overlays = [
    { name = "staging", inherits = "production" },
    { name = "development", inherits = "staging" },
]

[entity.service]
fields = [
    { name = "replicas", type = "int", merge = "max", default = 1 },
    { name = "image", type = "string", merge = "lww" },
    { name = "env_vars", type = "map", merge = "lww" },
    { name = "ports", type = "list<int>", merge = "set" },
]

[entity.route]
fields = [
    { name = "path", type = "string", merge = "lww" },
    { name = "service", type = "ref<service>", merge = "lww" },
    { name = "weight", type = "int", merge = "max", default = 100 },
    { name = "timeout", type = "duration", merge = "min" },
]
```

## Field Types

| Type | Description | Example Values |
|------|-------------|----------------|
| `string` | UTF-8 text | `"hello"` |
| `int` | 64-bit signed integer | `42`, `-1` |
| `float` | 64-bit floating point | `3.14` |
| `bool` | Boolean | `true`, `false` |
| `duration` | Time duration | `"30s"`, `"5m"`, `"1h"` |
| `list<T>` | Ordered list of type T | `["a", "b", "c"]` |
| `map` | Key-value pairs | `{"key": "value"}` |
| `ref<E>` | Reference to entity type E | `"service.api"` |

## Merge Strategies

| Strategy | Applicable Types | Behavior |
|----------|-----------------|----------|
| `lww` | Any | Last-writer-wins by timestamp |
| `max` | `int`, `float` | Highest value wins |
| `min` | `int`, `float` | Lowest value wins |
| `set` | `list<T>` | Union of elements (add-wins) |
| `review` | Any | Concurrent changes flagged for review |

### LWW (Last-Writer-Wins)

The most recent write (by HLC timestamp) wins:

```toml
{ name = "image", type = "string", merge = "lww" }
```

### Max/Min

For numeric fields where you want the highest or lowest value:

```toml
{ name = "replicas", type = "int", merge = "max" }   # Scale up wins
{ name = "timeout", type = "duration", merge = "min" }  # Faster timeout wins
```

### Set

For list fields with add-wins semantics:

```toml
{ name = "allowed_ips", type = "list<string>", merge = "set" }
```

### Review

For fields where concurrent changes must be manually reviewed:

```toml
{ name = "critical_config", type = "string", merge = "review" }
```

## Environment Overlays

Environments form an inheritance chain:

```toml
[environments]
base = "production"
overlays = [
    { name = "staging", inherits = "production" },
    { name = "development", inherits = "staging" },
]
```

Field resolution walks up the chain: `development → staging → production`

This means:
- Production values are the base
- Staging can override production
- Development can override staging

## Loading a Schema

```rust
use conflux_schema::Schema;
use std::path::Path;

// From file
let schema = Schema::from_file(Path::new("schema.toml"))?;

// From string
let schema = Schema::from_str(toml_content)?;

// Validate internal consistency
schema.validate()?;
```

## Schema Validation

The schema is validated for:

- Unique entity type names
- Unique field names within each entity
- Valid field types
- Valid merge strategies for field types
- Valid reference targets (ref types point to existing entity types)
- Valid environment inheritance (no cycles)

```rust
let schema = Schema::from_file(path)?;

// Validate a document against the schema
schema.validate_document(&document)?;
```

## SchemaInfo Trait

For decoupling from the full schema in `conflux-core`:

```rust
use conflux_core::SchemaInfo;

impl SchemaInfo for Schema {
    fn merge_strategy(&self, entity_type: &str, field: &str) -> MergeStrategy {
        // Look up merge strategy from schema
    }

    fn field_type(&self, entity_type: &str, field: &str) -> Option<FieldType> {
        // Look up field type from schema
    }
}
```

## Key Types

### Schema

Top-level schema definition:

```rust
pub struct Schema {
    pub name: String,
    pub version: String,
    pub environments: Option<EnvironmentDef>,
    pub entities: HashMap<String, EntityDef>,
}
```

### EntityDef

Entity type definition:

```rust
pub struct EntityDef {
    pub fields: HashMap<String, FieldDef>,
}
```

### FieldDef

Field definition with type and merge strategy:

```rust
pub struct FieldDef {
    pub name: String,
    pub field_type: FieldType,
    pub merge: MergeStrategy,
    pub default: Option<FieldValue>,
    pub required: bool,
}
```

## Error Handling

```rust
use conflux_schema::SchemaError;

// Error variants:
// - ParseError: Invalid TOML syntax
// - ValidationError: Schema validation failed
// - UnknownEntityType: Referenced entity type doesn't exist
// - UnknownField: Field not defined for entity type
// - TypeMismatch: Value doesn't match field type
```

## Dependencies

- `conflux-core`: Core types (FieldValue, MergeStrategy)
- `toml`: TOML parsing
- `serde`: Deserialization
- `thiserror`: Error handling
