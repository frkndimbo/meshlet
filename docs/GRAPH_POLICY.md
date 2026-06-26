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
- `imported`

Current edge kinds:

- `references`
- `produced`
- `supports`
- `depends_on`
- `uses`
- `derived_from`
- `updates`

New node or edge kinds require a matching update to this file and tests for rebuild behavior.

## Import Rules

- Imported graph data must enter through a `graph.imported` event.
- Graph imports are materialized views and must rebuild from the event log.
- Imported node IDs use `<namespace>:<external_id>`.
- Imported graph attrs must include `namespace`, `source`, and `external_id` for nodes.
- Unknown imported relations materialize as `references` edges while preserving the original `relation` attr.

## Graphify Policy

- Graphify is installed for Codex via AGENTS.md and `.codex/hooks.json`.
- Run Graphify manually after structural source, docs, policy, or MCP changes when hooks do not update the graph.
- `graphify update .` is the default low-cost code graph refresh.
- After manual updates, run clustering/report refresh with `graphify cluster-only . --no-viz --no-label` when report/community data is needed.
- Full semantic extraction can be retried when quota/API backend is available.
- If `graphify-out/graph.json` exists, answer architecture questions from graph first.
- Treat graph reports as stale if source/docs changed after the latest graph generation.
- Keep cache and temp sidecars ignored.
