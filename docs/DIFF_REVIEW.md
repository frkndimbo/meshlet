# Diff Review: v0.4.1 Release Smoke Automation

## Reviewed

- `scripts/release_smoke.sh` mirrors the existing manual release checklist without changing Meshlet runtime behavior.
- Smoke workspace cleanup is fixed to `/tmp/meshlet-v04-smoke` and verified after execution.
- Spec/eval/progress/checklist docs describe the automation tranche and preserve release-blocker scope.

## Verification

- `rtk run bash -n scripts/release_smoke.sh`
- `rtk cargo fmt --check`
- `rtk cargo check --locked`
- `rtk cargo build --locked`
- `rtk cargo test --locked` - 89 passed
- `rtk cargo clippy --all-targets --all-features -- -D warnings`
- `rtk run scripts/release_smoke.sh` - exit 0
- `rtk run graphify update .`

## Result

v0.4.1 now has a single local maintainer smoke command for the release checklist. Public release/tag work remains blocked until GitHub Actions can run real steps.
