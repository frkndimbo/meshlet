# Meshlet

Meshlet is a local-first context mesh for agent workflows. It stores durable events, materializes a context graph, registers skills, and exposes local state through MCP stdio.

Meshlet is not a model, chatbot, dashboard, cloud service, or marketplace. v0.4 stays small: public-safe local runtime, CLI, SQLite, MCP stdio, sanitized export/digest paths, and a local agent mailbox.

## Core Flow

```text
Agent -> MCP -> Meshlet -> Event Log -> Context Graph -> Skill Registry
```

## Current Features

- Append-only local event log with hash chaining.
- Event hash-chain verification.
- SQLite-backed local project state in `.meshlet/`.
- Rebuildable graph nodes and edges derived from events.
- Namespaced Graphify graph import.
- Deterministic event/graph query.
- Skill manifest registration from TOML.
- Task, evidence, mailbox, and timeline views for agent work journals.
- Evidence SHA-256 attach and verify helpers.
- MCP stdio server exposing Meshlet tools and resources.
- Public-safe profile with compact output, visibility filtering, and public doctor/export commands.
- Public-safe OKF markdown export and OKF bundle doctor.

## Commands

```bash
rtk cargo run -- init
rtk cargo run -- status
rtk cargo run -- verify
rtk cargo run -- event append --type context.added --json '{"label":"repo context"}'
rtk cargo run -- event list
rtk cargo run -- query "repo context" --kind all --limit 20 --mode compact
rtk cargo run -- query "Meshlet" --kind nodes --namespace graphify:repo --limit 20
rtk cargo run -- event append --type context.added --visibility public --json '{"label":"public context"}'
rtk cargo run -- graph nodes --limit 20
rtk cargo run -- graph import graphify-out/graph.json --source graphify --namespace graphify:repo
rtk cargo run -- graph namespaces
rtk cargo run -- skill add ./skill.toml
rtk cargo run -- task create --task-id task-1 --title "Ship mailbox" --assignee agent:b
rtk cargo run -- task update task-1 --status in_progress
rtk cargo run -- task list
rtk cargo run -- task timeline task-1
rtk cargo run -- mailbox send --from agent:a --to agent:b --summary "Please handle task-1" --task-id task-1
rtk cargo run -- mailbox inbox agent:b
rtk cargo run -- evidence attach --path src/lib.rs --sha256 auto
rtk cargo run -- evidence verify <evidence-id>
rtk cargo run -- evidence list
rtk cargo run -- doctor public
rtk cargo run -- export public --out /tmp/meshlet-public.json
rtk cargo run -- export public --format okf --out /tmp/meshlet-okf
rtk cargo run -- okf doctor /tmp/meshlet-okf
rtk cargo run -- serve --mcp stdio --profile public-safe
```

## Verification

```bash
rtk cargo fmt --check
rtk cargo check
rtk cargo test
```

v0.4 hardening note: event hashes include event visibility. If `verify` fails on a pre-hardening local DB, treat it as legacy local state and export any needed public data before creating fresh `.meshlet/` state.

## v0.4 Scope

- Event log as source of truth.
- Context graph as materialized view.
- Namespaced graph imports from Graphify output.
- Skill registry with declared permissions.
- MCP stdio tools/resources.
- CLI for local operation.
- Visibility-aware events: `private`, `local`, and `public`.
- Compact public-safe query/digest/export outputs.
- Public readiness check through `doctor public`.
- Public OKF export for portable human/agent-readable sharing.
- OKF bundle doctor for frontmatter and markdown-link sanity checks.
- Typed local task state machine.
- Agent inbox/outbox over `agent.message` events.
- Compact task timeline replay.

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
- `docs/OKF_POLICY.md` - OKF export and doctor rules.
- `docs/RELEASE_CHECKLIST.md` - v0.4 release smoke checklist.
- `docs/PRODUCT_HANDOFF.md` - product direction and phased roadmap.
