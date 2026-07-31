# Meshlet

Meshlet is a verifiable event log and coordination substrate for multi-agent systems. Instead of giving one agent a fuzzy memory of past chats, it gives a system of agents a durable, hash-chained record of what happened, who did it, and what's safe to share outside the local machine — with no vector database, embedding model, or cloud service involved.

`v0.4` focuses on a public-safe local runtime: SQLite local storage, visibility-aware event log, compact public exports, OKF markdown projections, and local agent mailboxes.

---

## Why Not Just Use a Memory MCP Server?


Most MCP memory servers solve one problem: help a single agent recall facts
across sessions, usually through vector embeddings and semantic search.
Meshlet solves a different problem: give a *system of agents* a durable,
tamper-evident record of what happened, who did it, and what's safe to expose
outside the local machine.

There is no vector database and no embedding model here, by design. Every
event is appended to a hash-chained log, so `meshlet verify` can prove the
history hasn't been altered — something semantic-recall memory stores aren't
built to do. Graph, task, mailbox, and timeline views are just materialized
read models over that log, and can be rebuilt at any time.

| | Typical memory MCP server | meshlet |
|---|---|---|
| Core primitive | Vector embeddings + semantic search | Append-only event log + hash chain |
| Answers | "What do I know about X?" | "What happened, in what order, can I prove it?" |
| Storage | Vector DB (Qdrant, LanceDB, etc.) | SQLite only |
| Multi-agent coordination | Rare, usually bolted on | Core primitive (typed tasks, mailbox) |
| Data governance | Often all-or-nothing | Per-event `private` / `local` / `public` visibility |
| External sharing | Not typically a design goal | `doctor public` + sanitized export built in |
| Permissions | Rare | Skill manifests with declared permissions |
| Integrity guarantee | None typical | Hash-chain verification (`verify`) |

If you want an agent that remembers your coding preferences, a vector-memory
MCP server is probably the right tool. If you want multiple agents
coordinating through typed tasks and mailbox messages, with an audit trail
you can verify and a clear boundary between private, local, and public data,
meshlet is built for that.

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

**Who this is for:** teams or solo builders running more than one agent
against the same project, who need to know — later, and provably — exactly
what each agent did and why. Not for building a single chatty assistant with
a good long-term memory; plenty of tools already do that well.

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
