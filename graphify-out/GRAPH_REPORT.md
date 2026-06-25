# Graph Report - .  (2026-06-25)

## Corpus Check
- cluster-only mode — file stats not available

## Summary
- 163 nodes · 347 edges · 13 communities
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 3 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `bc5623f2`
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

## God Nodes (most connected - your core abstractions)
1. `Result` - 51 edges
2. `Meshlet` - 34 edges
3. `Value` - 25 edges
4. `Event` - 16 edges
5. `Meshlet New Chat Handoff` - 16 edges
6. `mcp_request()` - 12 edges
7. `Agent Instructions` - 10 edges
8. `Option` - 8 edges
9. `handle_mcp_request()` - 8 edges
10. `mcp_tool_call()` - 8 edges

## Surprising Connections (you probably didn't know these)
- `main()` --calls--> `run_mcp_stdio()`  [INFERRED]
  src/main.rs → src/lib.rs
- `main()` --calls--> `find_project_root()`  [INFERRED]
  src/main.rs → src/lib.rs
- `main()` --calls--> `parse_json_arg()`  [INFERRED]
  src/main.rs → src/lib.rs

## Import Cycles
- 1-file cycle: `src/lib.rs -> src/lib.rs`
- 1-file cycle: `src/main.rs -> src/main.rs`

## Communities (13 total, 0 thin omitted)

### Community 0 - "Community 0"
Cohesion: 0.17
Nodes (25): AsRef, Path, Row, Self, append_event_chains_hashes(), edge_from_row(), event_from_row(), graph_rebuild_is_deterministic() (+17 more)

### Community 1 - "Community 1"
Cohesion: 0.19
Nodes (9): Connection, Option, canonical_json(), Event, event_hash(), Meshlet, SkillManifest, String (+1 more)

### Community 2 - "Community 2"
Cohesion: 0.09
Nodes (21): Batasan Penting, Contoh CLI Target, Event Awal, Filosofi Produk, Inti Ide, Kenapa Ini Relevan, Keputusan Saat Ini, Later (+13 more)

### Community 3 - "Community 3"
Cohesion: 0.19
Nodes (13): Command, PathBuf, Serialize, find_project_root(), parse_json_arg(), Cli, Command, EventCommand (+5 more)

### Community 4 - "Community 4"
Cohesion: 0.24
Nodes (11): handle_mcp_request(), json_rpc_error(), mcp_read_resource(), mcp_resources(), mcp_tool_call(), mcp_tools(), run_mcp_stdio(), str_field() (+3 more)

### Community 5 - "Community 5"
Cohesion: 0.18
Nodes (10): Agent Instructions, Architecture Rules, Commands, Git, Graphify, graphify, Product Scope, Progressive Docs (+2 more)

### Community 6 - "Community 6"
Cohesion: 0.22
Nodes (8): Commands, Core Flow, Current Features, Docs, Meshlet, Not Doing Yet, v0.1 Scope, Verification

### Community 7 - "Community 7"
Cohesion: 0.25
Nodes (7): Current Phase, Current State, Deprecated / Superseded, Last Verified, Maintenance Rules, Meshlet Progress, Next 3 Tasks

### Community 8 - "Community 8"
Cohesion: 0.33
Nodes (5): Explicitly Out of Scope for v0.1, Meshlet Scope, Phase Gate, Product Line, v0.1 Active Scope

### Community 9 - "Community 9"
Cohesion: 0.33
Nodes (5): Future Remote Mode, Local State, Secrets, Security Policy, Tool Safety

### Community 10 - "Community 10"
Cohesion: 0.40
Nodes (4): Graph Policy, Graphify Policy, Node and Edge Rules, Source of Truth

### Community 11 - "Community 11"
Cohesion: 0.40
Nodes (4): MCP Policy, Resources, Tools, v0.1 Transport

### Community 12 - "Community 12"
Cohesion: 0.40
Nodes (4): Manifest, Permissions, Registry Rules, Skill Policy

## Knowledge Gaps
- **66 isolated node(s):** `Connection`, `Write`, `Command`, `Command`, `EventCommand` (+61 more)
  These have ≤1 connection - possible missing edges or undocumented components.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Result` connect `Community 0` to `Community 1`, `Community 3`, `Community 4`?**
  _High betweenness centrality (0.087) - this node is a cross-community bridge._
- **Why does `Meshlet` connect `Community 1` to `Community 0`, `Community 3`, `Community 4`?**
  _High betweenness centrality (0.063) - this node is a cross-community bridge._
- **Why does `main()` connect `Community 3` to `Community 4`?**
  _High betweenness centrality (0.029) - this node is a cross-community bridge._
- **What connects `Connection`, `Write`, `Command` to the rest of the system?**
  _66 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Community 2` be split into smaller, more focused modules?**
  _Cohesion score 0.09090909090909091 - nodes in this community are weakly interconnected._