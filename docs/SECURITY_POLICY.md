# Security Policy

## Secrets

- Do not store secrets in event payloads.
- Do not store secrets in graph attrs.
- Do not store secrets in docs, tests, logs, or examples.
- Use placeholder names for sensitive variables.
- Event append rejects payload keys that normalize to sensitive names such as `password`, `secret`, `api_key`, `authorization`, `access_token`, `refresh_token`, or `private_key`.
- Secret rejection errors may name the offending key path, but must never print the value.

## Local State

- `.meshlet/` contains generated runtime state and must not be committed.
- SQLite DBs are local artifacts.
- Event actors must be explicit and non-empty.

## Tool Safety

- MCP tools must validate inputs.
- Skill manifests must declare permissions.
- Shell/network execution is out of scope for v0.1 skill handling.
- Graph imports are CLI-only in v0.2; MCP must not import arbitrary files.
- Evidence attach may hash local files, but stored evidence payloads must not contain secret values.
- Evidence verification reports digest match status and must not print file contents.

## Future Remote Mode

Cloud sync, auth, identities, and remote transports require a separate security design before implementation.
