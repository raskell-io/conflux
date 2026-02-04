# Conflux Project Rules

These rules ensure contributions align with Conflux's architecture and philosophy.

---

## Core Principles

### 1. Deterministic Merge

Every merge must produce the same result regardless of operation order or which node performed it:

```rust
// This must ALWAYS hold:
assert_eq!(merge(&a, &b), merge(&b, &a));  // Commutative
assert_eq!(merge(&merge(&a, &b), &c), merge(&a, &merge(&b, &c)));  // Associative
assert_eq!(merge(&a, &a), a);  // Idempotent
```

**If you add a new merge strategy, property-test these three invariants.**

### 2. Identity on Every Write

No anonymous mutations. Every operation must carry:
- `ActorId` — who or what made the change
- `ActorClass` — human, pipeline, operator, or system
- `HlcTimestamp` — causal ordering
- Optional `intent` — why the change was made

```rust
// GOOD: Full attribution
Operation::SetField {
    entity_id: route_id,
    field: "weight",
    value: FieldValue::Int(80),
    actor: ActorId::new("autoscaler", ActorClass::Operator),
    intent: Some("load-response".into()),
    timestamp: clock.now(),
}

// BAD: Missing identity
Operation::SetField {
    entity_id: route_id,
    field: "weight",
    value: FieldValue::Int(80),
    // No actor, no intent, no timestamp
}
```

### 3. Git is a Projection

The Conflux engine is the source of truth. Git receives milestone snapshots. This means:

| Allowed | Not Allowed |
|---------|-------------|
| Read resolved state from git | Write config by editing git directly |
| Diff milestones in git | Use git merge to reconcile environments |
| Blame via git or via `conflux blame` | Treat git history as the operation log |

**If someone edits git directly, the next milestone overwrites their changes.** This is intentional. Use `conflux import --git` to pull external changes into the operation log.

### 4. Schema Owns the Merge Rules

Merge behavior is never hardcoded. It's always declared in the schema:

```kdl
entity "route" {
    field "weight" type="int" merge="max"       // Schema says max-wins
    field "path"   type="string" merge="lww"    // Schema says LWW
}
```

**Don't add merge logic that isn't driven by a schema declaration.**

### 5. Validate After Merge

CRDTs guarantee convergence. They don't guarantee correctness. Every merge must be followed by schema validation:

```rust
let merged = merge(&doc_a, &doc_b)?;
schema.validate(&merged)?;  // Catches broken refs, invalid ranges, etc.
```

---

## Architecture Rules

### Crate Boundaries

| Crate | Owns | Does NOT Own |
|-------|------|--------------|
| `core` | Document model, operations, merge logic, HLC | Schema parsing, storage, serialization |
| `schema` | Schema definition, field types, validation | Merge execution, storage |
| `store` | Operation log, snapshots, persistence | Merge logic, git interaction |
| `git` | Milestone projection, serialization formats | Merge logic, API serving |
| `api` | HTTP/gRPC server, request handling | Core merge logic, storage details |
| `cli` | CLI commands, daemon entry point | Library logic |

**Cross-crate rules:**
- `core` has no internal dependencies
- `schema` depends only on `core`
- `store` depends on `core` and `schema`
- `git` depends on `core`, `schema`, and `store`
- `api` and `cli` may depend on all crates

### Operation Log is Append-Only

Operations are never modified or deleted. This provides:
- Full audit trail
- Ability to rebuild state from any point
- Causal range queries for milestones

### Environment Overlays, Not Branches

Environments are a dimension in the data model, not git branches:

```
production (base) ← staging (overlay) ← development (overlay)
```

**Never use git branches to model environments.**

---

## Security Rules

### No Secrets in Operations

Operations and their payloads are stored in plaintext in the operation log and projected to git. Never store secrets as field values.

```rust
// BAD: Secret in config state
Operation::SetField { field: "api-key", value: "sk-abc123..." }

// GOOD: Reference to external secret
Operation::SetField { field: "api-key-ref", value: "vault:secret/api-key" }
```

### Validate at System Boundaries

All input from HTTP, gRPC, and CLI is validated before entering the operation log:
- Actor identity present and valid
- Operation matches schema
- Field values satisfy constraints
- Entity references resolve

---

## Testing Rules

### CRDT Properties are Non-Negotiable

Every merge strategy must have property tests for commutativity, associativity, and idempotency. No exceptions.

### Test Naming

```rust
#[test]
fn lww_prefers_later_hlc_timestamp() { }

#[test]
fn max_merge_selects_highest_value_across_actors() { }

#[test]
fn concurrent_set_and_remove_preserves_set() { }

#[test]
fn milestone_includes_all_operations_in_causal_range() { }
```

### Deterministic Tests

All tests must use controlled clocks and deterministic actor IDs. No real-time dependencies.
