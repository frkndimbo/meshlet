# Meshlet

Meshlet is a local-first context mesh for agent workflows. It provides agents with a durable event log, a rebuildable context graph, a skill registry, and an MCP stdio surface—without adding cloud dependencies, external dashboards, marketplaces, vector databases, or hosted AI models.

`v0.4` focuses on a public-safe local runtime: SQLite local storage, visibility-aware event log, compact public exports, OKF markdown projections, and local agent mailboxes.

---

## Why It Exists

Agent sessions are frequently transient, but valuable engineering context is not. Meshlet retains structured working context in a local, inspectable, append-only system:

```text
Agent -> MCP stdio -> Event Log -> Read Models -> Context Graph -> Public-Safe Views
```

The event log is the sole source of truth. The context graph, task timeline, mailbox, evidence registry, and search indexes are materialized read models that can be rebuilt on demand.

---

## Current Status

- **Package Version**: `0.4.0`
- **Runtime**: Local CLI + MCP stdio server
- **Storage**: SQLite local database (`.meshlet/`)
- **Safety Levels**: `private`, `local`, and `public` event visibilities

---

## Core Features

- **Append-Only Event Log**: Hash-chain verification for tamper-evident event storage.
- **Context Graph**: Materialized graph nodes and edges rebuildable from context events.
- **MCP Stdio Server**: Protocol-compliant JSON-RPC tools and resources for AI coding agents.
- **Skill Registry**: Register and inspect structured skill manifests with declared permissions.
- **Agent Workflows**: Built-in support for agent tasks, mailbox messaging, evidence SHA-256 digests, and task timelines.
- **Public-Safe Sharing & Exports**: Compact public JSON and OKF (Open Knowledge Format) markdown exports.
- **Safety Checks**: Built-in `doctor public` command to verify sanitization before sharing exports.
- **Project Adoption**: Command-line tools (`agent install codex`, `init --adopt`, `refresh`) for local agent environment integration.

---

## Quick Start

### Requirements
- Rust `1.85` or newer (install via [rustup.rs](https://rustup.rs))

### Build & Run

```bash
# Build binary
cargo build --release

# Initialize local Meshlet environment
cargo run -- init

# Check runtime status
cargo run -- status
```

### Append & Verify Events

```bash
# Append a context event
cargo run -- event append --type context.added --json '{"label":"repository architecture"}'

# List events
cargo run -- event list

# Verify hash-chain integrity
cargo run -- verify
```

### Query Context

```bash
# Perform a compact deterministic query
cargo run -- query "architecture" --kind all --limit 20 --mode compact
```

---

## Command Reference

| Command | Purpose |
|---|---|
| `cargo run -- init` | Initialize local `.meshlet/` runtime state |
| `cargo run -- init --adopt` | Adopt repo with local config, OKF skeleton, and ignore rules |
| `cargo run -- status` | Show local Meshlet status and database metrics |
| `cargo run -- verify` | Verify event hash-chain integrity |
| `cargo run -- refresh --graphify --okf` | Refresh Graphify import and OKF markdown projections |
| `cargo run -- agent install codex --patch` | Patch local `.codex/config.toml` with Meshlet MCP stdio configuration |
| `cargo run -- agent show codex` | Show detected Meshlet & Codex environment status |
| `cargo run -- event list` | List recorded events |
| `cargo run -- query "term" --kind all --limit 20 --mode compact` | Search local context models |
| `cargo run -- graph import <graph.json> --source graphify --namespace graphify:repo` | Import a namespaced Graphify graph |
| `cargo run -- skill add ./skill.toml` | Register a skill manifest |
| `cargo run -- task create --task-id task-1 --title "Context refactor"` | Create a typed task event |
| `cargo run -- mailbox send --from agent:a --to agent:b --summary "Task ready" --task-id task-1` | Send an agent mailbox message |
| `cargo run -- evidence attach --path src/lib.rs --sha256 auto` | Attach evidence with SHA-256 digest |
| `cargo run -- evidence retrieve <sha256>` | Retrieve evidence metadata by hash |
| `cargo run -- doctor public` | Check whether public sharing is safe |
| `cargo run -- export public --out /tmp/meshlet-public.json` | Write a compact public JSON bundle |
| `cargo run -- export public --format okf --out /tmp/meshlet-okf` | Write a public-safe OKF markdown bundle |
| `cargo run -- serve --mcp stdio --profile public-safe` | Start MCP stdio server in public-safe mode |

---

## Public-Safe Sharing & Security

Meshlet is designed to be local-first while allowing safe, sanitized sharing when needed:
- Public-safe export commands reject private/local payload keys and sensitive attributes.
- In `public-safe` MCP mode, mutation tools are blocked automatically.
- Local database state (`.meshlet/`), SQLite files, private evidence, and exports are ignored by `.gitignore` and should never be committed to source control.

Before publishing or sharing exports, run:

```bash
cargo run -- doctor public
cargo run -- export public --out /tmp/meshlet-public.json
```

---

## Development & Verification

Run standard verification commands before pushing or releasing:

```bash
cargo fmt --check
cargo check --locked
cargo build --locked
cargo test --locked
```

---

## License

[MIT](LICENSE)
