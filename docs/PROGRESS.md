# Meshlet Progress

## Current Phase

v0.3 - Public-Safe Local Runtime.

## Current State

- Rust CLI crate exists.
- SQLite event log exists under `.meshlet/` after `meshlet init`.
- Events include `repo.initialized`, `skill.added`, `context.added`, `agent.message`, `evidence.attached`, `task.created`, `task.updated`, and `graph.imported`.
- Event hash chains can be verified with `meshlet verify`.
- Contexts are materialized from `context.added` events into the `contexts` read model.
- Graph nodes and edges are materialized from events and can be rebuilt.
- Graphify `graph.json` can be imported through CLI as a namespaced graph.
- Graph namespaces can be listed through CLI and MCP.
- Deterministic local query is available through CLI and MCP as `meshlet_query`.
- Query supports optional graph namespace filtering for nodes and edges.
- Event, graph, and context outputs use bounded limits for large local state.
- Context list/search helpers apply visibility filtering in SQL before `LIMIT`.
- Graph nodes, graph edges, and skills have SQL `visibility` columns for public-safe read paths before FTS5.
- Graph and skill scoped helpers apply visibility filtering in SQL before `LIMIT`.
- Skill manifests can be registered from TOML with permission allowlist and entry path hygiene.
- Task and evidence views are available through CLI and MCP.
- Evidence can support tasks through graph edges.
- Evidence attach can compute SHA-256 digests, and evidence verify can compare stored digest with current file contents.
- Events have `private`, `local`, or `public` visibility.
- Public-safe query/digest/export paths use compact output and filter private/local state.
- Full public export rewrite through dedicated public-safe views/queries remains deferred.
- `meshlet doctor public` checks event-chain integrity and stored secret-like data before sharing.
- `meshlet export public` writes a compact sanitized public bundle.
- MCP stdio skeleton supports initialize, tools/list, tools/call, resources/list, and resources/read.
- Tests cover init, hash chaining, chain verification, secret-key rejection, public-safe value rejection, visibility-filtered compact query, context materialization, deterministic context rebuild, v2-to-v3 context migration, graph/skill SQL visibility materialization and v3-to-v4 migration, public doctor, skill materialization and validation, deterministic query, graph imports, graph rebuild determinism, namespace filtering, task/evidence views, evidence digest verification, bounded context, and direct MCP behavior for initialize, tools, resources, valid tool calls, public-safe guards, and invalid tool params.
- Agent docs and policy docs exist: AGENTS.md, README.md, docs/SCOPE.md, docs/MCP_POLICY.md, docs/SKILL_POLICY.md, docs/SECURITY_POLICY.md, docs/GRAPH_POLICY.md.
- Graphify Codex integration is installed through AGENTS.md and .codex/hooks.json.
- Graphify code graph exists in graphify-out/ with GRAPH_REPORT.md, graph.json, and manifest.json. Semantic extraction was quota-blocked, so the current graph is code-focused.
- Ponytail is installed and enabled in the local Codex plugin registry as a simplicity/over-engineering guard. Project rules keep it subordinate to Meshlet safety, scope, architecture, and verification gates.

## Last Verified

- 2026-06-28: `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test` passed after Patch 2 SQL visibility column implementation. Test result: 53 passed.
- 2026-06-28: `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test` passed after Patch 1 contexts read model implementation. Test result: 46 passed.
- 2026-06-26: `rtk cargo test` passed after v0.3 public-safe runtime implementation. Test result: 42 passed.
- 2026-06-26: Graphify refreshed after v0.3 source and policy changes. Result: 293 nodes, 877 edges, 18 communities.
- 2026-06-26: `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test` passed after v0.2 graph import, namespace query, MCP namespace, and evidence digest implementation. Test result: 37 passed.
- 2026-06-26: Graphify refreshed with `rtk run graphify update . --force --no-cluster` and `rtk run graphify cluster-only . --no-viz --no-label`. Result: 250 nodes, 674 edges, 22 communities.
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

1. Add FTS5-backed local search for contexts/events compact text; FTS5 is an architecture target, not a perf-only optional index.
2. Design public-safe export views/queries after SQL visibility coverage is stable.
3. Design the blob metadata table and retrieve-by-hash flow after read-model visibility stays stable.

## Maintenance Rules

- Update this file when scope, current state, verification status, or next tasks change.
- Keep only current work under `Next 3 Tasks`.
- Move obsolete notes to `Deprecated / Superseded`.

## Deprecated / Superseded

- Cloud sync remains future scope, not v0.1 work.
- Graphify is manual-trigger only, not a watch mode or commit hook.
