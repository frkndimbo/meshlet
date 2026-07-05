# Spec Design: src/lib.rs Helper Split

## Objective

Split the remaining helper families out of `src/lib.rs` without changing Meshlet behavior, public API, database schema, CLI output, MCP tools, OKF output, or public-safe visibility/redaction rules.

## Changes

- Move compact output helpers into `src/compact.rs`.
- Move public-safe projection helpers into `src/public_safe.rs`.
- Move OKF markdown/export helper types and functions into `src/okf.rs`.
- Keep public types, constants, CLI-facing parse helpers, validation helpers, row mappers, and the `Meshlet` entrypoint in `src/lib.rs`.
- Keep crate-root `pub(crate) use` re-exports so existing internal module call sites remain stable.

## Boundaries

- Always preserve event log as source of truth and graph/read models as rebuildable views.
- Never change SQL table shape, schema version, public export format, OKF format, public-safe projection semantics, or MCP tool/resource names in this refactor.
- Do not add dependencies, rename functions, or opportunistically fix unrelated issues.

## Success Criteria

- `rtk cargo fmt --check`, `rtk cargo check --locked`, `rtk cargo build --locked`, and `rtk cargo test --locked` pass.
- Existing 85 tests still pass or increase only from added tests.
- `rtk cargo clippy --all-targets -- -D warnings` passes for this task.
- `src/lib.rs` keeps no large compact/public-safe/OKF helper blocks.
