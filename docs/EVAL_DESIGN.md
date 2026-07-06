# Eval Design: Public OKF Limit Fix

## Verification

- Add a regression where a private event is newer than a public event and OKF export runs with `limit=1`.
- Confirm `public_export_okf` uses public-safe event filtering before applying the limit.
- Run `rtk cargo fmt --check`, `rtk cargo check --locked`, `rtk cargo test --locked`, and `rtk cargo clippy --all-targets --all-features -- -D warnings`.
- Inspect `rtk git diff --stat` and targeted diffs for source, tests, and docs.

## Risk Focus

- Avoid leaking private/local events into OKF output.
- Avoid changing public export schema or OKF markdown format.
- Avoid broad refactors that hide the behavior fix.
