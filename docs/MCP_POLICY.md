# MCP Policy

See `docs/PUBLIC_SAFE_CONTRACT.md` for the public-safe sharing boundary across MCP, CLI, and exports.

## v0.1 Transport

- Support MCP over stdio only.
- Do not add SSE or Streamable HTTP until the local runtime is stable.

## Tools

Active tools:

- `meshlet_publish_event`
- `meshlet_list_skills`
- `meshlet_list_tasks`
- `meshlet_get_task`
- `meshlet_create_task`
- `meshlet_update_task`
- `meshlet_send_message`
- `meshlet_get_mailbox`
- `meshlet_get_timeline`
- `meshlet_query`
- `meshlet_get_digest`
- `meshlet_get_context`

Tool rules:

- Tools must have small input schemas.
- Tools must validate inputs before mutating state.
- Tools must return structured JSON-RPC errors instead of panicking.
- Tools must not read secrets or execute shell/network commands implicitly.
- `meshlet_query` is deterministic local search over events, graph nodes, and graph edges only.
- `meshlet_query` accepts optional `namespace` for graph nodes and edges.
- `meshlet_query` accepts `mode=compact|full`; `full` is rejected in `public-safe`.
- `meshlet_create_task`, `meshlet_update_task`, and `meshlet_send_message` are disabled in `public-safe`.
- `meshlet_list_tasks`, `meshlet_get_task`, `meshlet_get_mailbox`, and `meshlet_get_timeline` must apply profile visibility before returning data.
- Large tool outputs must respect the current limit clamp of 1-100 items.
- MCP must not import graph files in v0.2.
- `meshlet_publish_event` and `meshlet_get_context` are disabled in `public-safe`; use `meshlet_get_digest`.

## Resources

Active resources:

- `meshlet://skills`
- `meshlet://events/recent`
- `meshlet://tasks`
- `meshlet://evidence/recent`
- `meshlet://graph/namespaces`
- `meshlet://graph`

Resource rules:

- Resources return JSON text.
- Large outputs must include limit/truncation metadata before remote transports are added.
- Resource content must not include secrets.
- Public-safe resources must return compact sanitized views only.
