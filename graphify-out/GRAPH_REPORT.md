# Graph Report - .  (2026-06-26)

## Corpus Check
- cluster-only mode — file stats not available

## Summary
- 241 nodes · 633 edges · 21 communities (20 shown, 1 thin omitted)
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 6 edges (avg confidence: 0.82)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `a175b3b3`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- [[_COMMUNITY_Community 0|Community 0]]
- [[_COMMUNITY_Community 1|Community 1]]
- [[_COMMUNITY_Community 2|Community 2]]
- [[_COMMUNITY_Community 3|Community 3]]
- [[_COMMUNITY_Community 4|Community 4]]
- [[_COMMUNITY_Community 5|Community 5]]
- [[_COMMUNITY_Community 6|Community 6]]
- [[_COMMUNITY_Community 7|Community 7]]
- [[_COMMUNITY_Community 8|Community 8]]
- [[_COMMUNITY_Community 9|Community 9]]
- [[_COMMUNITY_Community 10|Community 10]]
- [[_COMMUNITY_Community 11|Community 11]]
- [[_COMMUNITY_Community 12|Community 12]]
- [[_COMMUNITY_Community 13|Community 13]]
- [[_COMMUNITY_Community 14|Community 14]]
- [[_COMMUNITY_Community 15|Community 15]]
- [[_COMMUNITY_Community 16|Community 16]]
- [[_COMMUNITY_Community 17|Community 17]]
- [[_COMMUNITY_Community 18|Community 18]]
- [[_COMMUNITY_Community 19|Community 19]]
- [[_COMMUNITY_Community 20|Community 20]]

## God Nodes (most connected - your core abstractions)
1. `Result` - 93 edges
2. `Meshlet` - 50 edges
3. `Value` - 39 edges
4. `Event` - 21 edges
5. `Option` - 19 edges
6. `mcp_request()` - 16 edges
7. `Meshlet New Chat Handoff` - 16 edges
8. `String` - 13 edges
9. `Vec` - 13 edges
10. `clamp_limit()` - 12 edges

## Surprising Connections (you probably didn't know these)
- `MCP Stdio Server` --calls--> `Meshlet Core`  [INFERRED]
  docs/MCP_POLICY.md → README.md
- `mcp_tool_call()` --calls--> `str_field()`  [INFERRED]
  src/mcp.rs → src/lib.rs
- `mcp_request()` --calls--> `handle_mcp_request()`  [INFERRED]
  src/lib.rs → src/mcp.rs
- `main()` --calls--> `run_mcp_stdio()`  [INFERRED]
  src/main.rs → src/mcp.rs
- `Meshlet Core` --conceptually_related_to--> `Ponytail Simplicity Check`  [EXTRACTED]
  README.md → AGENTS.md

## Import Cycles
- 1-file cycle: `src/lib.rs -> src/lib.rs`
- 1-file cycle: `src/main.rs -> src/main.rs`

## Hyperedges (group relationships)
- **Meshlet v0.1 Core Architecture** — meshlet_event_log, meshlet_context_graph, meshlet_skill_registry, meshlet_mcp_server [EXTRACTED 1.00]

## Communities (21 total, 1 thin omitted)

### Community 0 - "Community 0"
Cohesion: 0.18
Nodes (11): Connection, Option, Bounded, clamp_limit(), Meshlet, nonempty_string(), required_nonempty_string(), Value (+3 more)

### Community 1 - "Community 1"
Cohesion: 0.18
Nodes (18): append_event_accepts_safe_payload_keys(), append_event_chains_hashes(), append_event_rejects_secret_key_names(), canonical_json(), event_hash(), graph_import_event_accepts_valid_payload(), graph_import_materializes_namespaced_nodes_and_edges(), graph_namespaces_lists_imported_namespaces() (+10 more)

### Community 2 - "Community 2"
Cohesion: 0.09
Nodes (21): Batasan Penting, Contoh CLI Target, Event Awal, Filosofi Produk, Inti Ide, Kenapa Ini Relevan, Keputusan Saat Ini, Later (+13 more)

### Community 3 - "Community 3"
Cohesion: 0.21
Nodes (9): Row, edge_from_row(), Event, event_from_row(), event_record_from_row(), EventRecord, graph_import_edge_kind(), node_from_row() (+1 more)

### Community 4 - "Community 4"
Cohesion: 0.25
Nodes (15): mcp_content_text(), mcp_get_context_respects_limit(), mcp_get_task_returns_json_rpc_error_for_missing_task(), mcp_initialize_advertises_server_capabilities(), mcp_list_skills_returns_registered_skill(), mcp_publish_event_appends_event_and_returns_event_details(), mcp_publish_event_rejects_missing_required_params(), mcp_query_returns_search_results() (+7 more)

### Community 5 - "Community 5"
Cohesion: 0.16
Nodes (15): Command, PathBuf, Serialize, find_project_root(), parse_json_arg(), Cli, Command, EventCommand (+7 more)

### Community 6 - "Community 6"
Cohesion: 0.18
Nodes (10): AsRef, Path, Self, graph_import_event_rejects_invalid_payload(), graph_import_file_appends_event_with_digest_and_counts(), init_creates_repo_event(), query_rejects_empty_or_unknown_kind(), sha256_hex() (+2 more)

### Community 7 - "Community 7"
Cohesion: 0.34
Nodes (14): Meshlet, clamp_limit(), handle_mcp_request(), json_rpc_error(), limit_arg(), mcp_read_resource(), mcp_resources(), mcp_tool_call() (+6 more)

### Community 8 - "Community 8"
Cohesion: 0.17
Nodes (11): Agent Instructions, Architecture Rules, Commands, Git, Graphify, Optimization, Product Scope, Progressive Docs (+3 more)

### Community 9 - "Community 9"
Cohesion: 0.40
Nodes (5): Meshlet CLI, Meshlet Core, MCP Stdio Server, SQLite Backend, Ponytail Simplicity Check

### Community 10 - "Community 10"
Cohesion: 0.22
Nodes (8): Commands, Core Flow, Current Features, Docs, Meshlet, Not Doing Yet, v0.1 Scope, Verification

### Community 11 - "Community 11"
Cohesion: 0.67
Nodes (3): Context Graph, Event Log, Skill Registry

### Community 12 - "Community 12"
Cohesion: 0.25
Nodes (7): Current Phase, Current State, Deprecated / Superseded, Last Verified, Maintenance Rules, Meshlet Progress, Next 3 Tasks

### Community 13 - "Community 13"
Cohesion: 0.29
Nodes (8): Map, find_secret_key(), imported_node_id(), merge_task_payload(), namespace_json_fragment(), normalize_key(), VerificationReport, String

### Community 14 - "Community 14"
Cohesion: 0.33
Nodes (5): Explicitly Out of Scope for v0.1, Meshlet Scope, Phase Gate, Product Line, v0.1 Active Scope

### Community 15 - "Community 15"
Cohesion: 0.33
Nodes (5): Future Remote Mode, Local State, Secrets, Security Policy, Tool Safety

### Community 16 - "Community 16"
Cohesion: 0.40
Nodes (4): Graph Policy, Graphify Policy, Node and Edge Rules, Source of Truth

### Community 17 - "Community 17"
Cohesion: 0.40
Nodes (4): MCP Policy, Resources, Tools, v0.1 Transport

### Community 18 - "Community 18"
Cohesion: 0.40
Nodes (4): Manifest, Permissions, Registry Rules, Skill Policy

### Community 19 - "Community 19"
Cohesion: 0.40
Nodes (4): query_finds_events_nodes_and_edges(), skill_manifest_roundtrip_materializes_skill_and_graph(), SkillManifest, validate_skill_manifest()

## Knowledge Gaps
- **78 isolated node(s):** `Connection`, `T`, `Map`, `Command`, `Command` (+73 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **1 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Result` connect `Community 1` to `Community 0`, `Community 3`, `Community 4`, `Community 5`, `Community 6`, `Community 13`, `Community 19`?**
  _High betweenness centrality (0.132) - this node is a cross-community bridge._
- **Why does `Meshlet` connect `Community 0` to `Community 1`, `Community 3`, `Community 4`, `Community 5`, `Community 6`, `Community 19`?**
  _High betweenness centrality (0.067) - this node is a cross-community bridge._
- **Why does `mcp_request()` connect `Community 4` to `Community 0`, `Community 7`?**
  _High betweenness centrality (0.033) - this node is a cross-community bridge._
- **What connects `Connection`, `T`, `Map` to the rest of the system?**
  _78 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Community 2` be split into smaller, more focused modules?**
  _Cohesion score 0.09090909090909091 - nodes in this community are weakly interconnected._