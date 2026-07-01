# Graph Report - Meshlet  (2026-07-02)

## Corpus Check
- 15 files · ~23,580 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 404 nodes · 1415 edges · 30 communities (22 shown, 8 thin omitted)
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 10 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `05e953f9`
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
- [[_COMMUNITY_Community 21|Community 21]]
- [[_COMMUNITY_Community 22|Community 22]]
- [[_COMMUNITY_Community 23|Community 23]]
- [[_COMMUNITY_Community 29|Community 29]]
- [[_COMMUNITY_Community 30|Community 30]]
- [[_COMMUNITY_Community 31|Community 31]]
- [[_COMMUNITY_Community 32|Community 32]]
- [[_COMMUNITY_Community 33|Community 33]]
- [[_COMMUNITY_Community 34|Community 34]]

## God Nodes (most connected - your core abstractions)
1. `Meshlet` - 109 edges
2. `SafetyProfile` - 42 edges
3. `Event` - 33 edges
4. `clamp_limit()` - 20 edges
5. `mcp_request()` - 19 edges
6. `EventVisibility` - 16 edges
7. `Meshlet New Chat Handoff` - 16 edges
8. `Bounded` - 14 edges
9. `visibility_clause()` - 13 edges
10. `handle_mcp_request_with_profile()` - 11 edges

## Surprising Connections (you probably didn't know these)
- `mcp_tool_call()` --calls--> `str_field()`  [INFERRED]
  src/mcp.rs → src/lib.rs
- `mcp_request()` --calls--> `handle_mcp_request()`  [INFERRED]
  src/lib.rs → src/mcp.rs
- `mcp_public_safe_task_reads_exclude_private_tasks()` --calls--> `handle_mcp_request_with_profile()`  [INFERRED]
  src/lib.rs → src/mcp.rs
- `mcp_public_safe_rejects_mutation_and_full_output()` --calls--> `handle_mcp_request_with_profile()`  [INFERRED]
  src/lib.rs → src/mcp.rs
- `main()` --calls--> `run_mcp_stdio_with_profile()`  [INFERRED]
  src/main.rs → src/mcp.rs

## Import Cycles
- None detected.

## Hyperedges (group relationships)
- **Meshlet v0.1 Core Architecture** — meshlet_event_log, meshlet_context_graph, meshlet_skill_registry, meshlet_mcp_server [EXTRACTED 1.00]

## Communities (30 total, 8 thin omitted)

### Community 0 - "Community 0"
Cohesion: 0.16
Nodes (24): context_added_materializes_into_contexts(), contexts_fts_finds_context_by_title_or_summary(), events_fts_finds_event_by_compact_text(), fts_public_safe_filters_visibility_before_limit(), fts_query(), graph_edges_fts_finds_edge_by_label_or_kind(), graph_edges_visibility_column_materializes_from_event_visibility(), graph_nodes_fts_finds_node_by_label() (+16 more)

### Community 1 - "Community 1"
Cohesion: 0.09
Nodes (18): attrs_with_visibility(), compact_event_text(), Event, event_from_row(), event_task_id(), EventVisibility, graph_edge_label(), graph_import_edge_kind() (+10 more)

### Community 2 - "Community 2"
Cohesion: 0.13
Nodes (21): Connection, Map, Bounded, clamp_limit(), compact_edge(), compact_event(), compact_message(), compact_node() (+13 more)

### Community 3 - "Community 3"
Cohesion: 0.11
Nodes (15): BTreeMap, attrs_visibility(), metadata_line(), okf_document_text(), okf_index(), okf_log(), okf_slug(), OkfDocument (+7 more)

### Community 4 - "Community 4"
Cohesion: 0.09
Nodes (22): Batasan Penting, Contoh CLI Target, Event Awal, Filosofi Produk, Inti Ide, Kenapa Ini Relevan, Keputusan Saat Ini, Later (+14 more)

### Community 5 - "Community 5"
Cohesion: 0.06
Nodes (33): AsRef, Default, FromStr, Path, PathBuf, Self, Serialize, collect_markdown_files() (+25 more)

### Community 6 - "Community 6"
Cohesion: 0.38
Nodes (4): evidence_attach_auto_sha256_and_verify_passes(), evidence_verify_fails_for_changed_or_missing_file(), graph_import_file_appends_event_with_digest_and_counts(), sha256_hex()

### Community 7 - "Community 7"
Cohesion: 0.28
Nodes (17): clamp_limit(), handle_mcp_request(), handle_mcp_request_with_profile(), json_rpc_error(), limit_arg(), mcp_read_resource(), mcp_resources(), mcp_tool_call() (+9 more)

### Community 8 - "Community 8"
Cohesion: 0.15
Nodes (19): append_event_accepts_safe_payload_keys(), append_event_chains_hashes(), append_event_rejects_secret_key_names(), contexts_rebuild_is_deterministic(), graph_import_event_accepts_valid_payload(), graph_import_event_rejects_invalid_payload(), graph_import_materializes_namespaced_nodes_and_edges(), graph_namespaces_lists_imported_namespaces() (+11 more)

### Community 10 - "Community 10"
Cohesion: 0.17
Nodes (11): Agent Instructions, Architecture Rules, Commands, Git, Graphify, Optimization, Product Scope, Progressive Docs (+3 more)

### Community 12 - "Community 12"
Cohesion: 0.22
Nodes (8): Commands, Core Flow, Current Features, Docs, Meshlet, Not Doing Yet, v0.4 Scope, Verification

### Community 13 - "Community 13"
Cohesion: 0.25
Nodes (7): Current Phase, Current State, Deprecated / Superseded, Last Verified, Maintenance Rules, Meshlet Progress, Next 3 Tasks

### Community 14 - "Community 14"
Cohesion: 0.29
Nodes (6): Graph Policy, Graphify Policy, Import Rules, Node and Edge Rules, Read Models, Source of Truth

### Community 15 - "Community 15"
Cohesion: 0.29
Nodes (6): Future Remote Mode, Local State, Secrets, Security Policy, Tool Safety, Visibility

### Community 16 - "Community 16"
Cohesion: 0.33
Nodes (5): Boundaries, Doctor Rules, Export Rules, OKF Policy, Role

### Community 17 - "Community 17"
Cohesion: 0.33
Nodes (5): Explicitly Out of Scope for v0.4, Meshlet Scope, Phase Gate, Product Line, v0.4 Active Scope

### Community 18 - "Community 18"
Cohesion: 0.40
Nodes (4): MCP Policy, Resources, Tools, v0.1 Transport

### Community 19 - "Community 19"
Cohesion: 0.40
Nodes (4): Manifest, Permissions, Registry Rules, Skill Policy

### Community 20 - "Community 20"
Cohesion: 0.17
Nodes (19): Row, canonical_json(), context_from_row(), edge_from_row(), event_hash(), event_record_from_row(), EventRecord, message_from_row() (+11 more)

### Community 21 - "Community 21"
Cohesion: 0.17
Nodes (18): mcp_content_text(), mcp_get_context_respects_limit(), mcp_get_task_returns_json_rpc_error_for_missing_task(), mcp_graph_namespaces_resource_returns_namespaces(), mcp_initialize_advertises_server_capabilities(), mcp_public_safe_task_reads_exclude_private_tasks(), mcp_publish_event_appends_event_and_returns_event_details(), mcp_publish_event_rejects_missing_required_params() (+10 more)

### Community 22 - "Community 22"
Cohesion: 0.25
Nodes (5): mcp_list_skills_returns_registered_skill(), migration_from_v3_adds_visibility_columns_and_backfills_safely(), query_finds_events_nodes_and_edges(), skill_manifest_roundtrip_materializes_skill_and_graph(), skills_visibility_column_materializes_from_event_visibility()

### Community 23 - "Community 23"
Cohesion: 0.43
Nodes (4): normalize_key(), RedactionReport, scan_payload_safety(), scan_payload_safety_at()

## Knowledge Gaps
- **84 isolated node(s):** `EventCommand`, `DoctorCommand`, `ExportCommand`, `OkfCommand`, `GraphCommand` (+79 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **8 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Meshlet` connect `Community 2` to `Community 0`, `Community 1`, `Community 3`, `Community 5`, `Community 6`, `Community 7`, `Community 8`, `Community 20`, `Community 21`, `Community 22`, `Community 23`?**
  _High betweenness centrality (0.125) - this node is a cross-community bridge._
- **Why does `SafetyProfile` connect `Community 2` to `Community 0`, `Community 1`, `Community 5`, `Community 7`, `Community 20`, `Community 21`, `Community 23`?**
  _High betweenness centrality (0.014) - this node is a cross-community bridge._
- **Why does `EventVisibility` connect `Community 1` to `Community 0`, `Community 20`, `Community 5`?**
  _High betweenness centrality (0.010) - this node is a cross-community bridge._
- **What connects `EventCommand`, `DoctorCommand`, `ExportCommand` to the rest of the system?**
  _84 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Community 1` be split into smaller, more focused modules?**
  _Cohesion score 0.09387755102040816 - nodes in this community are weakly interconnected._
- **Should `Community 2` be split into smaller, more focused modules?**
  _Cohesion score 0.12655367231638417 - nodes in this community are weakly interconnected._
- **Should `Community 3` be split into smaller, more focused modules?**
  _Cohesion score 0.10666666666666667 - nodes in this community are weakly interconnected._