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

### Example: The false conflict

An autoscaler and a human both need to update the same service config file at the same time. The autoscaler is adjusting traffic weight based on load. The human is updating the request timeout after a performance review.

The YAML file looks like this:

```yaml
# services/api-gateway.yaml
route:
  path: /api/v2
  weight: 60
  timeout: 3000
```

The autoscaler opens a PR changing `weight` to `80`. The human opens a PR changing `timeout` to `5000`. Git sees two branches modifying the same file and reports a merge conflict. A human has to manually resolve it — or a merge bot has to be built and maintained — even though these changes are to completely independent fields that have no semantic relationship.

In a busy system this happens constantly. Every pair of concurrent writers touching the same file produces a conflict that requires intervention.

### Example: The push race

A deployment pipeline promotes a new image tag to staging. At the same time, a policy engine updates a resource limit on the same environment. Both systems:

1. Clone the repo
2. Make their change
3. Commit
4. Push

The second push fails because the remote has moved. So it pulls, rebases, and pushes again — but by then a third writer may have pushed. Teams end up building retry loops with jitter and backoff just to write a config value. Under high write volume (dozens of automated actors), this becomes a significant source of latency and operational complexity. Some changes take multiple minutes to land because of repeated rebase cycles.

### Example: The environment drift

A team uses three long-lived branches: `main` (production), `staging`, and `dev`. A fix is applied to `main` for an urgent production issue. A week later, someone promotes a feature from `dev` to `staging` by merging the branch. The merge doesn't include the production fix because it was applied directly to `main` and never cherry-picked back. Staging now has a different base configuration than production — but nobody notices until the next production deploy introduces a regression.

The fundamental issue is that branch-based environment modeling provides no mechanism to ensure that a base environment's state is consistently inherited by its descendants.

### Example: The mystery write

An on-call engineer notices that a service's replica count was changed at 3 AM. The git log shows a commit from a CI service account with the message "automated config update." There's no indication of which system triggered the change, what condition caused it, or whether it was an autoscaler responding to load, a rollback triggered by a failed health check, or a policy engine enforcing a minimum replica count.

The engineer has to search through logs from multiple systems to reconstruct what happened. In the meantime, they don't know whether to revert the change or leave it.

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
