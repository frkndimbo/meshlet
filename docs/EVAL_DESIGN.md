# Eval Design: v0.4.1 `init --adopt`

## Verification

- Run `rtk cargo fmt --check`.
- Run `rtk cargo check --locked`.
- Run `rtk cargo test --locked`.
- Inspect `rtk git diff --stat` and targeted diffs for CLI, config, and docs changes.

## Risk Focus

- Plain `meshlet init` must preserve its previous JSON and side effects.
- Adopt JSON must keep stable fields for `state`, `config`, `okf`, `created`, `patched`, `already_present`, and `next`.
- Adoption file patches must be idempotent and avoid duplicate `.gitignore` or `AGENTS.md` entries.
- Adoption must not ignore `.meshlet-okf/` by default.
- Adoption must not modify existing `meshlet.toml` content.
