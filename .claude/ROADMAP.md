# Conflux Roadmap

## Vision

Replace git as the write path for infrastructure config. Provide schema-aware, multi-actor config coordination with deterministic merge and git-compatible auditability.

---

## v0.1 — Core Engine (Foundation) ✅

The minimum viable artifact: a single-node daemon that accepts typed operations, merges them per schema, and projects milestones to git.

### Milestone: Schema + Core CRDT Model ✅
- [x] Define the `Document`, `Entity`, `Field` types in `conflux-core`
- [x] Implement `MergeStrategy` variants: LWW, Max, Min, GrowSet, OrSet, Review
- [x] Implement HLC (Hybrid Logical Clock) for causal ordering
- [x] Implement `Operation` types: SetField, InsertEntity, RemoveEntity, MoveEntity
- [x] Implement document merge algorithm (two documents → one)
- [x] Property tests: merge commutativity, associativity, idempotency
- [x] TOML schema parser in `conflux-schema`
- [x] Schema validation (field types, constraints, cross-entity refs)

### Milestone: Storage Layer ✅
- [x] SQLite-backed operation log in `conflux-store`
- [x] Append operations, query by entity/field/actor/time range
- [x] Periodic snapshot materialization
- [x] Milestone metadata storage

### Milestone: Git Projection ✅
- [x] Serialize resolved state to YAML, JSON, TOML, KDL, XML, TF in `conflux-git`
- [x] Structured milestone commit messages with causal attribution
- [x] File layout: per-environment directories
- [x] Import from existing config files (`conflux import`)

### Milestone: CLI + API ✅
- [x] CLI commands: `init`, `import`, `set`, `get`, `diff`, `log`, `blame`, `status`, `milestone`
- [x] HTTP REST API with actor identity headers
- [x] Daemon mode (`conflux daemon`)

### Milestone: Release (In Progress)
- [ ] CI/CD workflows (GitHub Actions for test, lint, release)
- [x] Integration test suite (multi-actor scenarios)
- [x] Example project with schema and sample configs
- [ ] Single static binary builds (Linux, macOS, Docker)
- [ ] Documentation site (getting started, schema reference, CLI reference)

---

## v0.2 — Environments + Promotion

Per-environment field overrides and promotion workflow.

### Goals
- [x] `SetOverride` operation type in `conflux-core`
- [x] Environment-aware field resolution (inheritance chain lookup)
- [ ] Environment API endpoints (`/v1/envs/{env}/state`, `/v1/envs/diff`)
- [ ] `conflux promote` command (staging → production as an operation)
- [ ] Environment-aware `conflux diff`
- [ ] Per-environment milestone projection (separate directories)

### Current Status
- [x] Schema parsing for environment definitions
- [x] Environment inheritance chain validation
- [x] Core field resolution with environment context
- [ ] Promote command is placeholder only

---

## v0.3 — Conflict Review Workflow

Human-in-the-loop conflict resolution for `review` merge strategy fields.

### Goals
- [x] Conflict flagging for `review` merge strategy fields (core implemented)
- [ ] `conflux conflicts` command to list unresolved conflicts
- [ ] `conflux resolve` command for human conflict resolution
- [ ] Conflict metadata in milestone commits
- [ ] Conflict notification hooks

---

## v0.4 — Reactive Consumers

Push-based state updates for machine actors and reconcilers.

### Goals
- [ ] gRPC service definition (`.proto` files)
- [ ] gRPC `Apply`, `BatchApply`, `GetState` endpoints
- [ ] gRPC `WatchState` streaming endpoint
- [ ] Webhook registration and subscription API
- [ ] Webhook notifications on state change
- [ ] Event format for GitOps reconcilers (Flux, ArgoCD compatible)
- [ ] `conflux watch` CLI command

### Current Status
- [x] HTTP REST API functional (11 endpoints)
- [x] tonic/prost dependencies present
- [ ] gRPC implementation (NOT STARTED)
- [ ] Webhook infrastructure (NOT STARTED)

---

## v0.5 — Multi-Node Replication

Distributed Conflux for high availability and geo-distribution.

### Goals
- [ ] Operation replication between Conflux instances
- [ ] Causal broadcast protocol
- [ ] Conflict-free convergence across nodes
- [ ] Leader election for milestone projection
- [ ] Network partition tolerance

---

## Future Considerations

### Format Support
- Jsonnet / CUE support
- Custom serializer plugin API

### Integrations
- Kubernetes operator (CRD-based config management)
- Terraform provider
- ArgoCD / Flux plugin (Conflux as a source)
- GitHub App (PR-based milestone review)

### Security
- Actor authentication (API keys, mTLS)
- Role-based access control per entity/environment
- Signed operations (Ed25519)
- Audit log export

### Performance
- Incremental merge (avoid full document re-merge)
- Operation batching and compression
- Lazy snapshot materialization
- Benchmark suite
- Storage abstraction (pluggable backends beyond SQLite)
