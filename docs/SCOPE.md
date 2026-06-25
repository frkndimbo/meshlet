# Meshlet Scope

## Product Line

Meshlet is a Rust-core local context mesh for agent workflows. It gives agents and humans a durable local substrate for events, graph context, skills, and MCP access.

## v0.1 Active Scope

- CLI-first local runtime.
- SQLite storage under `.meshlet/`.
- Append-only event log with hash chaining.
- Context graph materialized from events.
- Skill registry from TOML manifests.
- MCP stdio server with small tools/resources.
- Tests for event, graph, skill, and MCP behavior.

## Explicitly Out of Scope for v0.1

- Cloud sync.
- Web dashboard.
- Marketplace or hosted registry.
- A2A adapter.
- Auth, teams, or remote identities.
- Vector database.
- AI summarizer, model calls, or prompt generation.
- Implicit shell/network execution from skills.

## Phase Gate

A feature moves into active scope only when `docs/PROGRESS.md` and the relevant policy doc are updated in the same change.
