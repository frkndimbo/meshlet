# Spec Design: CI Workflow Recovery

## Objective

Recover GitHub Actions visibility for the v0.4 release gate without changing Meshlet runtime behavior.

## Changes

- Diagnose the zero-step GitHub Actions failures from current run metadata and public job annotations.
- Add `workflow_dispatch` to `.github/workflows/rust.yml` so CI can be manually rerun after the account-level blocker is cleared.
- Record the CI blocker and verification status in project docs.

## Boundaries

- Do not change Rust source, Cargo metadata, database schema, public export behavior, MCP behavior, or Meshlet scope.
- Do not change repository/account Actions permissions without explicit human confirmation.
- Do not tag or publish `v0.4.0` until CI is verifiably green on `PUSAT`.
- Do not start v0.4.1 adoption-hardening work.

## Success Criteria

- `rtk cargo fmt --check`, `rtk cargo check --locked`, `rtk cargo build --locked`, and `rtk cargo test --locked` pass.
- A fresh CI run can be manually dispatched after the account-level blocker is resolved.
- A `PUSAT` CI run shows real `Format`, `Check`, `Build`, and `Test` steps before release tagging proceeds.
