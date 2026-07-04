# Eval Design: src/lib.rs Simplification

## Verification

- Baseline before edits: `rtk cargo fmt --check`, `rtk cargo check`, `rtk cargo test`.
- After each extraction group: run `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test`.
- Final: inspect `rtk git diff --stat` and targeted diffs for `src/lib.rs`, module files, `src/mcp.rs`, and docs.

## Risk Focus

- Event hash/visibility behavior.
- SQL visibility filters before `LIMIT`.
- Public export and OKF output staying compact/public-safe.
- MCP public-safe mutation guards and resource names.
- Migration/backfill behavior.
