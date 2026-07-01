# Meshlet Scope

## Product Line

Meshlet is a Rust-core local context mesh for agent workflows. It gives agents and humans a durable local substrate for events, graph context, skills, and MCP access.

## v0.4 Active Scope

- CLI-first local runtime.
- SQLite storage under `.meshlet/`.
- Append-only event log with hash chaining.
- Context graph materialized from events.
- Namespaced graph import from Graphify `graph.json`.
- Deterministic namespace-aware graph query.
- Skill registry from TOML manifests.
- MCP stdio server with small tools/resources.
- Evidence SHA-256 attach and verify helpers.
- Public-safe local profile with compact outputs and visibility filtering.
- Public doctor/export commands for sanitized local sharing.
- Public-safe OKF markdown export and OKF bundle doctor.
- Typed task creation/update with a small local state machine.
- Agent mailbox over local `agent.message` events.
- Inbox/outbox and task timeline replay through CLI and MCP.
- Tests for event, graph, skill, and MCP behavior.

## Explicitly Out of Scope for v0.4

- Cloud sync.
- Web dashboard.
- Marketplace or hosted registry.
- A2A adapter.
- Auth, teams, or remote identities.
- Remote MCP transport.
- Vector database.
- AI summarizer, model calls, or prompt generation.
- Implicit shell/network execution from skills.
- MCP file import tool.
- OKF import.
- Local-trusted full OKF export.

## Phase Gate

A feature moves into active scope only when `docs/PROGRESS.md` and the relevant policy doc are updated in the same change.
