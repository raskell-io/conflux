<div align="center">

<h1 align="center">
  Conflux
</h1>

<p align="center">
  <em>Schema-aware config state coordination.</em><br>
  <em>Multiple writers, deterministic merge, git milestones.</em>
</p>

<p align="center">
  <a href="https://www.rust-lang.org/">
    <img alt="Rust" src="https://img.shields.io/badge/Rust-stable-000000?logo=rust&logoColor=white&style=for-the-badge">
  </a>
  <a href="LICENSE">
    <img alt="License" src="https://img.shields.io/badge/License-Apache--2.0-c6a0f6?style=for-the-badge">
  </a>
</p>

<p align="center">
  <a href="https://github.com/raskell-io/conflux/discussions">Discussions</a> •
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

</div>

---

Conflux is a daemon + CLI that replaces git as the write path for infrastructure config. Multiple actors — humans, CI pipelines, operators, autoscalers — submit typed operations through a schema-aware engine that merges them deterministically. Resolved state is periodically projected to a git repository as milestones.

**The problem:** GitOps treats config files as source code. Multiple actors editing config race to push, create mechanical merge conflicts on structured data git doesn't understand, and diverge across long-lived environment branches. Git's text-level merge is the wrong primitive for typed config state.

**The fix:** Move writes off git. Let a schema-aware engine handle concurrent modifications using per-field merge strategies (CRDT-backed). Project clean, auditable snapshots to git on your terms.

## Status

Early development. Core CRDT model and schema language are being implemented. Not yet usable.

## Quick Start

```bash
# Install
cargo install conflux

# Initialize a project with a schema
conflux init --schema ./schema.kdl --git-remote git@github.com:org/infra-config.git

# Import existing config files
conflux import ./configs/ --env production

# Make a change
conflux set route.api-v1.weight 80 --reason "shifting traffic for canary"

# See pending changes
conflux status

# Snapshot to git
conflux milestone --message "post-canary traffic shift"
```

## How It Works

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

1. **Actors submit typed operations** — `SetField(route.api-v1, weight, 80, actor=autoscaler)`, not a text diff
2. **Schema defines merge rules** — Each field declares its merge strategy (last-writer-wins, max-wins, add-wins set, require-review)
3. **Engine merges deterministically** — CRDT-backed, per-field, causal ordering via hybrid logical clocks
4. **State is validated post-merge** — Schema constraints catch semantic errors that clean merges can't
5. **Milestones project to git** — Periodic snapshots with structured commit messages and full causal attribution

## Features

| Feature | Description |
|---------|-------------|
| **Schema-Aware Merge** | Per-field merge strategies: LWW, max, min, grow-set, OR-set, require-review |
| **Multi-Actor** | Humans, CI pipelines, operators, autoscalers — all identified, all tracked |
| **Causal Ordering** | Hybrid logical clocks for causally consistent, totally ordered operations |
| **Environment Overlays** | Environments as a data dimension, not git branches. Promote with an operation, not a merge |
| **Git Milestones** | Periodic snapshots to git with structured commit messages and operation attribution |
| **Format Agnostic** | Import and export YAML, JSON, TOML, KDL. The CRDT layer is format-independent |
| **Operation Log** | Append-only audit trail of every mutation with actor identity and intent |
| **Post-Merge Validation** | Schema constraints enforced after every merge — convergence + correctness |

### Schema Example

Define your config structure and merge rules in KDL:

```kdl
schema "my-config" version="1.0" {

    entity "route" {
        field "path"     type="string"   merge="lww"  conflict="review"
        field "weight"   type="int"      merge="max"  range="0,100"
        field "timeout"  type="duration" merge="lww"
        field "upstream" type="ref"      target="upstream" merge="lww"
    }

    entity "upstream" {
        field "targets"      type="list<address>" merge="grow-set"
        field "health-check" type="bool"          merge="lww"
    }

    environments {
        base "production"
        overlay "staging"     inherits="production"
        overlay "development" inherits="staging"
    }
}
```

### Merge Strategies

| Strategy | Semantics | CRDT Backing |
|----------|-----------|--------------|
| `lww` | Last writer wins (HLC + actor tiebreak) | LWW-Register |
| `max` | Highest numeric value wins | Max-Register |
| `min` | Lowest numeric value wins | Min-Register |
| `grow-set` | Elements can be added, never removed | G-Set |
| `or-set` | Add and remove, concurrent add wins | OR-Set |
| `review` | Concurrent writes flagged for human review | LWW + conflict flag |

### Milestone Commits

Git history that humans actually want to read:

```
milestone: post-canary traffic shift

Operations since last milestone:
  [autoscaler]  route.api-v1.weight: 50 → 90 (load-response)
  [human:zara]  route.api-v1.weight: 90 → 80 (manual override)
  [ci-pipeline] upstream.backend.targets: +10.0.1.5 (scale-out)

Environments affected: production, staging
Causal range: hlc:1706012400.0042 → hlc:1706015000.0187
Operations: 3
```

## Why Not Just Git?

Git solves versioning. It doesn't solve collaboration on structured config:

- **Merge conflicts on structured data** — Two people change different routes in the same YAML file, git panics
- **Push races** — Whoever pushes first wins, everyone else rebases
- **Branch drift** — Long-lived environment branches diverge structurally over months
- **No actor identity** — `git blame` shows who committed, not who or what requested the change
- **No intent tracking** — Commit messages are freeform text, not structured operation metadata
- **Text-level merge** — Git doesn't know that a route and an upstream are independent entities

Conflux keeps everything people like about git (diffable history, auditable, inspectable) and removes everything that breaks (text merge, push races, branch-based environments).

<details>
<summary><strong>Crates</strong></summary>

Each crate has its own `docs/` directory with detailed documentation.

| Crate | Description |
|-------|-------------|
| [`conflux-core`](crates/core/) | CRDT document model, typed operations, per-field merge semantics |
| [`conflux-schema`](crates/schema/) | Schema definition language (KDL) for config structure and merge rules |
| [`conflux-store`](crates/store/) | Persistent operation log and snapshot storage (SQLite) |
| [`conflux-git`](crates/git/) | Git milestone projection and config format serialization |
| [`conflux-api`](crates/api/) | HTTP and gRPC API server for machine actors |
| [`conflux`](crates/cli/) | CLI and daemon entry point |

</details>

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for guidelines.

**Using Claude Code?** See [`.claude/CLAUDE.md`](.claude/CLAUDE.md) for project context, architecture, and coding rules.

## Community

- 💬 [Discussions](https://github.com/raskell-io/conflux/discussions) — Questions, ideas, show & tell
- 🐛 [Issues](https://github.com/raskell-io/conflux/issues) — Bug reports and feature requests

## License

Apache 2.0 — See [LICENSE](LICENSE).
