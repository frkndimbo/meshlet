# Meshlet Progress

## Current Phase

v0.1 - Local Runtime.

## Current State

- Rust CLI crate exists.
- SQLite event log exists under `.meshlet/` after `meshlet init`.
- Events include `repo.initialized`, `skill.added`, `context.added`, `agent.message`, `evidence.attached`, `task.created`, and `task.updated`.
- Event hash chains can be verified with `meshlet verify`.
- Graph nodes and edges are materialized from events and can be rebuilt.
- Deterministic local query is available through CLI and MCP as `meshlet_query`.
- Event, graph, and context outputs use bounded limits for large local state.
- Skill manifests can be registered from TOML with permission allowlist and entry path hygiene.
- Task and evidence views are available through CLI and MCP.
- Evidence can support tasks through graph edges.
- MCP stdio skeleton supports initialize, tools/list, tools/call, resources/list, and resources/read.
- Tests cover init, hash chaining, chain verification, secret-key rejection, skill materialization and validation, deterministic query, graph rebuild determinism, task/evidence views, bounded context, and direct MCP behavior for initialize, tools, resources, valid tool calls, and invalid tool params.
- Agent docs and policy docs exist: AGENTS.md, README.md, docs/SCOPE.md, docs/MCP_POLICY.md, docs/SKILL_POLICY.md, docs/SECURITY_POLICY.md, docs/GRAPH_POLICY.md.
- Graphify Codex integration is installed through AGENTS.md and .codex/hooks.json.
- Graphify code graph exists in graphify-out/ with GRAPH_REPORT.md, graph.json, and manifest.json. Semantic extraction was quota-blocked, so the current graph is code-focused.
- Ponytail is installed and enabled in the local Codex plugin registry as a simplicity/over-engineering guard. Project rules keep it subordinate to Meshlet safety, scope, architecture, and verification gates.

## Last Verified

- 2026-06-26: `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test` passed after task/evidence journal and skill manifest hardening implementation. Test result: 26 passed.
- 2026-06-26: Graphify refreshed with `rtk run graphify update . --force --no-cluster` and `rtk run graphify cluster-only . --no-viz --no-label`. Result: 219 nodes, 566 edges, 16 communities.
- 2026-06-26: `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test` passed after verify/query/limit/secret guard implementation. Test result: 21 passed.
- 2026-06-26: Graphify refreshed with `rtk run graphify update . --force --no-cluster` and `rtk run graphify cluster-only . --no-viz --no-label`. Result: 203 nodes, 500 edges, 16 communities.
- 2026-06-26: Ponytail marketplace added and `ponytail@ponytail` installed/enabled in Codex plugin registry at version 4.8.3.
- 2026-06-25: `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test` passed after Graphify install and policy docs. Test result: 4 passed.
- 2026-06-25: Graphify Codex install completed. Code-focused graph/report generated after full semantic extraction hit quota.
- 2026-06-25: `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test` passed after MCP behavior tests. Test result: 12 passed.
- 2026-06-25: Graphify refreshed with `rtk proxy graphify update . --force --no-cluster` and `rtk proxy graphify cluster-only . --no-viz --no-label`.

## Next 3 Tasks

1. Split `src/lib.rs` into event, graph, skill, MCP, and type modules if the next feature needs more core changes.
2. Add evidence digest verification helpers if real evidence files become a core workflow.
3. Add lightweight query indexes only if real local usage shows current SQLite scans are too slow.

## Maintenance Rules

- Update this file when scope, current state, verification status, or next tasks change.
- Keep only current work under `Next 3 Tasks`.
- Move obsolete notes to `Deprecated / Superseded`.

## Deprecated / Superseded

- Cloud sync remains future scope, not v0.1 work.
- Graphify is manual-trigger only, not a watch mode or commit hook.
