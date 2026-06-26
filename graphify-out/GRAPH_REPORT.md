# Graph Report - .  (2026-06-26)

## Corpus Check
- cluster-only mode — file stats not available

## Summary
- 219 nodes · 566 edges · 16 communities
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 4 edges (avg confidence: 0.83)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `cd6c8394`
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

## God Nodes (most connected - your core abstractions)
1. `Result` - 86 edges
2. `Meshlet` - 50 edges
3. `Value` - 45 edges
4. `Event` - 20 edges
5. `Option` - 16 edges
6. `mcp_request()` - 16 edges
7. `Meshlet New Chat Handoff` - 16 edges
8. `Vec` - 12 edges
9. `mcp_tool_call()` - 12 edges
10. `clamp_limit()` - 12 edges

## Surprising Connections (you probably didn't know these)
- `MCP Stdio Server` --calls--> `Meshlet Core`  [INFERRED]
  docs/MCP_POLICY.md → README.md
- `main()` --calls--> `run_mcp_stdio()`  [INFERRED]
  src/main.rs → src/lib.rs
- `Meshlet Core` --conceptually_related_to--> `Ponytail Simplicity Check`  [EXTRACTED]
  README.md → AGENTS.md
- `main()` --calls--> `find_project_root()`  [INFERRED]
  src/main.rs → src/lib.rs
- `main()` --calls--> `parse_json_arg()`  [INFERRED]
  src/main.rs → src/lib.rs

## Import Cycles
- 1-file cycle: `src/lib.rs -> src/lib.rs`
- 1-file cycle: `src/main.rs -> src/main.rs`

## Hyperedges (group relationships)
- **Meshlet v0.1 Core Architecture** — meshlet_event_log, meshlet_context_graph, meshlet_skill_registry, meshlet_mcp_server [EXTRACTED 1.00]

## Communities (16 total, 0 thin omitted)

### Community 0 - "Community 0"
Cohesion: 0.12
Nodes (44): AsRef, Path, Row, Self, append_event_accepts_safe_payload_keys(), append_event_chains_hashes(), append_event_rejects_secret_key_names(), canonical_json() (+36 more)

### Community 1 - "Community 1"
Cohesion: 0.13
Nodes (18): Map, Bounded, clamp_limit(), handle_mcp_request(), json_rpc_error(), limit_arg(), mcp_read_resource(), mcp_resources() (+10 more)

### Community 2 - "Community 2"
Cohesion: 0.09
Nodes (21): Batasan Penting, Contoh CLI Target, Event Awal, Filosofi Produk, Inti Ide, Kenapa Ini Relevan, Keputusan Saat Ini, Later (+13 more)

### Community 3 - "Community 3"
Cohesion: 0.28
Nodes (3): Connection, Event, Meshlet

### Community 4 - "Community 4"
Cohesion: 0.16
Nodes (15): Command, PathBuf, Serialize, find_project_root(), parse_json_arg(), Cli, Command, EventCommand (+7 more)

### Community 5 - "Community 5"
Cohesion: 0.17
Nodes (11): Agent Instructions, Architecture Rules, Commands, Git, Graphify, Optimization, Product Scope, Progressive Docs (+3 more)

### Community 6 - "Community 6"
Cohesion: 0.31
Nodes (9): Option, find_secret_key(), nonempty_string(), normalize_key(), required_nonempty_string(), SkillManifest, validate_event_payload(), VerificationReport (+1 more)

### Community 7 - "Community 7"
Cohesion: 0.22
Nodes (8): Commands, Core Flow, Current Features, Docs, Meshlet, Not Doing Yet, v0.1 Scope, Verification

### Community 8 - "Community 8"
Cohesion: 0.25
Nodes (7): Current Phase, Current State, Deprecated / Superseded, Last Verified, Maintenance Rules, Meshlet Progress, Next 3 Tasks

### Community 9 - "Community 9"
Cohesion: 0.40
Nodes (5): Meshlet CLI, Meshlet Core, MCP Stdio Server, SQLite Backend, Ponytail Simplicity Check

### Community 10 - "Community 10"
Cohesion: 0.33
Nodes (5): Explicitly Out of Scope for v0.1, Meshlet Scope, Phase Gate, Product Line, v0.1 Active Scope

### Community 11 - "Community 11"
Cohesion: 0.67
Nodes (3): Context Graph, Event Log, Skill Registry

### Community 12 - "Community 12"
Cohesion: 0.33
Nodes (5): Future Remote Mode, Local State, Secrets, Security Policy, Tool Safety

### Community 13 - "Community 13"
Cohesion: 0.40
Nodes (4): Graph Policy, Graphify Policy, Node and Edge Rules, Source of Truth

### Community 14 - "Community 14"
Cohesion: 0.40
Nodes (4): MCP Policy, Resources, Tools, v0.1 Transport

### Community 15 - "Community 15"
Cohesion: 0.40
Nodes (4): Manifest, Permissions, Registry Rules, Skill Policy

## Knowledge Gaps
- **77 isolated node(s):** `Connection`, `T`, `Write`, `Map`, `Command` (+72 more)
  These have ≤1 connection - possible missing edges or undocumented components.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Result` connect `Community 0` to `Community 1`, `Community 3`, `Community 4`, `Community 6`?**
  _High betweenness centrality (0.123) - this node is a cross-community bridge._
- **Why does `Meshlet` connect `Community 3` to `Community 0`, `Community 1`, `Community 4`, `Community 6`?**
  _High betweenness centrality (0.068) - this node is a cross-community bridge._
- **Why does `PathBuf` connect `Community 4` to `Community 3`?**
  _High betweenness centrality (0.031) - this node is a cross-community bridge._
- **What connects `Connection`, `T`, `Write` to the rest of the system?**
  _77 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Community 0` be split into smaller, more focused modules?**
  _Cohesion score 0.12089447938504543 - nodes in this community are weakly interconnected._
- **Should `Community 1` be split into smaller, more focused modules?**
  _Cohesion score 0.13333333333333333 - nodes in this community are weakly interconnected._
- **Should `Community 2` be split into smaller, more focused modules?**
  _Cohesion score 0.09090909090909091 - nodes in this community are weakly interconnected._