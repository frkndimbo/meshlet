# Diff Review: CI Workflow Recovery

## Reviewed

- `.github/workflows/rust.yml` keeps the existing `push` and `pull_request` triggers for `PUSAT`.
- `.github/workflows/rust.yml` now includes `workflow_dispatch` so the same Rust quality gate can be rerun manually after the account-level blocker is cleared.
- `docs/SPEC_DESIGN.md` and `docs/EVAL_DESIGN.md` describe the CI recovery scope, verification, and release gate.
- `docs/PROGRESS.md` records the CI diagnosis and local verification.

## Verification

- `rtk cargo fmt --check`
- `rtk cargo check --locked`
- `rtk cargo build --locked`
- `rtk cargo test --locked` - 88 passed
- `rtk proxy gh run view 28735853418 --json databaseId,status,conclusion,event,headBranch,headSha,workflowName,createdAt,updatedAt,jobs` showed job `quality` with `steps: []`, `runner_id: 0`, and 3s runtime.
- Public GitHub job-page annotation showed: "The job was not started because your account is locked due to a billing issue."
- Repository Actions permission API checks returned HTTP 401/403 without admin auth, so no account-level settings were changed.

## Result

In-repo workflow recovery is limited to adding a manual dispatch trigger. Release tagging remains blocked until the GitHub account billing lock is resolved and `PUSAT` CI runs real `Format`, `Check`, `Build`, and `Test` steps successfully.
