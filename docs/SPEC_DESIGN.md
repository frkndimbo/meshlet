# Spec Design: Public OKF Limit Fix

## Objective

Fix the public-safe OKF export limit path so visibility filtering happens before `LIMIT`.

## Changes

- Use the existing scoped event reader in OKF export instead of filtering after a local-trusted event limit.
- Keep the fix narrow: no schema, MCP, CLI, dependency, or export-format changes.
- Add a regression test for a newer private event hiding an older public event at `limit=1`.
- Apply minimal clippy cleanup only where it keeps the code simpler.

## Boundaries

- Do not broaden v0.4 scope.
- Do not change Cargo metadata, SQLite schema, MCP wire shape, CLI args, or OKF document format.
- Do not refactor inline tests, batch timelines, or introduce request structs unless required by verification.
- Preserve unrelated pre-existing working-tree edits.

## Success Criteria

- OKF public export returns the older public event even when a newer private event exists and `limit=1`.
- Public-safe output remains compact and public-only.
- `rtk cargo fmt --check`, `rtk cargo check --locked`, `rtk cargo test --locked`, and `rtk cargo clippy --all-targets --all-features -- -D warnings` pass.
