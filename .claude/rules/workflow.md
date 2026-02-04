# Conflux Workflow

Commands, processes, and common tasks for working with Conflux.

---

## Development Environment

### Prerequisites

- Rust 1.85.0+ (see `rust-toolchain.toml`)
- mise (task runner)
- Git (for milestone projection tests)

### Setup

```bash
cargo build --workspace
cargo test --workspace
```

---

## Common Commands

### Building

```bash
# Debug build
cargo build --workspace

# Release build
cargo build --workspace --release

# Build specific crate
cargo build -p conflux-core
cargo build -p conflux-schema
cargo build -p conflux-store
cargo build -p conflux-git
cargo build -p conflux-api
cargo build -p conflux  # CLI binary
```

### Testing

```bash
# Run all tests
cargo test --workspace

# Run tests for specific crate
cargo test -p conflux-core
cargo test -p conflux-schema

# Run specific test
cargo test -p conflux-core lww_merge

# Run property tests (may be slow)
cargo test --workspace -- --include-ignored proptest

# Run tests with output
cargo test --workspace -- --nocapture
```

### Linting

```bash
# Format code
cargo fmt --all

# Check formatting (CI)
cargo fmt --all --check

# Run clippy
cargo clippy --workspace --all-targets -- -D warnings

# Run clippy with fixes
cargo clippy --workspace --all-targets --fix --allow-dirty
```

### Documentation

```bash
# Generate docs
cargo doc --workspace --no-deps

# Open docs in browser
cargo doc --workspace --no-deps --open
```

---

## Running Conflux

### Local Development

```bash
# Run daemon
cargo run --bin conflux -- daemon

# Run with debug logging
RUST_LOG=debug cargo run --bin conflux -- daemon

# Initialize a project
cargo run --bin conflux -- init --schema schema.kdl

# Import existing configs
cargo run --bin conflux -- import ./configs/ --env production
```

---

## Git Workflow

### Branch Naming

```
feature/add-environment-overlays
fix/hlc-timestamp-tiebreak
docs/update-schema-reference
refactor/simplify-merge-algorithm
```

### Commit Messages

Follow conventional commits:

```
feat(core): add OR-Set merge strategy

Implement observed-remove set CRDT for list fields where
concurrent add/remove should resolve to add.

Property tests for commutativity, associativity, idempotency.
```

```
fix(git): handle empty milestone with no operations

Skip git commit when milestone has zero operations since
the last milestone. Previously created an empty commit.
```

### Pre-commit Checklist

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
```

---

## Release Process

### Version Bump

1. Update version in `Cargo.toml` (workspace)
2. Update `CHANGELOG.md`
3. Commit: `chore: bump version to X.Y.Z`
4. Tag: `git tag vX.Y.Z`
5. Push: `git push && git push --tags`

### Crates.io Publishing

```bash
# Publish in dependency order
cargo publish -p conflux-core
cargo publish -p conflux-schema
cargo publish -p conflux-store
cargo publish -p conflux-git
cargo publish -p conflux-api
cargo publish -p conflux
```

---

## Debugging

### Logging

```bash
# Maximum verbosity
RUST_LOG=trace cargo run --bin conflux -- daemon

# Specific modules
RUST_LOG=conflux_core::merge=debug,conflux_store=trace cargo run --bin conflux -- daemon
```

### Inspecting State

```bash
# Dump the operation log
cargo run --bin conflux -- log

# Dump resolved state
cargo run --bin conflux -- get --all

# Check milestone history
cargo run --bin conflux -- milestone --list
```

---

## CI/CD

### GitHub Actions

| Workflow | Trigger | Purpose |
|----------|---------|---------|
| `ci.yml` | Push, PR | Build, test, lint, property tests |
| `release.yml` | Tag push | Build binaries, publish |

### Local CI Simulation

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
```
