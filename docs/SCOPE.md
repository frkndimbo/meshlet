# Meshlet Scope

## Product Line

Meshlet is a Rust-core local context mesh for agent workflows. It gives agents and humans a durable local substrate for events, graph context, skills, and MCP access.

## v0.2 Active Scope

- CLI-first local runtime.
- SQLite storage under `.meshlet/`.
- Append-only event log with hash chaining.
- Context graph materialized from events.
- Namespaced graph import from Graphify `graph.json`.
- Deterministic namespace-aware graph query.
- Skill registry from TOML manifests.
- MCP stdio server with small tools/resources.
- Evidence SHA-256 attach and verify helpers.
- Tests for event, graph, skill, and MCP behavior.

## Explicitly Out of Scope for v0.2

- Cloud sync.
- Web dashboard.
- Marketplace or hosted registry.
- A2A adapter.
- Auth, teams, or remote identities.
- Vector database.
- AI summarizer, model calls, or prompt generation.
- Implicit shell/network execution from skills.
- MCP file import tool.

## Phase Gate

A feature moves into active scope only when `docs/PROGRESS.md` and the relevant policy doc are updated in the same change.
