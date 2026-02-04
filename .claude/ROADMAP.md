# Conflux Roadmap

## Vision

Replace git as the write path for infrastructure config. Provide schema-aware, multi-actor config coordination with deterministic merge and git-compatible auditability.

---

## v0.1 — Core Engine (Foundation)

The minimum viable artifact: a single-node daemon that accepts typed operations, merges them per schema, and projects milestones to git.

### Milestone: Schema + Core CRDT Model
- [ ] Define the `Document`, `Entity`, `Field` types in `conflux-core`
- [ ] Implement `MergeStrategy` variants: LWW, Max, Min, GrowSet, OrSet, Review
- [ ] Implement HLC (Hybrid Logical Clock) for causal ordering
- [ ] Implement `Operation` types: SetField, InsertEntity, RemoveEntity, MoveEntity
- [ ] Implement document merge algorithm (two documents → one)
- [ ] Property tests: merge commutativity, associativity, idempotency
- [ ] TOML schema parser in `conflux-schema`
- [ ] Schema validation (field types, constraints, cross-entity refs)

### Milestone: Storage Layer
- [ ] SQLite-backed operation log in `conflux-store`
- [ ] Append operations, query by entity/field/actor/time range
- [ ] Periodic snapshot materialization
- [ ] Milestone metadata storage

### Milestone: Git Projection
- [ ] Serialize resolved state to YAML, JSON, TOML, KDL, XML, TF in `conflux-git`
- [ ] Structured milestone commit messages with causal attribution
- [ ] File layout: per-environment directories
- [ ] Import from existing config files (`conflux import`)

### Milestone: CLI + API
- [ ] CLI commands: `init`, `import`, `set`, `get`, `diff`, `log`, `blame`, `status`, `milestone`
- [ ] HTTP REST API with actor identity headers
- [ ] Daemon mode (`conflux daemon`)

### Milestone: Release
- [ ] Single static binary (Linux, macOS, Docker)
- [ ] Documentation site (getting started, schema reference, CLI reference)
- [ ] Integration test suite (multi-actor scenarios)

---

## v0.2 — Environments + Promotion

### Goals
- [ ] Environment overlay resolution (inheritance chain)
- [ ] `conflux promote` command (staging → production as an operation, not a merge)
- [ ] Environment-aware `conflux diff`
- [ ] SetOverride operation type
- [ ] Per-environment milestone projection (separate directories or branches)

---

## v0.3 — Conflict Review Workflow

### Goals
- [ ] Conflict flagging for `review` merge strategy fields
- [ ] `conflux conflicts` command to list unresolved conflicts
- [ ] `conflux resolve` command for human conflict resolution
- [ ] Webhook/notification on conflict creation
- [ ] Conflict metadata in milestone commits

---

## v0.4 — Reactive Consumers

### Goals
- [ ] gRPC `WatchState` streaming endpoint
- [ ] Webhook notifications on state change
- [ ] Event format for GitOps reconcilers (Flux, ArgoCD compatible)
- [ ] `conflux watch` CLI command

---

## v0.5 — Multi-Node Replication

### Goals
- [ ] Operation replication between Conflux instances
- [ ] Causal broadcast protocol
- [ ] Conflict-free convergence across nodes (the actual CRDT replication promise)
- [ ] Leader election for milestone projection (only one node commits to git)
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
