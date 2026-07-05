# Event Schema

Meshlet stores durable state as append-only events. Read models are derived from those events and are rebuildable.

## Versions

- Event store schema version: `6`.
- Context read-model schema version: `1`.
- Task read-model schema version: `1`.
- Message read-model schema version: `1`.

Do not bump schema versions for documentation-only changes.

## Event Envelope

Every event has:

| Field | Meaning |
|---|---|
| `id` | Event id. |
| `type` | One supported event type. |
| `created_at` | Event timestamp. |
| `actor` | Explicit non-empty event actor. |
| `payload` | JSON object validated by event type. |
| `visibility` | `private`, `local`, or `public`. |
| `hash` | Hash over event content, including visibility. |
| `prev_hash` | Previous event hash for chain verification. |

Default visibility is `private`.

## Event Types

Supported event types:

- `repo.initialized`
- `skill.added`
- `context.added`
- `agent.message`
- `evidence.attached`
- `task.created`
- `task.updated`
- `graph.imported`

Unknown event types are rejected.

## Payload Rules

`context.added` accepts flexible context payloads. It materializes compact context fields from `kind`, `namespace`, `title`, `label`, and `summary` when present.

`task.created` requires `title`; optional fields are `task_id`, `status`, `assignee`, and `note`.

`task.updated` requires `task_id` and at least one update field: `status`, `assignee`, or `note`.

Task statuses are:

- `open`
- `in_progress`
- `blocked`
- `done`
- `canceled`

Allowed transitions:

- `open` to `in_progress`, `blocked`, or `canceled`
- `in_progress` to `blocked`, `done`, or `canceled`
- `blocked` to `in_progress` or `canceled`
- `done` and `canceled` are terminal

`agent.message` requires `from`, `to`, and `summary`; optional fields are `task_id`, `body`, and `reply_to`. If `task_id` is present, the task must exist.

`evidence.attached` requires `path` or `ref`; optional fields include `task_id` and `sha256`.

`graph.imported` requires `source`, `namespace`, `nodes`, and `links`. Each node requires `id`; each link requires `source` and `target`.

`skill.added` is produced by skill registration and is governed by `docs/SKILL_POLICY.md`.

`repo.initialized` is produced by repository initialization.

## Safety Rules

Event append rejects secret-like payload keys such as `password`, `secret`, `api_key`, `authorization`, `access_token`, `refresh_token`, and `private_key`.

Public-safe append also rejects common secret-looking string values such as bearer tokens, private-key blocks, GitHub/Slack token prefixes, and `sk-` token-like values.

Error messages may name the key path, but must not print secret values.

## Read Models

The event log is the source of truth.

Derived read models include:

- `contexts` from `context.added`
- `tasks` from `task.created` and `task.updated`
- `mailbox_messages` from `agent.message`
- graph nodes and edges from supported event types
- FTS tables over compact safe fields

Read models must preserve visibility and apply profile filtering before returning public-safe data.

## Hash Chain

Event hashes include visibility. Editing an event between `private`, `local`, and `public` must fail verification.

v0.4 uses strict visibility-bound hashes. If `meshlet verify` fails on a pre-hardening local DB, treat it as legacy local state and export any needed public data before creating fresh `.meshlet/` state.
