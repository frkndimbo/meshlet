# Skill Policy

## Manifest

Every skill must be registered through a TOML manifest.

Required fields:

```toml
name = "rust-review"
version = "0.1.0"
kind = "skill"
entry = "./SKILL.md"
permissions = ["read_repo", "run_check"]
```

Optional fields:

```toml
description = "Review Rust code."
```

## Permissions

- Permissions are declarations in v0.1, not enforcement guarantees.
- No implicit shell or network access from a skill manifest.
- Future enforcement must be default-deny.

## Registry Rules

- Registering a skill appends a `skill.added` event.
- The skills table is a materialized view of events.
- `graph rebuild` must reconstruct skills from event history.
