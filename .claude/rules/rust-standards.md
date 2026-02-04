# Rust Coding Standards

> Minimum Rust version: **1.85.0** (Edition 2021)

These standards apply to all Rust code in Conflux. They enforce **deterministic behavior**, **bounded resources**, and **correctness over cleverness**.

**Related rules:**
- [project.md](project.md) — Conflux-specific architecture rules
- [patterns.md](patterns.md) — Code patterns for CRDTs, operations, storage
- [workflow.md](workflow.md) — Commands and processes

---

## Error Handling

### Use `?` Operator

```rust
// GOOD
fn process() -> Result<Data, Error> {
    let file = File::open(path)?;
    let data = parse(&file)?;
    Ok(data)
}
```

### Avoid `unwrap()` and `expect()` in Library Code

```rust
// GOOD: Return Result
pub fn parse_schema(path: &Path) -> Result<Schema, SchemaError> {
    let content = fs::read_to_string(path)?;
    Schema::from_str(&content)
}

// BAD: Panicking in library code
pub fn parse_schema(path: &Path) -> Schema {
    let content = fs::read_to_string(path).expect("failed to read");
    Schema::from_str(&content).unwrap()
}
```

### Use `thiserror` for Libraries, `anyhow` for Application Code

```rust
// Library crate (core, schema, store, git, api)
#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    #[error("schema mismatch: expected {expected}, got {actual}")]
    SchemaMismatch { expected: String, actual: String },
    #[error("invalid operation: {0}")]
    InvalidOperation(String),
}

// Application crate (cli)
fn main() -> anyhow::Result<()> {
    let config = load_config()?;
    run(config)?;
    Ok(())
}
```

---

## Memory and Performance

### Prefer References Over Cloning

```rust
// GOOD
fn validate(schema: &Schema, doc: &Document) -> Result<(), ValidationError> { ... }

// BAD: Unnecessary ownership
fn validate(schema: Schema, doc: Document) -> Result<(), ValidationError> { ... }
```

### Use `Cow` for Flexible Ownership

```rust
fn normalize_key(input: &str) -> Cow<'_, str> {
    if input.contains(' ') {
        Cow::Owned(input.replace(' ', "-"))
    } else {
        Cow::Borrowed(input)
    }
}
```

---

## Async Code

### Prefer `async fn`

```rust
// GOOD
async fn apply_operation(store: &Store, op: &Operation) -> Result<(), StoreError> {
    store.append(op).await?;
    Ok(())
}
```

### Use Structured Concurrency

```rust
// GOOD: Concurrent independent operations
let (snapshot, log) = tokio::join!(
    store.latest_snapshot(),
    store.operation_count()
);
```

---

## Type Design

### Use Newtypes for Domain Concepts

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntityId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActorId(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HlcTimestamp(u64);
```

### Prefer Enums Over Booleans

```rust
// GOOD
pub enum ConflictResolution {
    Auto,
    Review,
}

// BAD
fn merge(field: &Field, require_review: bool) { ... }
```

---

## Testing

### Property Tests for CRDTs

CRDT merge operations must be property-tested for:
- **Commutativity**: `merge(a, b) == merge(b, a)`
- **Associativity**: `merge(merge(a, b), c) == merge(a, merge(b, c))`
- **Idempotency**: `merge(a, a) == a`

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn merge_is_commutative(a in arb_document(), b in arb_document()) {
        let ab = merge(&a, &b);
        let ba = merge(&b, &a);
        assert_eq!(ab, ba);
    }
}
```

### Test Structure

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lww_prefers_later_timestamp() {
        // Arrange — explicit field initialization, no ..Default::default()
        // Act
        // Assert
    }
}
```

### No Flaky Tests

```rust
// GOOD: Controlled time
let clock = TestClock::new(1000);
let ts = clock.now();

// BAD: Real time
let ts = SystemTime::now();
```

---

## Documentation

### Document Public APIs

```rust
/// Merges two documents using the schema's per-field merge strategies.
///
/// # Errors
///
/// Returns `MergeError::SchemaMismatch` if the documents were created
/// against different schemas.
pub fn merge(a: &Document, b: &Document) -> Result<Document, MergeError> {
    // ...
}
```

---

## Linting and Formatting

### Run Before Committing

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
