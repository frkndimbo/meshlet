# OKF Policy

## Role

OKF is an export/interoperability format for Meshlet. It is not the runtime source of truth.

## Export Rules

- `meshlet export public --format okf --out <dir>` writes a public-safe markdown bundle.
- The output directory must be missing or empty.
- The bundle includes `index.md`, `log.md`, and public concept documents under `contexts/`, `tasks/`, `messages/`, `skills/`, and `evidence/`.
- Concept documents must have YAML frontmatter with a non-empty `type`.
- Task documents may include a compact public timeline replay.
- Message documents must export compact envelope metadata only; message bodies stay out of OKF export.
- Public graph edges may appear as markdown links in document bodies.
- Evidence documents must include a `# Citations` section when evidence metadata is exported.

## Doctor Rules

- `meshlet okf doctor <dir>` checks markdown frontmatter and local markdown links.
- Missing concept frontmatter and empty `type` fields are errors.
- Broken local markdown links are warnings.
- Unknown frontmatter fields and unknown OKF concept types are allowed.
- External links and anchors are ignored.

## Boundaries

- OKF import is out of scope for v0.4.
- MCP must not import OKF bundles.
- OKF export must not replace the event log, SQLite store, typed graph, MCP tools, or skill TOML manifests.
