# GitHub Publication Notes

Use this page when preparing the repository profile, first release, or public cleanup.

## Repository About

Description:

```text
Local-first context mesh for agent workflows: a Rust CLI and MCP stdio runtime for durable events, rebuildable context graphs, skill manifests, public-safe exports, and agent mailboxes.
```

Website: leave empty for the first v0.4 release unless a docs site exists.

Topics:

```text
rust cli sqlite mcp local-first agent-workflows context-graph event-log public-safe okf
```

Short social/profile text:

```text
Meshlet keeps agent working context local, durable, queryable, and safe to share through visibility-aware public exports.
```

## Publication Hygiene

Tracked intentionally:

- Rust source, Cargo metadata, and lockfile.
- README, license, policy docs, release checklist, and product handoff.
- GitHub Actions workflow.
- Current Graphify summary outputs: `graphify-out/GRAPH_REPORT.md`, `graphify-out/graph.json`, and `graphify-out/manifest.json`.
- Project agent rules and portable hook config.

Must stay untracked:

- `.meshlet/` runtime state.
- SQLite database files and journal/WAL files.
- Local exports and OKF bundles generated from private/local state.
- Logs, temporary files, process IDs, and build output.
- Environment files, keys, certificates, credential stores, and local secret files.
- Graphify caches, dated snapshots, generated HTML, and hidden Graphify working files.
- Local Codex/agent sessions, inboxes, scratch files, and local override configs.

Before publishing a release:

```bash
rtk git status --short --branch
rtk cargo fmt --check
rtk cargo check
rtk cargo test
```

Then run `docs/RELEASE_CHECKLIST.md` from the final release commit.

## Current Ignore Assessment

The ignore policy blocks the high-risk local artifacts for a public Meshlet repository:

- Runtime DBs: `.meshlet/`, `*.db`, `*.sqlite`, `*.sqlite3`, and SQLite sidecar suffixes.
- Export artifacts: public JSON/OKF export names and local evidence directories.
- Secrets: `.env*`, PEM/key/certificate bundles, SSH key names, age/KeePass stores, and generic secret/credential files.
- Generated output: Rust `target/`, logs, temp files, editor metadata, Graphify cache/snapshots/HTML, and local Codex session state.

One public-hygiene fix was applied: `.codex/hooks.json` now uses `graphify hook-check` instead of an absolute host path.
