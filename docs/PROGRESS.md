# Meshlet Progress

## Current Phase

v0.4 - Agent Mailbox.

## Current State

- Rust CLI crate exists.
- Core library implementation is split across focused modules under `src/` while `src/lib.rs` keeps public types, constants, shared helpers, and tests.
- SQLite event log exists under `.meshlet/` after `meshlet init`.
- Events include `repo.initialized`, `skill.added`, `context.added`, `agent.message`, `evidence.attached`, `task.created`, `task.updated`, and `graph.imported`.
- Event hash chains can be verified with `meshlet verify`.
- v0.4 hardening uses strict visibility-bound event hashes; pre-hardening local DBs may fail verification and should be treated as legacy local state.
- Contexts are materialized from `context.added` events into the `contexts` read model.
- Tasks are materialized from `task.created` and `task.updated` events into the `tasks` read model.
- Agent messages are materialized from `agent.message` events into the `mailbox_messages` read model.
- Graph nodes and edges are materialized from events and can be rebuilt.
- Graphify `graph.json` can be imported through CLI as a namespaced graph.
- Graph namespaces can be listed through CLI and MCP.
- Deterministic local query is available through CLI and MCP as `meshlet_query`.
- FTS5 is the primary text search path for compact events, contexts, graph nodes, graph edges, and skills.
- Query supports optional graph namespace filtering for nodes and edges.
- Event, graph, and context outputs use bounded limits for large local state.
- FTS-backed public-safe searches join to base read-model tables and apply SQL visibility filtering before `LIMIT`.
- Raw `payload_json` and `attrs_json` scans are not core retrieval paths; event search indexes compact safe fields only.
- Graph nodes, graph edges, and skills have SQL `visibility` columns for public-safe read paths.
- Graph and skill scoped helpers apply visibility filtering in SQL before `LIMIT`.
- Skill manifests can be registered from TOML with permission allowlist and entry path hygiene.
- Task, mailbox, timeline, and evidence views are available through CLI and MCP.
- Task status transitions are constrained to `open`, `in_progress`, `blocked`, `done`, and `canceled`.
- Public-safe task reads replay only public task events, so private/local task metadata is not exposed by digest, task tools, or task resources.
- Evidence can support tasks through graph edges.
- Evidence attach can compute SHA-256 digests, and evidence verify can compare stored digest with current file contents.
- Public-safe evidence reads use a compact whitelist projection and do not expose raw graph attrs, local paths, notes, or path-derived file node IDs.
- Events have `private`, `local`, or `public` visibility.
- Public-safe query/digest/export paths use compact output and filter private/local state.
- Full public export rewrite through dedicated public-safe views/queries remains deferred.
- Blob/CCR metadata and retrieve-by-hash flow remain deferred.
- `meshlet doctor public` checks event-chain integrity and stored secret-like data before sharing.
- `meshlet export public` writes a compact sanitized public bundle with public events, tasks, mailbox message envelopes, timelines, and graph data.
- `meshlet export public --format okf` writes a public-safe OKF markdown bundle with contexts, tasks, message envelopes, skills, evidence, event log, and compact task timelines.
- `meshlet okf doctor` checks OKF concept frontmatter and local markdown links.
- MCP stdio skeleton supports initialize, tools/list, tools/call, resources/list, and resources/read.
- Tests cover init, hash chaining, chain verification, secret-key rejection, public-safe value rejection, visibility-filtered compact query, context materialization, deterministic context and FTS rebuilds, v2-to-v3 context migration, graph/skill SQL visibility materialization, v3-to-v4 migration, v4-to-v5 FTS migration, v5-to-v6 task/mailbox read-model creation, FTS-backed events/contexts/graph/skills search, public-safe FTS limit regression, public-safe task filtering, public doctor/export coverage, OKF message/timeline export, skill materialization and validation, deterministic query, graph imports, graph rebuild determinism, namespace filtering, task/evidence/mailbox/timeline views, evidence digest verification, bounded context, and direct MCP behavior for initialize, tools, resources, v0.4 task/mailbox/timeline tools, public-safe guards, and invalid tool params.
- Agent docs and policy docs exist: AGENTS.md, README.md, docs/SCOPE.md, docs/MCP_POLICY.md, docs/SKILL_POLICY.md, docs/SECURITY_POLICY.md, docs/GRAPH_POLICY.md.
- OKF policy docs exist in docs/OKF_POLICY.md.
- v0.4 release smoke checklist exists in docs/RELEASE_CHECKLIST.md.
- GitHub publication metadata and repository hygiene notes exist in docs/GITHUB_PUBLICATION.md.
- Public repository no longer tracks local Codex config or generated Graphify output.
- Ponytail is installed and enabled in the local Codex plugin registry as a simplicity/over-engineering guard. Project rules keep it subordinate to Meshlet safety, scope, architecture, and verification gates.
- Local tool baselines: Graphify CLI/skill 0.9.1, Ponytail plugin 4.8.4, RTK 0.43.0.

## Last Verified

- 2026-07-05: `rtk cargo fmt --check`, `rtk cargo check`, `rtk cargo test --test public_safe_boundary -- --nocapture`, `rtk cargo test`, `rtk cargo build`, and fresh release smoke passed after public-safe evidence projection hardening. Test result: 85 passed.
- 2026-07-04: `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test` passed after behavior-preserving `src/lib.rs` module split. Test result: 74 passed.
- 2026-07-04: `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test` passed after removing tracked local Codex config and generated Graphify output. Test result: 74 passed.
- 2026-07-04: `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test` passed after README/publication hygiene refinement. Test result: 74 passed.
- 2026-07-04: `rtk cargo fmt --check`, `rtk cargo check`, `rtk cargo build`, `rtk cargo test`, and v0.4 release smoke checklist passed after adding the release checklist and CI quality gates. Test result: 74 passed.
- 2026-07-04: `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test` passed after v0.4 strict hash-chain docs. Test result: 74 passed.
- 2026-07-02: `rtk cargo fmt --check`, `rtk cargo check`, `rtk cargo test`, and OKF export/doctor smoke passed after public export mailbox/timeline coverage. Test result: 69 passed.
- 2026-07-02: Graphify refreshed after v0.4 public export stabilization. Result: 404 nodes, 1415 edges, 30 communities.
- 2026-06-30: `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test` passed after OKF public export and doctor implementation. Test result: 69 passed.
- 2026-06-30: `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test` passed after v0.4 Agent Mailbox implementation. Test result: 67 passed.
- 2026-06-30: Ponytail plugin refreshed with `rtk proxy codex plugin add ponytail@ponytail --json`; installed path resolved to `/home/d0mb1/.codex/plugins/cache/ponytail/ponytail/4.8.4`.
- 2026-06-29: Graphify updated from 0.8.45 to 0.9.1 with `rtk proxy uv tool upgrade graphifyy`; `graphify install --platform codex` and project-scoped Codex install refreshed the installed skills. Ponytail 4.8.3 and RTK 0.43.0 were already current against their upstream tags.
- 2026-06-28: `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test` passed after Patch 3 FTS5-first visibility-safe search implementation. Test result: 61 passed.
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

1. Rerun the v0.4 release smoke checklist from the final release commit before tagging.
2. Tag `v0.4.0` from clean `PUSAT` after release checks pass.
3. Start v0.4.1 adoption hardening after the tag: branding cleanup, install docs, binary release workflow, event schema docs, and public-safe contract docs.

## Maintenance Rules

- Update this file when scope, current state, verification status, or next tasks change.
- Keep only current work under `Next 3 Tasks`.
- Move obsolete notes to `Deprecated / Superseded`.

## Deprecated / Superseded

- Cloud sync remains future scope, not v0.1 work.
- Graphify is manual-trigger only, not a watch mode or commit hook.
