# Diff Review: src/lib.rs Helper Split

## Reviewed

- `src/lib.rs` now declares helper modules and keeps public core types, CLI parse helpers, validation helpers, row mappers, and integration tests.
- `src/compact.rs` contains compact output helpers and keeps public-safe routing through `src/public_safe.rs`.
- `src/public_safe.rs` contains the public-safe evidence/file/endpoint whitelist projections.
- `src/okf.rs` contains the OKF document type and markdown helper functions used by `src/public_export.rs`.
- Crate-root `pub(crate) use` re-exports keep existing internal call sites stable.

## Verification

- `rtk cargo fmt --check`
- `rtk cargo check --locked`
- `rtk cargo build --locked`
- `rtk cargo test --locked` - 85 passed
- `rtk cargo clippy --all-targets -- -D warnings` - moved-code lint fixed; remaining failures are pre-existing unrelated lints in `src/graph.rs`, `src/mailbox.rs`, `src/tasks.rs`, and unchanged `src/lib.rs` code.

## Result

Behavior-preserving helper split. No schema, dependency, CLI, MCP, public-safe projection semantics, or public export format changes.
