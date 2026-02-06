# Changelog

All notable changes to Conflux will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-02-06

Initial release of Conflux — schema-aware config state coordination.

### Added

#### Core Engine (`conflux-core`)
- CRDT document model with typed operations and per-field merge semantics
- Merge strategies: LWW, Max, Min, GrowOnlySet, ObservedRemoveSet, ReviewRegister
- Hybrid logical clock (HLC) for causally consistent, totally ordered operations
- Entity model with hierarchical structure (parent/child relationships)
- Field types: String, Int, Float, Bool, Duration, Ref, List, Map
- Ed25519 digital signatures on operations for cryptographic auditability
- Public key registry for signature verification

#### Schema (`conflux-schema`)
- TOML-based schema definition language
- Entity and field definitions with merge strategy declarations
- Environment overlays with inheritance chains
- Field validation: required, range, allowed_values, pattern
- Conflict modes: auto-merge or require-review

#### Storage (`conflux-store`)
- Pluggable storage backend abstraction (`Store` and `AsyncStore` traits)
- SQLite backend for production use
- In-memory backend for testing
- PostgreSQL backend (feature-gated: `postgres`)
- DynamoDB backend with single-table design (feature-gated: `dynamodb`)
- Operation log with causal range queries
- Snapshot management for fast state reconstruction
- Milestone tracking with HLC ranges
- Version vector support for replication
- Audit log export as JSON Lines (NDJSON)
- Signature verification during audit export

#### Git Integration (`conflux-git`)
- Milestone projection to git repositories
- Structured commit messages with operation attribution
- Multi-format serialization: YAML, JSON, TOML, KDL, XML, HCL
- Environment-specific file generation
- Causal range tracking for milestone boundaries

#### API Server (`conflux-api`)
- HTTP REST API on port 9400
- gRPC API for streaming and high-performance clients
- API key authentication with SHA-256 hashed keys
- mTLS client certificate authentication
- Role-based access control (RBAC) enforcement
- Legacy header authentication for backward compatibility
- Endpoints:
  - `POST /v1/ops` — Submit operation
  - `POST /v1/ops/batch` — Batch operations
  - `GET /v1/state` — Get document state
  - `GET /v1/state/{entity}` — Get entity state
  - `POST /v1/milestones` — Create milestone
  - `GET /v1/milestones` — List milestones
  - `GET /v1/log` — Operation log
  - `GET /v1/envs` — List environments
  - `GET /v1/envs/{name}/state` — Environment-specific state
  - `GET /v1/envs/diff` — Diff between environments
  - `POST /v1/webhooks` — Register webhook
  - `GET /v1/audit/export` — Export audit log
- Webhook system with configurable filters and formats
- GitOps-compatible webhook payload format

#### RBAC (`conflux-rbac`)
- Role definitions with permission sets
- Role inheritance for hierarchical access control
- Resource pattern matching (entity type, ID, field, environment)
- Actions: Read, Write, Create, Delete, Move, Override, Resolve, Promote, Milestone, Admin
- Actor assignments by ID or pattern with actor class filtering
- Default roles for authenticated and anonymous actors

#### CLI (`conflux`)
- Project initialization: `conflux init`
- Config import: `conflux import` (YAML, JSON, TOML, KDL, Jsonnet, CUE)
- State management: `conflux set`, `conflux get`, `conflux status`
- Bulk operations: `conflux bulk-set`
- History: `conflux log`, `conflux blame`, `conflux diff`
- Milestones: `conflux milestone create/list/show`
- Environments: `conflux promote`
- Conflicts: `conflux conflicts`, `conflux resolve`
- Real-time: `conflux watch`
- Server: `conflux daemon`
- Authentication: `conflux auth hash-key`
- Key management: `conflux keys generate/register/list/revoke`
- RBAC: `conflux rbac list-roles/check/show-roles/validate`
- Audit: `conflux audit export/verify`

#### Replication (`conflux-replication`)
- Replication protocol foundations
- Peer discovery and connection management
- Operation synchronization primitives

### Security

- API key authentication with secure hash comparison
- mTLS support with CN/SAN-based actor mapping
- Ed25519 operation signatures (optional, for high-security deployments)
- RBAC with deny-by-default policy
- Audit trail with cryptographic verification
- No secrets in operation payloads (by design)

### Documentation

- Comprehensive README with quick start guide
- Architecture documentation in `.claude/CLAUDE.md`
- Per-crate documentation in `docs/` directories
- Rust coding standards and project rules

[1.0.0]: https://github.com/raskell-io/conflux/releases/tag/v1.0.0
