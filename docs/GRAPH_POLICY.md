# Graph Policy

See `docs/EVENT_SCHEMA.md` for event payload rules and `docs/PUBLIC_SAFE_CONTRACT.md` for public-safe graph projection rules.

## Source of Truth

The event log is the source of truth. The context graph is a materialized view and must be rebuildable from events.

## Read Models

- `contexts` is a materialized read model built from `context.added` events.
- `tasks` is a materialized read model built from `task.created` and `task.updated` events.
- `mailbox_messages` is a materialized read model built from `agent.message` events.
- Context rows are query surfaces, not source of truth.
- Context, task, and mailbox rebuilds must replay events deterministically and keep visibility from the source event.
- Graph nodes, graph edges, and skills must keep visibility in SQL columns for read/query filtering.
- `attrs_json` visibility is compatibility metadata only, not the primary security or query surface.
- Public-safe read model queries must filter visibility in SQL before `LIMIT`.
- Text search must use compact FTS5 tables joined to base read-model tables.
- FTS rows are not an authorization source; base-table SQL visibility filters remain mandatory.
- Public-safe FTS queries must apply `base.visibility = 'public'` in SQL before `LIMIT`.
- Local-trusted FTS queries must apply `base.visibility IN ('private', 'local', 'public')` in SQL before `LIMIT`.
- Raw `payload_json` and `attrs_json` scans are not core retrieval paths.
- Event search indexes compact fields such as type, actor, label, title, summary, status, name, assignee, from, and to; it must not index raw payload dumps.
- `agent.message` materializes message nodes and mailbox rows; it does not become a context row in v0.4.

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
- Graph nodes and edges materialized from events inherit the source event visibility in SQL columns.
- Public-safe graph reads and exports must include only graph data whose SQL visibility is `public`.
- Unknown imported relations materialize as `references` edges while preserving the original `relation` attr.
- OKF export may project public graph edges into markdown links, but the typed graph remains the source for relation kind and visibility.

## Graphify Policy

- Graphify is optional local tooling for architecture navigation.
- Run Graphify manually after structural source, docs, policy, or MCP changes when a local graph is useful.
- `graphify update .` is the default low-cost code graph refresh.
- After manual updates, run clustering/report refresh with `graphify cluster-only . --no-viz --no-label` when report/community data is needed.
- Full semantic extraction can be retried when quota/API backend is available.
- Keep generated `graphify-out/` artifacts out of git.
- Treat graph reports as stale if source/docs changed after the latest graph generation.
