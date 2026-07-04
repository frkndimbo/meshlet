# Spec Design: src/lib.rs Simplification

## Objective

Split `src/lib.rs` into focused Rust modules without changing Meshlet behavior, public API, database schema, CLI output, MCP tools, or public-safe visibility rules.

## Changes

- Move existing `impl Meshlet` method groups into `schema`, `event_log`, `skills`, `tasks`, `mailbox`, `evidence`, `graph`, `query`, and `public_export` modules.
- Keep public types, constants, shared helpers, and tests in `src/lib.rs` unless a move is required to compile cleanly.
- Use `pub(crate)` only for helpers shared across modules.

## Boundaries

- Always preserve event log as source of truth and graph/read models as rebuildable views.
- Never change SQL table shape, schema version, public export format, or MCP tool/resource names in this refactor.
- Do not add dependencies or speculative helper modules.

## Success Criteria

- `rtk cargo fmt --check`, `rtk cargo check`, and `rtk cargo test` pass.
- Existing 74 tests still pass.
- `src/lib.rs` becomes an index plus shared core instead of a monolith.
