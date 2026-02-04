# Conflux

> **Schema-aware config state coordination. Multiple writers, deterministic merge, git milestones.**

Conflux is a daemon + CLI that replaces git as the write path for infrastructure config. Multiple actors (humans, CI pipelines, operators, autoscalers) submit typed operations. Conflux merges them deterministically using per-field CRDT semantics defined in a schema. Resolved state is periodically projected to a git repository as milestones.

## Philosophy (North Star)

1. **Git is the read path, not the write path** — Config changes flow through Conflux's typed operation model, not text file diffs. Git receives periodic milestone snapshots for auditability.
2. **Schema-aware merge over text merge** — Two actors changing different fields on the same entity is not a conflict. The schema defines what is.
3. **Every write has identity and intent** — Operations carry actor ID, actor class, and optional intent. No anonymous mutations.
4. **Convergence is guaranteed, correctness is validated** — CRDTs guarantee convergence. Post-merge validation against the schema catches semantic errors.
5. **Environments are a dimension, not branches** — Per-environment overrides live in the same document, not in diverging git branches.
6. **Simplicity over generality** — Solve the config coordination problem well. Don't become a general-purpose database.

**Before adding anything, ask:**
- Does this help config changes merge more correctly?
- Does this maintain the invariant that git is a projection, not a source of truth?
- Does this keep the operation model simple enough for a human to reason about?

---

## Architecture

```
                    ┌─────────────────────────────────┐
  Actors            │       Conflux Engine             │        Consumers
                    │                                  │
  Human (CLI/UI) ──>│  ┌─────────────────────────────┐ │──> Git repo (milestones)
  CI pipeline ─────>│  │   CRDT Document Store        │ │──> Webhook notifications
  K8s operator ────>│  │                               │ │──> Pull API (reconcilers)
  Autoscaler ──────>│  │   Per-field merge logic       │ │──> gRPC stream (watchers)
  Policy engine ───>│  │   Causal history (HLC)        │ │
                    │  │   Schema + validation         │ │
                    │  └─────────────────────────────┘ │
                    │                                  │
                    │  ┌────────────┐ ┌──────────────┐ │
                    │  │ Env/Stage  │ │  Milestone   │ │
                    │  │ Overlays   │ │  Projector   │ │
                    │  └────────────┘ └──────────────┘ │
                    └─────────────────────────────────┘
```

**Key design choice:** The engine owns all writes. Git is a read-only projection. This eliminates merge conflicts, push races, and branch drift.

---

## Crates

Each crate has its own `docs/` directory with detailed documentation. **When making changes to a crate, update its `docs/` accordingly.**

### Core Crates

#### `conflux-core` (`crates/core/`)
CRDT document model, typed operations, per-field merge semantics, causal ordering via HLC.
- **Key types:** `Document`, `Entity`, `Field`, `Operation`, `MergeStrategy`
- **Docs:** `crates/core/docs/` — architecture, operations

#### `conflux-schema` (`crates/schema/`)
Schema definition language (KDL). Declares entity types, field types, merge strategies, and environment overlay rules.
- **Key types:** `Schema`, `EntityDef`, `FieldDef`, `MergeStrategy`
- **Docs:** `crates/schema/docs/` — schema language, validation

#### `conflux-store` (`crates/store/`)
Persistent storage for the operation log, document snapshots, and milestone metadata. SQLite default backend.
- **Key types:** `Store`, `OpLog`, `Snapshot`
- **Docs:** `crates/store/docs/` — storage model

#### `conflux-git` (`crates/git/`)
Git milestone projection. Serializes resolved state to config file formats and commits to git with structured messages.
- **Key types:** `MilestoneProjector`, `Serializer`, `CommitBuilder`
- **Docs:** `crates/git/docs/` — milestones, serialization

### Supporting Crates

| Crate | Path | Purpose |
|-------|------|---------|
| `conflux-api` | `crates/api/` | HTTP (REST) and gRPC API server |
| `conflux` (CLI) | `crates/cli/` | Command-line interface and daemon entry point |

### Crate Dependencies

```
conflux (cli)
├── conflux-api
│   ├── conflux-core
│   ├── conflux-schema
│   ├── conflux-store
│   └── conflux-git
├── conflux-core
├── conflux-schema
├── conflux-store
└── conflux-git

conflux-git
├── conflux-core
├── conflux-schema
└── conflux-store

conflux-store
├── conflux-core
└── conflux-schema

conflux-schema
└── conflux-core

conflux-core
└── (no internal dependencies)
```

**Dependency rules:**
- `cli` and `api` may depend on all crates
- `git` depends on `core`, `schema`, and `store`
- `store` depends on `core` and `schema`
- `schema` depends only on `core`
- `core` has no internal dependencies

---

## Key Concepts

### Operation Lifecycle

1. **Submit** — Actor sends a typed operation via CLI, HTTP, or gRPC
2. **Validate** — Operation checked against schema (field type, constraints)
3. **Stamp** — HLC timestamp assigned, actor identity recorded
4. **Store** — Operation appended to the persistent log
5. **Merge** — Document state updated using the field's merge strategy
6. **Validate (post-merge)** — Resolved state checked for cross-entity invariants
7. **Notify** — Watchers receive the state change event

### Merge Model

Two concurrent operations on:
- **Different entities** — Always auto-merged, no conflict
- **Same entity, different fields** — Always auto-merged, no conflict
- **Same field** — Merge strategy decides (LWW, max, min, etc.)
- **Same field with `conflict="review"`** — Flagged for human resolution

### Environment Overlays

Environments form an inheritance chain declared in the schema. Field resolution walks up the chain:

```
development → staging → production (base)
```

Promotion between environments is an explicit operation, not a git merge.

---

## Rules

| File | Purpose |
|------|---------|
| [rust-standards.md](rules/rust-standards.md) | Rust coding standards (APIs, error handling, async) |
| [project.md](rules/project.md) | Conflux-specific context and architecture rules |
| [patterns.md](rules/patterns.md) | Code patterns (CRDTs, operations, schema, storage) |
| [workflow.md](rules/workflow.md) | Commands, testing, releases |

---

## Quick Reference

### Common Commands

```bash
# Development
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

# Run locally
cargo run --bin conflux -- daemon --config conflux.toml

# Run specific tests
cargo test -p conflux-core test_name
cargo test -p conflux-schema --lib
```

### Key Files

| Path | Purpose |
|------|---------|
| `crates/core/src/lib.rs` | CRDT document model |
| `crates/schema/src/lib.rs` | Schema parsing entry |
| `crates/store/src/lib.rs` | Storage layer |
| `crates/git/src/lib.rs` | Milestone projection |
| `crates/api/src/lib.rs` | API server |
| `crates/cli/src/main.rs` | CLI entry point |

### Documentation

**When making meaningful changes, documentation must be updated:**

#### 1. Crate-level docs (this repo)
Each crate has a `docs/` directory for technical reference:
```
crates/core/docs/     # CRDT model, operations, merge strategies
crates/schema/docs/   # Schema language, validation
crates/store/docs/    # Storage model, operation log
crates/git/docs/      # Milestones, serialization formats
crates/api/docs/      # HTTP and gRPC API reference
crates/cli/docs/      # CLI usage
```

**Update when:** Changing APIs, adding features, modifying behavior.

---

## Contributing Checklist

Before submitting code:

**Code Quality:**
- [ ] Aligns with philosophy (git is read path, schema-aware merge, identity on every write)
- [ ] CRDTs converge deterministically (property test this)
- [ ] Post-merge validation catches semantic errors
- [ ] Is observable (metrics, logs, traces)
- [ ] Has tests (unit + property-based + integration where applicable)
- [ ] Passes `cargo clippy -- -D warnings`
- [ ] Passes `cargo fmt --check`

**Documentation:**
- [ ] Crate `docs/` updated if API or behavior changed
- [ ] Code comments for non-obvious logic
- [ ] Public API has doc comments with `# Errors` section
