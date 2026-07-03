# Meshlet

Meshlet is a local-first context mesh for agent workflows. It gives agents a durable event log, a rebuildable context graph, a skill registry, and a small MCP stdio surface without adding a cloud service, dashboard, marketplace, vector database, or model runtime.

v0.4 focuses on a public-safe local runtime: SQLite storage, visibility-aware events, compact public exports, OKF markdown bundles, and a local agent mailbox.

## Why It Exists

Agent sessions are often transient, but useful working context is not. Meshlet keeps that context in a local, inspectable, append-only system:

```text
Agent -> MCP stdio -> Event Log -> Read Models -> Context Graph -> Public-Safe Views
```

The event log is the source of truth. Graph, task, mailbox, timeline, evidence, and search views are materialized from events and can be rebuilt.

## Current Status

- Version: `0.4.0`
- Runtime: local CLI + MCP stdio
- Storage: SQLite under `.meshlet/`
- Safety model: `private`, `local`, and `public` event visibility
- Release state: v0.4 release hardening / first GitHub release candidate

## Features

- Append-only event log with hash-chain verification.
- Visibility-aware public-safe query, digest, export, and MCP read paths.
- SQLite-backed read models for contexts, graph nodes/edges, skills, tasks, evidence, mailbox messages, and task timelines.
- FTS-backed deterministic query over compact safe fields.
- Graphify graph import with namespaces.
- Skill manifest registration with declared permissions.
- Evidence SHA-256 attach and verify helpers.
- MCP stdio server with typed tools/resources and structured JSON-RPC errors.
- Public-safe JSON export and OKF markdown export.
- `doctor public` checks before sharing public state.

## Quick Start

This repository uses `rtk` in local agent workflows. If you are not using RTK, remove the `rtk` prefix and run the equivalent `cargo` command.

```bash
rtk cargo build
rtk cargo run -- init
rtk cargo run -- status
```

Append and inspect a context event:

```bash
rtk cargo run -- event append --type context.added --json '{"label":"repo context"}'
rtk cargo run -- event list
rtk cargo run -- verify
```

Run a compact query:

```bash
rtk cargo run -- query "repo context" --kind all --limit 20 --mode compact
```

## Common Commands

| Command | Purpose |
|---|---|
| `rtk cargo run -- init` | Create local `.meshlet/` state. |
| `rtk cargo run -- status` | Show local runtime status. |
| `rtk cargo run -- verify` | Verify event hash-chain integrity. |
| `rtk cargo run -- event list` | List events. |
| `rtk cargo run -- query "term" --kind all --limit 20 --mode compact` | Search compact local context. |
| `rtk cargo run -- graph import graphify-out/graph.json --source graphify --namespace graphify:repo` | Import a namespaced Graphify graph. |
| `rtk cargo run -- skill add ./skill.toml` | Register a skill manifest. |
| `rtk cargo run -- task create --task-id task-1 --title "Ship mailbox"` | Create a typed task event. |
| `rtk cargo run -- mailbox send --from agent:a --to agent:b --summary "Please handle task-1" --task-id task-1` | Send an agent mailbox message. |
| `rtk cargo run -- evidence attach --path src/lib.rs --sha256 auto` | Attach evidence with a SHA-256 digest. |
| `rtk cargo run -- doctor public` | Check whether public sharing is safe. |
| `rtk cargo run -- export public --out /tmp/meshlet-public.json` | Write a compact public JSON bundle. |
| `rtk cargo run -- export public --format okf --out /tmp/meshlet-okf` | Write a public-safe OKF markdown bundle. |
| `rtk cargo run -- serve --mcp stdio --profile public-safe` | Start MCP stdio in public-safe mode. |

## Public-Safe Sharing

Meshlet is local-first, but it can publish sanitized public state. Public-safe paths only expose compact public data and reject mutation tools in public-safe MCP mode.

Before sharing anything:

```bash
rtk cargo run -- doctor public
rtk cargo run -- export public --out /tmp/meshlet-public.json
rtk cargo run -- export public --format okf --out /tmp/meshlet-okf
rtk cargo run -- okf doctor /tmp/meshlet-okf
```

Local runtime state, SQLite files, logs, private evidence, and generated exports must stay out of git.

## Verification

```bash
rtk cargo fmt --check
rtk cargo check
rtk cargo test
```

The v0.4 release checklist is in `docs/RELEASE_CHECKLIST.md`.

## v0.4 Scope

- Event log as source of truth.
- Context graph as a rebuildable materialized view.
- SQLite local storage.
- CLI for local operation.
- MCP stdio tools/resources.
- Skill registry with explicit manifests and permissions.
- Visibility-aware public-safe read/export paths.
- Agent task, evidence, mailbox, and timeline workflows.

## Non-Goals

These are intentionally out of scope for v0.4:

- Cloud sync.
- Web dashboard.
- Marketplace.
- A2A adapter.
- Auth/team mode.
- Vector database.
- AI summarizer or hosted model integration.

## Documentation

- `docs/SCOPE.md` - phase boundaries.
- `docs/PROGRESS.md` - current state and next tasks.
- `docs/MCP_POLICY.md` - MCP behavior rules.
- `docs/SKILL_POLICY.md` - skill manifest and permission rules.
- `docs/SECURITY_POLICY.md` - local safety rules.
- `docs/GRAPH_POLICY.md` - event/graph/Graphify rules.
- `docs/OKF_POLICY.md` - OKF export and doctor rules.
- `docs/RELEASE_CHECKLIST.md` - v0.4 release smoke checklist.
- `docs/GITHUB_PUBLICATION.md` - repository About text and publication hygiene notes.
- `docs/PRODUCT_HANDOFF.md` - product direction and phased roadmap.

## Security Notes

Event hashes include event visibility. If `verify` fails on a pre-hardening local DB, treat it as legacy local state and export any needed public data before creating fresh `.meshlet/` state.

Meshlet rejects secret-like payload keys and public-safe secret-looking values, but the safest workflow is still to keep private/local runtime state outside version control and run `doctor public` before sharing exports.

## License

MIT.
