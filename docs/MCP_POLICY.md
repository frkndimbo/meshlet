# MCP Policy

## v0.1 Transport

- Support MCP over stdio only.
- Do not add SSE or Streamable HTTP until the local runtime is stable.

## Tools

Active tools:

- `meshlet_publish_event`
- `meshlet_list_skills`
- `meshlet_list_tasks`
- `meshlet_get_task`
- `meshlet_query`
- `meshlet_get_context`

Tool rules:

- Tools must have small input schemas.
- Tools must validate inputs before mutating state.
- Tools must return structured JSON-RPC errors instead of panicking.
- Tools must not read secrets or execute shell/network commands implicitly.
- `meshlet_query` is deterministic local search over events, graph nodes, and graph edges only.
- `meshlet_query` accepts optional `namespace` for graph nodes and edges.
- Large tool outputs must respect the current limit clamp of 1-100 items.
- MCP must not import graph files in v0.2.

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
