# conflux CLI

Command-line interface for Conflux. Provides commands for managing configuration state, viewing history, and running the daemon.

## Installation

```bash
cargo install conflux
```

Or build from source:

```bash
cargo build --release -p conflux
```

## Quick Start

```bash
# Initialize a new project
conflux init --schema schema.toml

# Import existing configuration
conflux import ./configs/ --format yaml

# Set a field value
conflux set service.api replicas 5 --intent "Scale for traffic"

# View current state
conflux get service.api

# Create a milestone (commit to git)
conflux milestone create -m "Deploy v2.0"

# Run the API server
conflux daemon
```

## Global Options

```
-c, --config <PATH>  Path to configuration file (default: searches for conflux.toml)
-h, --help           Print help
-V, --version        Print version
```

## Commands

### init

Initialize a new Conflux project.

```bash
conflux init [OPTIONS]
```

**Options:**
- `--schema <PATH>` - Path to schema file (default: schema.toml)
- `--database <PATH>` - Database path (default: .conflux/conflux.db)
- `--document-id <ID>` - Document identifier (default: main)
- `--force` - Overwrite existing files

**Example:**
```bash
conflux init --schema infrastructure.toml --document-id prod
```

Creates:
- `conflux.toml` - Project configuration
- `schema.toml` - Schema file (if not exists)
- `.conflux/` - Data directory

### import

Import existing configuration files.

```bash
conflux import <PATH> [OPTIONS]
```

**Options:**
- `--format <FORMAT>` - File format: json, yaml, toml (default: auto-detect)
- `--entity-type <TYPE>` - Entity type for imported items
- `--actor <ID>` - Actor identity
- `--actor-class <CLASS>` - Actor class
- `--dry-run` - Show what would be imported

**Example:**
```bash
conflux import ./services/ --entity-type service --format yaml
```

### set

Set a field value on an entity.

```bash
conflux set <ENTITY_ID> <FIELD> <VALUE> [OPTIONS]
```

**Options:**
- `-t, --entity-type <TYPE>` - Entity type (required if entity doesn't exist)
- `--actor <ID>` - Actor identity
- `--actor-class <CLASS>` - Actor class
- `-i, --intent <MESSAGE>` - Intent message

**Examples:**
```bash
# Set a field on existing entity
conflux set service.api replicas 5

# Create entity and set field
conflux set service.worker image "worker:v1.0" -t service

# With intent
conflux set route.api weight 80 --intent "Canary deployment"
```

### get

Get entity or field values.

```bash
conflux get [ENTITY_ID] [FIELD] [OPTIONS]
```

**Options:**
- `-o, --output <FORMAT>` - Output format: plain, json, yaml (default: plain)
- `--all` - Show all entities

**Examples:**
```bash
# Get all entities
conflux get --all

# Get specific entity
conflux get service.api

# Get specific field
conflux get service.api replicas

# JSON output
conflux get service.api -o json
```

### diff

Show differences between states.

```bash
conflux diff [OPTIONS]
```

**Options:**
- `--since <MILESTONE>` - Compare since milestone
- `--entity <ID>` - Filter by entity
- `-o, --output <FORMAT>` - Output format

**Example:**
```bash
conflux diff --since abc123
```

### log

Show the operation log.

```bash
conflux log [OPTIONS]
```

**Options:**
- `-n, --limit <N>` - Number of operations to show (default: 20)
- `--entity <ID>` - Filter by entity
- `--actor <ID>` - Filter by actor
- `--since <TIMESTAMP>` - Filter by time
- `-o, --output <FORMAT>` - Output format

**Example:**
```bash
conflux log --entity service.api --limit 50
```

### blame

Show who changed what (per-field attribution).

```bash
conflux blame <ENTITY_ID> [OPTIONS]
```

**Options:**
- `--field <NAME>` - Show only specific field
- `-o, --output <FORMAT>` - Output format

**Example:**
```bash
conflux blame service.api
```

Output:
```
service.api (service)
  replicas = 5
    alice (human) at 2024-01-15T10:30:00Z
    Intent: Scale for traffic

  image = "api:v2.0"
    deploy-pipeline (pipeline) at 2024-01-15T09:00:00Z
```

### status

Show document status.

```bash
conflux status [OPTIONS]
```

**Options:**
- `-o, --output <FORMAT>` - Output format

**Example:**
```bash
conflux status
```

Output:
```
Document: main
Schema:   infrastructure v1.0.0

Entities: 15 (14 active, 1 deleted)
  service: 5
  route: 8
  policy: 1

Operations: 234
Fields: 89
Milestones: 12
  Latest: abc123de (5 ops since)
  Message: Deploy v2.0
```

### milestone

Milestone management.

#### milestone create

Create a new milestone (project to git).

```bash
conflux milestone create [OPTIONS]
```

**Options:**
- `-m, --message <MESSAGE>` - Milestone message (required)
- `-e, --environments <LIST>` - Environments to include (default: production)
- `--dry-run` - Show what would be committed

**Example:**
```bash
conflux milestone create -m "Deploy v2.0" -e production,staging
```

#### milestone list

List all milestones.

```bash
conflux milestone list [OPTIONS]
```

**Options:**
- `-n, --limit <N>` - Maximum number to show (default: 20)
- `-o, --output <FORMAT>` - Output format

#### milestone show

Show details of a specific milestone.

```bash
conflux milestone show <ID> [OPTIONS]
```

**Options:**
- `-o, --output <FORMAT>` - Output format

### promote

Promote configuration between environments.

```bash
conflux promote <FROM> <TO> [OPTIONS]
```

**Options:**
- `--entity <ID>` - Specific entity to promote
- `--field <NAME>` - Specific field to promote
- `--dry-run` - Show what would be promoted

**Example:**
```bash
conflux promote staging production --dry-run
```

*Note: Full promotion support requires SetOverride operations in core.*

### daemon

Run the Conflux API server.

```bash
conflux daemon [OPTIONS]
```

**Options:**
- `--host <HOST>` - Bind address (default: from config or 127.0.0.1)
- `--port <PORT>` - Bind port (default: from config or 8080)

**Example:**
```bash
conflux daemon --port 9000
```

## Configuration File

The `conflux.toml` configuration file:

```toml
# Schema file path (relative to config directory)
schema = "schema.toml"

# Database path (relative to config directory)
database = ".conflux/conflux.db"

# Document identifier
document_id = "main"

# Server configuration (for daemon)
[server]
host = "127.0.0.1"
port = 8080

# Git configuration (for milestones)
[git]
repo = "./config-repo"
format = "yaml"
author_name = "Conflux"
author_email = "conflux@example.com"
```

## Actor Identity

Actor identity is resolved in order:

1. `--actor` and `--actor-class` flags
2. `CONFLUX_ACTOR` and `CONFLUX_ACTOR_CLASS` environment variables
3. System username with class `human`

**Actor classes:**
- `human` - Interactive user
- `pipeline` - CI/CD automation
- `operator` - Kubernetes operator or controller
- `system` - Internal Conflux operations

## Output Formats

Most commands support multiple output formats:

- `plain` - Human-readable text (default)
- `json` - JSON for programmatic use
- `yaml` - YAML for readability

```bash
conflux get service.api -o json | jq '.fields.replicas'
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `CONFLUX_ACTOR` | Default actor identity |
| `CONFLUX_ACTOR_CLASS` | Default actor class |
| `CONFLUX_CONFIG` | Path to configuration file |
| `RUST_LOG` | Log level (e.g., `debug`, `conflux=trace`) |
