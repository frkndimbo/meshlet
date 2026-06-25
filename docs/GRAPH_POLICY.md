# Graph Policy

## Source of Truth

The event log is the source of truth. The context graph is a materialized view and must be rebuildable from events.

## Node and Edge Rules

Current node kinds:

- `repo`
- `skill`
- `file`
- `context`
- `agent`
- `message`
- `evidence`
- `task`

Current edge kinds:

- `references`
- `produced`

New node or edge kinds require a matching update to this file and tests for rebuild behavior.

## Graphify Policy

- Graphify is installed for Codex via AGENTS.md and `.codex/hooks.json`.
- Run Graphify manually after structural source, docs, policy, or MCP changes when hooks do not update the graph.
- `graphify update .` is the default low-cost code graph refresh.
- Full semantic extraction can be retried when quota/API backend is available.
- If `graphify-out/graph.json` exists, answer architecture questions from graph first.
- Treat graph reports as stale if source/docs changed after the latest graph generation.
- Keep cache and temp sidecars ignored.
