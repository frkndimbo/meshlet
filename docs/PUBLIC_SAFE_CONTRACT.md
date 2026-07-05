# Public-Safe Contract

Public-safe mode is the local sharing boundary for Meshlet. It exposes compact data derived only from public state and blocks mutation paths that could accidentally leak local context.

## Definition

Public-safe output may include only data with `public` visibility, after compact projection and whitelist redaction.

Public-safe output must not expose:

- private or local events
- raw event payloads outside compact projections
- raw graph attrs
- local file paths or refs
- path-derived file labels or path-derived file ids
- mailbox message bodies
- raw message attrs
- generated runtime state such as `.meshlet/`

## Profiles

`local-trusted` is the default local profile. It may return full local data to the operator.

`public-safe` is the sharing profile. It must use compact outputs and visibility filtering.

`full` output mode is local-trusted only. Public-safe requests for full output are rejected.

## MCP Contract

In `public-safe` MCP mode, these tools are disabled:

- `meshlet_publish_event`
- `meshlet_create_task`
- `meshlet_update_task`
- `meshlet_send_message`
- `meshlet_get_context`

Use `meshlet_get_digest` instead of `meshlet_get_context`.

Read tools and resources must apply profile visibility before returning data:

- `meshlet_query`
- `meshlet_list_skills`
- `meshlet_list_tasks`
- `meshlet_get_task`
- `meshlet_get_mailbox`
- `meshlet_get_timeline`
- `meshlet_get_digest`
- `meshlet://skills`
- `meshlet://events/recent`
- `meshlet://tasks`
- `meshlet://evidence/recent`
- `meshlet://graph/namespaces`
- `meshlet://graph`

All large outputs must respect the limit clamp and include truncation metadata where relevant.

## Export Contract

`meshlet doctor public` must verify the event hash chain and scan stored state for secret-like keys or values before sharing.

`meshlet export public` must write only compact sanitized public state.

`meshlet export public --format okf` must write only the public-safe OKF bundle and must avoid mixing with stale files by writing to an empty or missing directory.

Public export must fail closed when the public doctor fails.

OKF doctor validates bundle shape and local markdown links. It must not execute files or follow network links.

## Evidence And Files

Public-safe evidence projection is whitelist-only.

Allowed evidence fields include public ids, kind, source event id, visibility, valid SHA-256 digest, safe task id, and booleans that indicate whether a path or ref exists.

File nodes use stable hashed public ids and generic labels. They do not reveal local file paths.

Graph endpoints that point at file nodes must use the public-safe file id.

## Mailbox And Tasks

Public-safe task reads replay only public task events.

Public-safe timelines include only public task/message/evidence events.

Public-safe mailbox reads include message envelopes only. Message bodies and raw message attrs are omitted.

## Non-Goals

Public-safe mode is not auth, team sharing, remote identity, cloud sync, or a hosted permission system.

Remote transports and cloud sync require a separate security design before implementation.
