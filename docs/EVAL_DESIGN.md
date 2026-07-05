# Eval Design: CI Workflow Recovery

## Verification

- Inspect recent `PUSAT` workflow runs with `gh run list`.
- Inspect the latest failed run with `gh run view --json ... jobs` and confirm whether steps were recorded.
- Fetch public job-page annotations when logs require admin access.
- Attempt repository Actions permission API reads; record explicit auth/scope failures instead of guessing.
- After workflow/doc edits, run `rtk cargo fmt --check`, `rtk cargo check --locked`, `rtk cargo build --locked`, and `rtk cargo test --locked`.
- Inspect `rtk git diff --stat` and targeted diffs for `.github/workflows/rust.yml`, docs, and release-gate status.

## Risk Focus

- Distinguish in-repo workflow defects from account-level GitHub Actions blockers.
- Avoid running account-level permission changes without explicit confirmation.
- Keep release tagging blocked until CI is actually green on `PUSAT`.
- Keep the v0.4.1 next-task list unchanged.
