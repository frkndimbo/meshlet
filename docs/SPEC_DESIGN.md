# Spec Design: v0.4.1 `init --adopt`

## Objective

Add an adoption mode to `meshlet init` that prepares a local repo for Meshlet without changing the existing plain init behavior.

## Changes

- Keep `meshlet init` output and side effects as-is: initialize `.meshlet/` and print the existing initialized JSON.
- Add `meshlet init --adopt` with `--agent`, `--okf`, `--graphify-out`, and `--no-patch-agents`.
- Adoption creates `meshlet.toml` only when missing, creates the minimal OKF skeleton, appends missing `.gitignore` lines, and appends one Meshlet section to `AGENTS.md` unless disabled.
- Adoption prints stable structured JSON with `status`, `root`, `state`, `config`, `okf`, `created`, `patched`, `already_present`, and `next`.

## Boundaries

- Do not broaden v0.4.1 beyond adoption hardening.
- Do not change event log behavior, public export shape, MCP JSON-RPC shape, SQLite schema, or existing command behavior.
- Do not add dependencies, cloud sync, auth, dashboard, vector DB, or AI summarization.
- Preserve unrelated pre-existing working-tree edits.

## Success Criteria

- `meshlet init --adopt` is idempotent for `meshlet.toml`, OKF skeleton files, `.gitignore`, and `AGENTS.md`.
- `.gitignore` receives `.meshlet/` and the configured Graphify output directory only once.
- `.meshlet-okf/` is not ignored by default; output reports next-step guidance through `next`.
- `created` and `already_present` report `.meshlet/meshlet.db`, `meshlet.toml`, and OKF markdown files factually for each run.
- OKF directory entries are not reported as created files.
- Local locked gates pass.
