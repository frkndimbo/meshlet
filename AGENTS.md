# Agent Instructions

## Shell
- Use `rtk` for every shell command.
- Use `rtk run` for shell operators, pipes, redirects, expansions, or heredocs.
- Do not run bare `cargo`, `git`, `rg`, `sed`, `curl`, or `find`.

## Product Scope
- Meshlet is a Rust-core local context mesh for agent workflows.
- v0.1 scope: event log, context graph, skill registry, MCP stdio, CLI.
- Do not add cloud sync, dashboard, marketplace, A2A, auth, vector DB, or AI summarizer without an explicit phase change.

## Architecture Rules
- Event log is the source of truth.
- Context graph is a rebuildable materialized view from events.
- SQLite is the v0.1 storage backend.
- MCP tools must be small, typed, and return structured JSON-RPC errors.
- Skills must use explicit manifests and declared permissions.

## Security
- Never store secrets in events, graph attrs, logs, tests, docs, or examples.
- Do not add implicit shell or network execution from skill manifests.
- Keep local DBs and generated runtime state out of git.

## Commands
| Task | Command |
|---|---|
| Format check | `rtk cargo fmt --check` |
| Check | `rtk cargo check` |
| Test | `rtk cargo test` |
| Run CLI | `rtk cargo run -- <args>` |

## Progressive Docs
- Update `docs/PROGRESS.md` when project state or next tasks change.
- Update policy docs when behavior changes: MCP, skills, security, graph, or scope.
- Move stale notes to deprecated/superseded instead of leaving them active.

## Graphify
- If `graphify-out/graph.json` exists, answer architecture questions from graph first.
- Rerun Graphify manually after structural source/docs/policy/MCP changes.
- Do not rely on stale graph reports for repo readiness checks.

## Git
- Stage specific files only.
- Use Conventional Commits.
- Commit author: `frkndimbo <da.purplecats@gmail.com>`.
- Do not add co-author trailers.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

When the user types `/graphify`, invoke the `skill` tool with `skill: "graphify"` before doing anything else.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- Dirty graphify-out/ files are expected after hooks or incremental updates; dirty graph files are not a reason to skip graphify. Only skip graphify if the task is about stale or incorrect graph output, or the user explicitly says not to use it.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
