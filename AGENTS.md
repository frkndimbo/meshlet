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

## Optimization
- Use Ponytail as an additive simplicity check when available.
- Prefer YAGNI, existing repo patterns, stdlib, and minimal correct code.
- Do not use Ponytail to cut security checks, validation, tests, error handling, accessibility, or documented phase rules.
- If Ponytail advice conflicts with Meshlet scope, security, or architecture rules, keep the Meshlet rule.

## Token Economy
- Preserve accuracy by reducing context and output noise, not evidence, safety, validation, tests, or required verification.
- Use graph-first navigation for architecture questions when `graphify-out/graph.json` exists.
- Search before reading; read narrow files or line ranges and avoid full `graph.json` unless needed.
- Use `rtk git diff --stat` before detailed diffs; inspect full diffs only for changed or high-risk files.
- Remove transient/generated scratch files with `rtk` cleanup commands instead of `apply_patch` to avoid noisy deleted-file diffs.
- Keep patches small and scoped. Avoid broad rewrites unless explicitly required.
- Report successful verification tersely; include full logs only for failures or ambiguous results.
- Load only the minimal relevant skills for the task.

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
- For `/graphify`, use the `graphify` skill before other work.
- If `graphify-out/graph.json` exists, answer architecture questions with `graphify query`, `graphify path`, or `graphify explain` before raw file reads.
- Use `graphify-out/wiki/index.md` for broad navigation when present; read `GRAPH_REPORT.md` only for broad architecture review or insufficient query results.
- Dirty graphify outputs are expected after hooks or incremental updates; skip graph-first only for stale/incorrect graph tasks or explicit user opt-out.
- Run `graphify update .` after structural source/docs/policy/MCP changes; run clustering/report refresh only when report/community data is needed.

## Git
- Stage specific files only.
- Use Conventional Commits.
- Commit author: `frkndimbo <da.purplecats@gmail.com>`.
- Do not add co-author trailers.
