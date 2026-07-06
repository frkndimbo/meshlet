# Eval Design: v0.4.1 Release Smoke Automation

## Verification

- Run `rtk cargo fmt --check`.
- Run `rtk cargo check --locked`.
- Run `rtk cargo build --locked`.
- Run `rtk cargo test --locked`.
- Run `rtk cargo clippy --all-targets --all-features -- -D warnings`.
- Run `rtk run scripts/release_smoke.sh`.
- Inspect `rtk git diff --stat` and targeted diffs for docs and script changes.

## Risk Focus

- Avoid leaving `/tmp/meshlet-v04-smoke` behind after failures.
- Avoid leaking private/local markers, local evidence paths, or rejected mutation payloads into public outputs.
- Avoid changing public runtime behavior while adding maintainer automation.
