# Eval Design: src/lib.rs Helper Split

## Verification

- Baseline before edits: `rtk cargo fmt --check`, `rtk cargo check --locked`, `rtk cargo build --locked`, and `rtk cargo test --locked`.
- After the helper extraction: run `rtk cargo fmt --check`, `rtk cargo check --locked`, `rtk cargo build --locked`, and `rtk cargo test --locked`.
- Run `rtk cargo clippy --all-targets -- -D warnings` after the moved modules compile.
- Final: inspect `rtk git diff --stat` and targeted diffs for `src/lib.rs`, `src/compact.rs`, `src/public_safe.rs`, `src/okf.rs`, `src/public_export.rs`, and docs.

## Risk Focus

- Event hash/visibility behavior.
- SQL visibility filters before `LIMIT`.
- Public export and OKF output staying compact/public-safe.
- MCP public-safe mutation guards and resource names.
- Migration/backfill behavior.
- Moved public-safe helpers must stay byte-for-byte equivalent in behavior.
