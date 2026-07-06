# Spec Design: v0.4.1 Release Smoke Automation

## Objective

Make the existing v0.4 release smoke checklist runnable as one local maintainer command.

## Changes

- Add a small Bash smoke script that builds the debug binary, creates a fresh `/tmp/meshlet-v04-smoke` workspace, runs the public-safe release smoke flow, and removes the workspace on exit.
- Keep the script as maintainer automation only; no Meshlet CLI subcommand, schema change, MCP change, Cargo version bump, or release tag.
- Update the release checklist to point maintainers at the script while preserving the manual steps.

## Boundaries

- Do not broaden v0.4.1 beyond adoption hardening.
- Do not change runtime behavior, public export shape, OKF format, MCP JSON-RPC shape, or SQLite schema.
- Do not add dependencies or CI release workflow.
- Preserve unrelated pre-existing working-tree edits.

## Success Criteria

- `rtk run scripts/release_smoke.sh` exercises init, event append, verify, public doctor, JSON export, OKF export/doctor, public-safe MCP digest, public-safe MCP mutation rejection, and private-marker absence checks.
- Temporary smoke state is removed on success or failure.
- Local locked gates and clippy pass.
