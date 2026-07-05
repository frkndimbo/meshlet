# Contributing

## Branches

- Base release work from `PUSAT`.
- Use scoped branch names such as `hardening/<topic>`, `fix/<topic>`, or `docs/<topic>`.

## Commits

- Use Conventional Commits: `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, or `chore:`.
- Keep one logical change per commit.
- Do not commit `.meshlet/`, generated DB files, `target/`, or `graphify-out/`.

## Phase Gate

- Changes that move scope into active v0.4 behavior must update `docs/PROGRESS.md` and the relevant policy doc in the same commit.
- Respect `docs/SCOPE.md`; cloud sync, dashboard, auth, vector DB, AI summarizer, marketplace, A2A, and remote transports are out of scope for v0.4.

## Verification

Before opening a review, run:

```bash
cargo fmt --check
cargo check --locked
cargo build --locked
cargo test --locked
```

Before release, also follow `docs/RELEASE_CHECKLIST.md`.
