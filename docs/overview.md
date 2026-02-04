# Conflux: Schema-Aware Config State Coordination

## What is Conflux?

Conflux is a daemon and CLI that coordinates infrastructure configuration across multiple writers. It replaces git as the **write path** for config changes, using per-field CRDT merge semantics defined in a schema to deterministically resolve concurrent edits. Resolved state is periodically projected to a git repository as milestone snapshots, preserving git as the **read path** for auditability and consumption.

## The Problem

Infrastructure configuration is increasingly written by multiple actors: humans editing service definitions, CI pipelines promoting builds, Kubernetes operators adjusting replicas, autoscalers tuning traffic weights, and policy engines enforcing constraints. These actors often modify the same logical configuration concurrently.

The standard approach is to funnel all changes through git: each actor opens a pull request, and merge conflicts are resolved manually or through automation. This creates several problems:

- **Text-level merge conflicts that aren't real conflicts.** Two actors changing different fields on the same YAML file produces a git merge conflict, even though the changes are semantically independent. A human or bot must intervene to resolve something that was never actually in contention.

- **Push races under high write volume.** When multiple pipelines try to push to the same branch, they serialize on git's lock. Retry loops, rebase logic, and exponential backoff become necessary infrastructure just to write a config value.

- **Environments modeled as branches.** Using long-lived branches for dev/staging/production leads to branch drift, painful cross-branch merges, and no clear promotion model. The relationship between environments is implicit in branch history rather than explicit in the data model.

- **No identity or intent on writes.** A git commit says who committed, but not which system made the change or why. When an autoscaler and a human both edit the same file in the same commit window, the audit trail is ambiguous.

- **Convergence is not guaranteed.** Different actors applying changes in different orders can produce different final states. Git merge strategies are heuristic and order-dependent. There is no formal guarantee that all replicas of the config will converge.

## How Conflux Solves It

### Schema-aware merge instead of text merge

Conflux requires a schema that declares every entity type, its fields, their types, and how each field should be merged when written concurrently. For example:

```toml
[entity.route]
fields = [
    { name = "weight",  type = "int",    merge = "max" },
    { name = "path",    type = "string", merge = "lww" },
    { name = "timeout", type = "int",    merge = "lww" },
]
```

Two actors setting `weight` and `timeout` on the same route is not a conflict — the schema knows these are independent fields. Two actors setting `weight` concurrently is resolved by the declared strategy (`max` in this case), not by line-level text diffing.

### Typed operations with identity

Every write is a typed operation that carries:

- **Actor ID** — which human, pipeline, or system made the change
- **Actor class** — human, CI pipeline, Kubernetes operator, or system process
- **HLC timestamp** — a hybrid logical clock value for causal ordering
- **Optional intent** — a short description of why the change was made

There are no anonymous mutations. The operation log provides a complete, attributable history of every change.

### CRDT-based deterministic convergence

Merge strategies are implemented as CRDTs (Conflict-free Replicated Data Types), which provide three mathematical guarantees:

- **Commutativity** — merging A then B produces the same result as merging B then A
- **Associativity** — the grouping of merges doesn't matter
- **Idempotency** — applying the same operation twice has no additional effect

These properties mean that regardless of the order in which operations arrive or which node processes them, all replicas converge to the same state.

### Post-merge validation

CRDTs guarantee convergence but not correctness. After every merge, Conflux validates the resolved state against the schema: cross-entity references resolve, numeric values fall within declared ranges, and required fields are present. This catches semantic errors that a purely mechanical merge would miss.

### Environments as a data dimension

Environments are not modeled as git branches. They are an explicit dimension in the data model, forming an inheritance chain:

```
production (base) <- staging (overlay) <- development (overlay)
```

Each environment can override specific fields. Promotion between environments is an explicit operation — not a branch merge — with full identity and intent tracking.

### Git as a read-only projection

Conflux periodically snapshots resolved state into a git repository as milestones. Git remains the consumption point: other tools read config from git, CI pipelines trigger on git commits, and teams can diff milestones using standard git tooling. But git never receives direct writes. If someone edits git directly, the next milestone overwrites their changes. The operation log inside Conflux is the source of truth.

## Architecture at a Glance

```
                    +----------------------------------+
  Actors            |        Conflux Engine            |        Consumers
                    |                                  |
  Human (CLI/UI) -->|  +----------------------------+  |--> Git repo (milestones)
  CI pipeline ----->|  |  CRDT Document Store       |  |--> Webhook notifications
  K8s operator ---->|  |                            |  |--> Pull API (reconcilers)
  Autoscaler ------>|  |  Per-field merge logic     |  |--> gRPC stream (watchers)
  Policy engine --->|  |  Causal history (HLC)      |  |
                    |  |  Schema + validation       |  |
                    |  +----------------------------+  |
                    |                                  |
                    |  +-----------+ +--------------+  |
                    |  | Env/Stage | |  Milestone   |  |
                    |  | Overlays  | |  Projector   |  |
                    |  +-----------+ +--------------+  |
                    +----------------------------------+
```

The engine owns all writes. Actors submit typed operations via CLI, HTTP, or gRPC. The engine validates each operation against the schema, stamps it with an HLC timestamp, appends it to the operation log, merges it into document state, runs post-merge validation, and notifies watchers. Periodically, the milestone projector serializes resolved state to the git repository.

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| Git is read-only | Eliminates merge conflicts, push races, and branch drift at the source |
| Schema declares merge rules | Merge behavior is data-driven, not hardcoded; adding a new field or strategy requires no code changes |
| Append-only operation log | Provides full audit trail, enables rebuilding state from any point, supports causal range queries |
| HLC for causal ordering | Captures causality without requiring synchronized clocks across actors |
| Per-field (not per-entity) merge | Maximizes automatic resolution; most concurrent edits touch different fields |
| Post-merge validation | Separates convergence (guaranteed by CRDTs) from correctness (enforced by schema constraints) |
