# API

## Overview

Conflux exposes two API surfaces:

- **HTTP REST** (port 9400) — For human tooling, dashboards, and simple integrations
- **gRPC** (port 9401) — For machine actors, operators, and high-throughput integrations

Both APIs require actor identity on every request.

## Authentication

Every request must identify the actor:

```
X-Conflux-Actor: autoscaler
X-Conflux-Actor-Class: operator
```

Or via gRPC metadata:

```
conflux-actor: ci-pipeline
conflux-actor-class: pipeline
```

## HTTP Endpoints

### Operations

```
POST   /v1/ops                    # Submit an operation
POST   /v1/ops/batch              # Submit multiple operations atomically
GET    /v1/state/{entity_path}    # Get resolved state for an entity
GET    /v1/state                  # Get full resolved document state
```

### Environments

```
GET    /v1/envs                           # List environments
GET    /v1/envs/{env}/state               # Get resolved state for environment
POST   /v1/envs/promote                   # Promote one env to another
GET    /v1/envs/diff?from={env}&to={env}  # Diff between environments
```

### Milestones

```
POST   /v1/milestones             # Create a milestone (commit to git)
GET    /v1/milestones             # List milestones
GET    /v1/milestones/{id}        # Get milestone details
```

### History

```
GET    /v1/log                    # Operation log (paginated)
GET    /v1/log/{entity_path}      # Operations for a specific entity
GET    /v1/blame/{entity_path}    # Who last modified each field
```

## gRPC Service

```protobuf
service Conflux {
    rpc Apply(ApplyRequest) returns (ApplyResponse);
    rpc BatchApply(BatchApplyRequest) returns (BatchApplyResponse);
    rpc GetState(GetStateRequest) returns (StateResponse);
    rpc WatchState(WatchRequest) returns (stream StateEvent);
    rpc CreateMilestone(MilestoneRequest) returns (MilestoneResponse);
}
```

The `WatchState` RPC provides a server-streaming endpoint for reactive consumers that need to act on config changes in real time.
