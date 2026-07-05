# Meshlet Install Notes

Meshlet is a local Rust CLI and MCP stdio runtime. It does not require a server, cloud account, database service, or model provider.

## Requirements

- Rust `1.85` or newer.
- A local checkout of this repository.
- SQLite is used through the Rust dependency tree; no external database service is required.

## Build From Source

Portable commands:

```bash
cargo build --locked
cargo run -- init
cargo run -- status
```

Maintainer commands inside this repository:

```bash
rtk cargo build --locked
rtk cargo run -- init
rtk cargo run -- status
```

## Optional Local Install

Install the current checkout as a local binary:

```bash
cargo install --path . --locked
meshlet init
meshlet status
```

This installs whatever is in the checkout. It is not a published binary release.

## Local State

`meshlet init` creates `.meshlet/meshlet.db` under the current project root. This directory is generated runtime state and must stay out of git.

The event log is the source of truth. Contexts, graph nodes and edges, tasks, mailbox messages, evidence, timelines, and search tables are read models that can be rebuilt from events.

## Public-Safe Workflow

Before sharing any public output:

```bash
cargo run -- doctor public
cargo run -- export public --out /tmp/meshlet-public.json
cargo run -- export public --format okf --out /tmp/meshlet-okf
cargo run -- okf doctor /tmp/meshlet-okf
```

Public-safe output is compact and visibility-filtered. Private/local events, raw graph attrs, local file paths, message bodies, and generated runtime state are not public artifacts.

## Verification

Portable gates:

```bash
cargo fmt --check
cargo check --locked
cargo build --locked
cargo test --locked
```

Maintainer gates in this repository:

```bash
rtk cargo fmt --check
rtk cargo check --locked
rtk cargo build --locked
rtk cargo test --locked
```

GitHub Actions is currently blocked before runner startup by an account-level billing-lock state, so local locked gates are the required private-trunk verification path.
