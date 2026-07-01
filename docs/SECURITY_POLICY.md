# Security Policy

## Secrets

- Do not store secrets in event payloads.
- Do not store secrets in graph attrs.
- Do not store secrets in docs, tests, logs, or examples.
- Use placeholder names for sensitive variables.
- Event append rejects payload keys that normalize to sensitive names such as `password`, `secret`, `api_key`, `authorization`, `access_token`, `refresh_token`, or `private_key`.
- `public-safe` event append also rejects secret-looking string values such as bearer tokens, private-key blocks, common GitHub/Slack token prefixes, and `sk-` token-like values.
- Secret rejection errors may name the offending key path, but must never print the value.

## Visibility

- Event visibility is `private`, `local`, or `public`.
- Default visibility is `private`.
- Public-safe query, digest, and export paths must only return public events and graph data derived from public events.
- Public-safe OKF export must only write public contexts, tasks, message envelopes, skills, evidence, graph links, timeline entries, and event-log entries.
- Public-safe task and timeline reads must replay only public task/message/evidence events.
- Public-safe mailbox reads must return only public messages.
- Public export must omit message bodies and raw message attrs.
- Full/raw output mode is local-trusted only.

## Local State

- `.meshlet/` contains generated runtime state and must not be committed.
- SQLite DBs are local artifacts.
- Event actors must be explicit and non-empty.

## Tool Safety

- MCP tools must validate inputs.
- MCP task/message mutation tools must be disabled in `public-safe`.
- Skill manifests must declare permissions.
- Shell/network execution is out of scope for v0.1 skill handling.
- Graph imports are CLI-only in v0.2; MCP must not import arbitrary files.
- Evidence attach may hash local files, but stored evidence payloads must not contain secret values.
- Evidence verification reports digest match status and must not print file contents.
- `meshlet doctor public` must fail when the local DB contains secret-looking stored state.
- `meshlet export public` must write only sanitized compact public state.
- `meshlet export public --format okf` must write to an empty or missing directory to avoid stale public bundles.
- OKF doctor validates bundle shape and links only; it must not execute files or follow network links.

## Future Remote Mode

Cloud sync, auth, identities, and remote transports require a separate security design before implementation.
