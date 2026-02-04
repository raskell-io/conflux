# Schema Language

## Overview

Conflux schemas define the structure of config documents: what entities exist, what fields they have, how fields merge under concurrent writes, and how environments layer overrides. Schemas are written in TOML.

## Example

```toml
[schema]
name = "sentinel-config"
version = "1.0"

[entity.listener]
fields = [
    { name = "address",  type = "string",   merge = "lww" },
    { name = "protocol", type = "string",   merge = "lww", values = "http,https" },
    { name = "timeout",  type = "duration", merge = "lww" },
]

[entity.route]
fields = [
    { name = "path",     type = "string",   merge = "lww", conflict = "review" },
    { name = "weight",   type = "int",      merge = "max", range = "0,100" },
    { name = "timeout",  type = "duration", merge = "lww" },
    { name = "upstream", type = "ref",      target = "upstream", merge = "lww" },
]

[entity.route.children]
filters = { type = "filter", merge = "set" }

[entity.upstream]
fields = [
    { name = "targets",      type = "list<address>", merge = "grow-set" },
    { name = "health-check", type = "bool",          merge = "lww" },
]

[entity.filter]
fields = [
    { name = "type",   type = "string", merge = "lww" },
    { name = "config", type = "map",    merge = "lww" },
]

[environments]
base = "production"

[environments.overlays]
staging = { inherits = "production" }
development = { inherits = "staging" }
```

## Field Types

| Type | Description | Example Values |
|------|-------------|----------------|
| `string` | UTF-8 string | `"api-v1"`, `"0.0.0.0:8080"` |
| `int` | 64-bit signed integer | `80`, `5000` |
| `float` | 64-bit floating point | `0.95`, `1.5` |
| `bool` | Boolean | `true`, `false` |
| `duration` | Time duration | `"30s"`, `"200ms"` |
| `ref` | Reference to another entity | `"backend"` (upstream name) |
| `list<T>` | Ordered list of values | `["10.0.1.1:80", "10.0.1.2:80"]` |
| `map` | Key-value map | `{"key": "value"}` |

## Merge Strategies

| Strategy | Description | Use When |
|----------|-------------|----------|
| `lww` | Last writer wins (by HLC timestamp) | Most fields — simple, predictable |
| `max` | Highest numeric value wins | Weights, capacities, limits |
| `min` | Lowest numeric value wins | Timeouts, thresholds |
| `grow-set` | Elements can be added, never removed | Target lists, allow-lists |
| `or-set` | Add/remove with add-wins semantics | Dynamic membership |
| `review` | Concurrent writes flagged for human review | Critical fields (paths, auth) |

## Conflict Attribute

The optional `conflict` attribute on a field controls what happens when the merge strategy alone can't resolve concurrent writes:

- `conflict="auto"` (default) — merge strategy decides, no flagging
- `conflict="review"` — flag for human review even if merge strategy could decide

## Validation Attributes

| Attribute | Description |
|-----------|-------------|
| `range="min,max"` | Numeric range constraint |
| `values="a,b,c"` | Enum constraint |
| `pattern="regex"` | Regex validation |
| `required` | Field must have a value |

## Environment Overlays

Environments form an inheritance chain. A field value is resolved by walking up the chain until a value is found:

```
development → staging → production (base)
```

Setting a field in `staging` overrides it for both `staging` and `development`, unless `development` has its own override.
