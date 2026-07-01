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

- Permissions are declarations in v0.3, not enforcement guarantees.
- Allowed permission names are `read_repo`, `read_files`, `write_docs`, and `run_check`.
- Skill entries must be relative paths and must not contain `..`.
- No implicit shell or network access from a skill manifest.
- Future enforcement must be default-deny.
- Public-safe MCP mode must not execute skills or treat declarations as authorization.

## Registry Rules

- Registering a skill appends a `skill.added` event.
- The skills table is a materialized view of events.
- `graph rebuild` must reconstruct skills from event history.
