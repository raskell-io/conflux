# CLI Usage

## Overview

The `conflux` CLI is the primary human interface. It communicates with the Conflux daemon or operates directly on a local state store.

## Commands

### `conflux init`

Initialize a new Conflux project.

```bash
conflux init --schema ./schema.kdl --git-remote git@github.com:org/infra-config.git
```

### `conflux import`

Import existing config files into the document store.

```bash
conflux import ./configs/ --env production
conflux import ./staging-overrides/ --env staging
conflux import --git  # Import from git repo state
```

### `conflux set`

Set a field value on an entity.

```bash
conflux set route.api-v1.weight 80 --reason "shifting traffic for canary"
conflux set upstream.backend.health-check true
```

### `conflux get`

Read resolved state.

```bash
conflux get route.api-v1              # Full entity
conflux get route.api-v1.weight       # Single field
conflux get --env staging route.api-v1  # Environment-specific
```

### `conflux diff`

Compare state between environments or milestones.

```bash
conflux diff --env staging --env production
conflux diff --milestone m1 --milestone m2
```

### `conflux log`

View the operation history.

```bash
conflux log                          # All operations
conflux log route.api-v1.weight      # Field history
conflux log --actor autoscaler       # By actor
```

### `conflux blame`

Show who last modified each field of an entity.

```bash
conflux blame route.api-v1
```

### `conflux status`

Show pending changes since the last milestone.

```bash
conflux status
```

### `conflux milestone`

Snapshot current state to git.

```bash
conflux milestone --message "post-canary traffic shift"
conflux milestone --auto  # Auto-generate message from operations
```

### `conflux promote`

Promote config from one environment to another.

```bash
conflux promote --from staging --to production
conflux promote --from staging --to production --entity route.api-v1  # Single entity
```

### `conflux daemon`

Start the Conflux daemon (API server).

```bash
conflux daemon --config ./conflux.toml
conflux daemon --port 9400 --grpc-port 9401
```
