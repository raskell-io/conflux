# Infrastructure Example

A complete example showing how to use Conflux for managing microservices infrastructure configuration.

## Overview

This example includes:
- **Schema**: Entity definitions for services, routes, and policies
- **Sample Configs**: YAML files representing a typical microservices setup
- **Demo Script**: Walkthrough of common operations

## Quick Start

```bash
# From the repository root
cd examples/infrastructure

# Initialize the project
conflux init --schema schema.toml

# Import the sample configurations
conflux import configs/services.yaml --entity-type service
conflux import configs/routes.yaml --entity-type route
conflux import configs/policies.yaml --entity-type policy

# View the current state
conflux status
conflux get --all
```

## Schema

The schema (`schema.toml`) defines three entity types:

### Services
Application deployments with fields for:
- Container image and resource limits
- Replica count (merge strategy: `max` — concurrent scale-ups don't conflict)
- Environment variables and ports
- Health check configuration

### Routes
Traffic routing rules with:
- Path matching and target service
- Traffic weight (merge strategy: `max` — safe for canary deployments)
- Timeout (merge strategy: `min` — most conservative wins)
- Rate limiting and retry configuration

### Policies
Access control and rate limiting:
- Policy type and target
- Rules list (merge strategy: `set` — rules are accumulated)
- Priority (merge strategy: `max`)

## Common Operations

### Scaling a Service

```bash
# Scale up the API service (max-wins, so concurrent scale-ups are safe)
conflux set service.api replicas 5 --intent "Scale for traffic spike"

# Check the result
conflux get service.api replicas
```

### Canary Deployment

```bash
# Deploy new version to a subset of traffic
conflux set service.api image "myapp/api:v2.2.0-canary" --intent "Canary v2.2.0"
conflux set route.api-public weight 10 --intent "10% canary traffic"

# Monitor and increase traffic
conflux set route.api-public weight 50 --intent "50% canary traffic"

# Full rollout
conflux set route.api-public weight 100 --intent "Full rollout v2.2.0"
```

### Adjusting Timeouts

```bash
# Reduce timeout (min-wins, so this is safe even with concurrent changes)
conflux set route.api-public timeout "15s" --intent "Reduce timeout for faster failover"
```

### Adding Rate Limiting

```bash
# Update rate limit
conflux set route.api-public rate_limit 500 --intent "Reduce rate limit during incident"
```

### Viewing History

```bash
# See recent operations
conflux log --limit 20

# See who changed a specific entity
conflux blame service.api

# See changes since last milestone
conflux diff
```

### Creating a Milestone

```bash
# Commit current state to git
conflux milestone create -m "Deploy API v2.2.0"

# List milestones
conflux milestone list
```

## Multi-Actor Scenario

Conflux shines when multiple actors modify config concurrently:

```bash
# Terminal 1: Human operator scales up
conflux set service.api replicas 5 --actor ops-alice --intent "Manual scale for launch"

# Terminal 2: Autoscaler also scales up (at the same time)
conflux set service.api replicas 8 --actor autoscaler --actor-class operator --intent "Load-based scaling"

# Result: replicas = 8 (max-wins, no conflict)
conflux get service.api replicas
# => 8

# Both operations are recorded
conflux blame service.api
```

## Environment Overlays

The schema defines three environments: `production` (base), `staging`, and `development`.

```bash
# View environment configuration
cat schema.toml | grep -A5 environments
```

Environment promotion is supported but requires SetOverride operations in core (planned feature).

## Running the API Server

```bash
# Start the daemon
conflux daemon --port 8080

# In another terminal, use the API
curl http://localhost:8080/entities
curl http://localhost:8080/entities/service.api
```

## Files

```
infrastructure/
├── schema.toml           # Schema definition
├── configs/
│   ├── services.yaml     # Service configurations
│   ├── routes.yaml       # Route configurations
│   └── policies.yaml     # Policy configurations
└── README.md             # This file
```
