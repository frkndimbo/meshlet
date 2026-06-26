# Meshlet

Meshlet is a local-first context mesh for agent workflows. It stores durable events, materializes a context graph, registers skills, and exposes local state through MCP stdio.

Meshlet is not a model, chatbot, dashboard, cloud service, or marketplace. v0.1 stays small: local runtime, CLI, SQLite, and MCP stdio.

## Core Flow

```text
Agent -> MCP -> Meshlet -> Event Log -> Context Graph -> Skill Registry
```

## Current Features

- Append-only local event log with hash chaining.
- Event hash-chain verification.
- SQLite-backed local project state in `.meshlet/`.
- Rebuildable graph nodes and edges derived from events.
- Deterministic event/graph query.
- Skill manifest registration from TOML.
- Task and evidence views for agent work journals.
- MCP stdio server exposing Meshlet tools and resources.

## Commands

```bash
rtk cargo run -- init
rtk cargo run -- status
rtk cargo run -- verify
rtk cargo run -- event append --type context.added --json '{"label":"repo context"}'
rtk cargo run -- event list
rtk cargo run -- query "repo context" --kind all --limit 20
rtk cargo run -- graph nodes --limit 20
rtk cargo run -- skill add ./skill.toml
rtk cargo run -- task list
rtk cargo run -- evidence list
rtk cargo run -- serve --mcp stdio
```

## Verification

```bash
rtk cargo fmt --check
rtk cargo check
rtk cargo test
```

## v0.1 Scope

- Event log as source of truth.
- Context graph as materialized view.
- Skill registry with declared permissions.
- MCP stdio tools/resources.
- CLI for local operation.

## Not Doing Yet

- Cloud sync.
- Web dashboard.
- Marketplace.
- A2A adapter.
- Auth/team mode.
- Vector database.
- AI summarizer or model integration.

## Docs

- `docs/SCOPE.md` - phase boundaries.
- `docs/PROGRESS.md` - current state and next tasks.
- `docs/MCP_POLICY.md` - MCP behavior rules.
- `docs/SKILL_POLICY.md` - skill manifest and permission rules.
- `docs/SECURITY_POLICY.md` - local safety rules.
- `docs/GRAPH_POLICY.md` - event/graph/Graphify rules.
- `docs/PRODUCT_HANDOFF.md` - product direction and phased roadmap.
