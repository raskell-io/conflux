# Git Milestones

## Overview

Milestones are periodic snapshots of the resolved config state, committed to a git repository. They provide the auditability and inspectability of git without requiring git to be the write path.

## Milestone Lifecycle

1. **Trigger** — Manual (`conflux milestone`), scheduled, or policy-based (e.g., after N operations, after a rollout completes)
2. **Resolve** — Materialize the current document state per environment
3. **Serialize** — Convert entities back to the original config format (YAML, JSON, TOML, KDL, XML, TF)
4. **Diff** — Compare against the last milestone to determine what changed
5. **Commit** — Write files to the git repo and commit with a structured message
6. **Record** — Store milestone metadata (commit SHA, causal range, timestamp)

## Commit Message Format

```
milestone: <user-provided message or auto-generated summary>

Operations since last milestone:
  [<actor>] <entity>.<field>: <old> -> <new> (<intent>)
  [<actor>] <entity>.<field>: <old> -> <new> (<intent>)
  ...

Environments affected: <list>
Causal range: hlc:<start> -> hlc:<end>
Operations: <count>
```

## Serialization Formats

The git projector serializes resolved state back to the original file format. Format is determined per-document:

| Format | Extension | Library |
|--------|-----------|---------|
| YAML | `.yaml`, `.yml` | `serde_yaml` |
| JSON | `.json` | `serde_json` |
| TOML | `.toml` | `toml` |
| KDL | `.kdl` | `kdl` |
| XML | `.xml` | `quick-xml` |
| TF (HCL) | `.tf` | `hcl-rs` |

## File Layout

The git repo mirrors the document structure:

```
<repo>/
├── production/
│   ├── routes.yaml
│   ├── upstreams.yaml
│   └── listeners.yaml
├── staging/
│   ├── routes.yaml        # Only overridden files
│   └── upstreams.yaml
└── .conflux/
    └── milestone.json     # Milestone metadata
```

## Conflict with Direct Git Edits

If someone edits the git repo directly (outside Conflux), the next milestone will overwrite their changes. This is by design — the Conflux engine is the source of truth, git is a projection. The milestone commit message will note the overwrite.

To import changes made directly in git, use `conflux import --git` which reads the repo and generates operations.
