# MCP Policy

## v0.1 Transport

- Support MCP over stdio only.
- Do not add SSE or Streamable HTTP until the local runtime is stable.

## Tools

Active tools:

- `meshlet_publish_event`
- `meshlet_list_skills`
- `meshlet_get_context`

Planned but not active:

- `meshlet_query`

Tool rules:

- Tools must have small input schemas.
- Tools must validate inputs before mutating state.
- Tools must return structured JSON-RPC errors instead of panicking.
- Tools must not read secrets or execute shell/network commands implicitly.

## Resources

Active resources:

- `meshlet://skills`
- `meshlet://events/recent`
- `meshlet://graph`

Resource rules:

- Resources return JSON text.
- Large outputs need pagination before remote transports are added.
- Resource content must not include secrets.
