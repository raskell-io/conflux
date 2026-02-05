# conflux-git

Git milestone projection for Conflux. Serializes resolved document state to configuration files and commits to a git repository.

## Overview

`conflux-git` projects Conflux document state to git repositories as "milestones". This enables:

- **Auditability**: Git history shows configuration evolution
- **Tooling Compatibility**: Existing tools read from git
- **Rollback**: Git provides a recovery mechanism
- **Code Review**: PRs can review projected config changes

## Key Concept: Git is a Projection

Conflux is the source of truth. Git receives periodic snapshots. If someone edits git directly, the next milestone overwrites their changes.

```
Conflux (source of truth) → Project → Git (read-only projection)
```

## MilestoneProjector

### Configuration

```rust
use conflux_git::{MilestoneProjector, ProjectorConfig, OutputFormat};
use std::path::PathBuf;

let config = ProjectorConfig {
    repo_path: PathBuf::from("./config-repo"),
    default_format: OutputFormat::Yaml,
    author_name: "Conflux".to_string(),
    author_email: "conflux@example.com".to_string(),
};

let projector = MilestoneProjector::new(config);
```

### Projecting a Milestone

```rust
use conflux_core::Document;

let document: Document = /* ... */;
let operations: Vec<Operation> = /* ops since last milestone */;
let environments = vec!["production".to_string()];
let message = "Deploy v2.0";

let result = projector.project(
    &document,
    operations,
    &environments,
    message,
)?;

println!("Commit: {}", result.commit_sha);
println!("Files: {:?}", result.files_written);
```

### ProjectionResult

```rust
pub struct ProjectionResult {
    pub commit_sha: String,           // Git commit SHA
    pub files_written: Vec<String>,   // Files that were written
    pub operation_count: usize,       // Number of operations included
    pub hlc_start: Option<HlcTimestamp>,  // First operation timestamp
    pub hlc_end: Option<HlcTimestamp>,    // Last operation timestamp
}
```

## Output Formats

Conflux can serialize to multiple configuration formats:

| Format | Extension | Use Case |
|--------|-----------|----------|
| YAML | `.yaml` | Kubernetes, Docker Compose |
| JSON | `.json` | Generic, JavaScript tools |
| TOML | `.toml` | Rust, Python tools |
| KDL | `.kdl` | Modern config format |
| XML | `.xml` | Legacy systems |
| HCL | `.tf` | Terraform |

### Setting the Format

```rust
use conflux_git::OutputFormat;

let config = ProjectorConfig {
    default_format: OutputFormat::Yaml,
    // ...
};
```

### Format-Specific Serialization

```rust
use conflux_git::Serializer;

let serializer = Serializer::new(OutputFormat::Yaml);
let content = serializer.serialize(&entity)?;
```

## File Layout

The projector writes files based on entity type and ID:

```
config-repo/
├── production/
│   ├── services/
│   │   ├── api.yaml
│   │   └── worker.yaml
│   └── routes/
│       ├── frontend.yaml
│       └── api.yaml
└── staging/
    └── services/
        └── api.yaml  (staging override)
```

## Environment Projection

Each environment gets its own directory with resolved values:

```rust
let environments = vec![
    "production".to_string(),
    "staging".to_string(),
];

// Projects to:
// - production/ (base values)
// - staging/ (production + staging overrides)
```

## Commit Messages

Milestones generate structured commit messages:

```
conflux: Deploy v2.0

Operations: 5
Entities modified: 3
  - service.api (2 operations)
  - route.frontend (2 operations)
  - route.api (1 operation)

HLC Range: 2024-01-15T10:30:00.000Z/1 - 2024-01-15T10:35:00.000Z/3
```

## Git Operations

### Repository Initialization

If the target path isn't a git repository, `MilestoneProjector` can initialize it:

```rust
// The projector handles git init if needed
let projector = MilestoneProjector::new(config);
```

### Atomic Commits

Each milestone creates a single atomic commit with all file changes:

1. Write all entity files
2. Stage changes
3. Create commit with structured message

## Integration with Store

Typically used with `conflux-store` to track milestones:

```rust
use conflux_store::SqliteStore;
use conflux_git::MilestoneProjector;

// Project to git
let result = projector.project(&document, operations, &environments, message)?;

// Record milestone in store
store.record_milestone(
    "doc-1",
    Some(&result.commit_sha),
    &result.hlc_start.unwrap(),
    &result.hlc_end.unwrap(),
    Some(message),
)?;
```

## Error Handling

```rust
use conflux_git::GitError;

// Error variants:
// - RepositoryError: Git repository operation failed
// - SerializationError: Failed to serialize entity
// - IoError: File system operation failed
// - InvalidPath: Invalid file path
```

## Dependencies

- `conflux-core`: Core types (Document, Entity, Operation)
- `conflux-schema`: Schema for type information
- `git2`: Git operations (libgit2 bindings)
- `serde_yaml`: YAML serialization
- `serde_json`: JSON serialization
- `toml`: TOML serialization
- `kdl`: KDL serialization
- `quick-xml`: XML serialization
- `hcl-rs`: HCL/Terraform serialization
