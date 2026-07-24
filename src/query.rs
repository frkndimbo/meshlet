use super::*;

impl Meshlet {
    pub fn list_contexts_limited(&self, limit: u32, profile: SafetyProfile) -> Result<Vec<Value>> {
        Ok(self.contexts_bounded(None, limit, profile)?.items)
    }

    pub fn search_contexts(&self, q: &str, limit: u32, profile: SafetyProfile) -> Result<Value> {
        if q.trim().is_empty() {
            bail!("query must not be empty");
        }
        let query = fts_query(q)?;
        let bounded = self.contexts_bounded(Some(&query), limit, profile)?;
        Ok(json!({
            "q": q,
            "limit": clamp_limit(limit),
            "contexts": {
                "items": bounded.items,
                "truncated": bounded.truncated,
            },
        }))
    }

    pub(crate) fn contexts_bounded(
        &self,
        query: Option<&str>,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Bounded<Value>> {
        let limit = clamp_limit(limit);
        let visibility = visibility_clause(profile);
        let sql = if query.is_some() {
            format!(
                "SELECT contexts.id, contexts.kind, contexts.namespace, contexts.title, contexts.summary, contexts.visibility, contexts.source_event_id, contexts.created_at, contexts.updated_at, contexts.schema_version, contexts.attrs_json
                 FROM contexts
                 JOIN contexts_fts ON contexts.id = contexts_fts.id
                 WHERE contexts_fts MATCH ?1
                   AND contexts.{visibility}
                 ORDER BY bm25(contexts_fts), contexts.created_at DESC, contexts.id
                 LIMIT ?2"
            )
        } else {
            format!(
                "SELECT id, kind, namespace, title, summary, visibility, source_event_id, created_at, updated_at, schema_version, attrs_json
                 FROM contexts
                 WHERE {visibility}
                 ORDER BY created_at DESC, id
                 LIMIT ?1"
            )
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut contexts = if let Some(query) = query {
            stmt.query_map(params![query, limit + 1], context_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map([limit + 1], context_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let truncated = contexts.len() > limit as usize;
        contexts.truncate(limit as usize);
        Ok(Bounded {
            items: contexts,
            truncated,
        })
    }

    pub fn context_digest_limited(&self, limit: u32, profile: SafetyProfile) -> Result<Value> {
        let limit = clamp_limit(limit);
        let events = self.list_events_bounded_scoped(limit, profile)?;
        let event_items = events.items.iter().map(compact_event).collect::<Vec<_>>();
        let nodes = self.graph_nodes_bounded(None, limit, profile)?;
        let edges = self.graph_edges_bounded(None, limit, profile)?;
        if profile == SafetyProfile::PublicSafe {
            return Ok(json!({
                "profile": "public-safe",
                "limit": limit,
                "counts": {
                    "events": self.event_count_scoped(profile)?,
                    "skills": self.list_skills_scoped(profile)?.len(),
                    "tasks": self.list_tasks_scoped(limit, profile)?.len(),
                    "namespaces": self.graph_namespaces_scoped(profile)?.len(),
                },
                "events_recent": {
                    "items": event_items,
                    "truncated": events.truncated,
                },
                "tasks": self.list_tasks_scoped(limit, profile)?,
                "graph": {
                    "nodes": {
                        "items": nodes.items.into_iter().map(public_digest_node).collect::<Vec<_>>(),
                        "truncated": nodes.truncated,
                    },
                    "edges": {
                        "items": edges.items.into_iter().map(public_digest_edge).collect::<Vec<_>>(),
                        "truncated": edges.truncated,
                    },
                }
            }));
        }
        Ok(json!({
            "root": self.root.display().to_string(),
            "profile": match profile {
                SafetyProfile::LocalTrusted => "local-trusted",
                SafetyProfile::PublicSafe => "public-safe",
            },
            "limit": limit,
            "counts": {
                "events": self.event_count_scoped(profile)?,
                "skills": self.list_skills_scoped(profile)?.len(),
                "tasks": self.list_tasks_scoped(limit, profile)?.len(),
                "namespaces": self.graph_namespaces_scoped(profile)?.len(),
            },
            "events_recent": {
                "items": event_items,
                "truncated": events.truncated,
            },
            "tasks": self.list_tasks_scoped(limit, profile)?,
            "graph": {
                "nodes": {
                    "items": nodes
                        .items
                        .into_iter()
                        .map(|node| compact_node_for_profile(node, profile))
                        .collect::<Vec<_>>(),
                    "truncated": nodes.truncated,
                },
                "edges": {
                    "items": edges
                        .items
                        .into_iter()
                        .map(|edge| compact_edge_for_profile(edge, profile))
                        .collect::<Vec<_>>(),
                    "truncated": edges.truncated,
                },
            }
        }))
    }

    pub fn context_snapshot(&self) -> Result<Value> {
        self.context_snapshot_limited(DEFAULT_LIMIT)
    }

    pub fn context_snapshot_limited(&self, limit: u32) -> Result<Value> {
        let limit = clamp_limit(limit);
        let events = self.list_events_bounded(limit)?;
        let nodes = self.graph_nodes_bounded(None, limit, SafetyProfile::LocalTrusted)?;
        let edges = self.graph_edges_bounded(None, limit, SafetyProfile::LocalTrusted)?;
        Ok(json!({
            "root": self.root.display().to_string(),
            "limit": limit,
            "events_recent": {
                "items": events.items,
                "truncated": events.truncated,
            },
            "skills": self.list_skills()?,
            "tasks": self.list_tasks(limit)?,
            "evidence": self.list_evidence(limit)?,
            "graph": {
                "nodes": {
                    "items": nodes.items,
                    "truncated": nodes.truncated,
                },
                "edges": {
                    "items": edges.items,
                    "truncated": edges.truncated,
                },
            }
        }))
    }

    pub fn query(&self, q: &str, kind: Option<&str>, limit: u32) -> Result<Value> {
        self.query_scoped(q, kind, None, limit)
    }

    pub fn query_scoped(
        &self,
        q: &str,
        kind: Option<&str>,
        namespace: Option<&str>,
        limit: u32,
    ) -> Result<Value> {
        self.query_scoped_with_profile(q, kind, namespace, limit, SafetyProfile::LocalTrusted)
    }

    pub(crate) fn query_scoped_with_profile(
        &self,
        q: &str,
        kind: Option<&str>,
        namespace: Option<&str>,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Value> {
        if q.trim().is_empty() {
            bail!("query must not be empty");
        }
        let query = fts_query(q)?;
        if namespace.is_some_and(|value| value.trim().is_empty()) {
            bail!("namespace must not be empty");
        }
        let kind = kind.unwrap_or("all");
        if !matches!(kind, "all" | "events" | "nodes" | "edges") {
            bail!("query kind must be all, events, nodes, or edges");
        }
        let limit = clamp_limit(limit);
        let events = if matches!(kind, "all" | "events") {
            Some(self.query_events(&query, limit, profile)?)
        } else {
            None
        };
        let nodes = if matches!(kind, "all" | "nodes") {
            Some(self.query_nodes(&query, namespace, limit, profile)?)
        } else {
            None
        };
        let edges = if matches!(kind, "all" | "edges") {
            Some(self.query_edges(&query, namespace, limit, profile)?)
        } else {
            None
        };

        Ok(json!({
            "q": q,
            "kind": kind,
            "namespace": namespace,
            "limit": limit,
            "events": events.map(|bounded| json!({
                "items": bounded.items,
                "truncated": bounded.truncated,
            })),
            "nodes": nodes.map(|bounded| json!({
                "items": bounded.items,
                "truncated": bounded.truncated,
            })),
            "edges": edges.map(|bounded| json!({
                "items": bounded.items,
                "truncated": bounded.truncated,
            })),
        }))
    }

    pub fn query_scoped_view(
        &self,
        q: &str,
        kind: Option<&str>,
        namespace: Option<&str>,
        limit: u32,
        mode: OutputMode,
        profile: SafetyProfile,
    ) -> Result<Value> {
        if profile == SafetyProfile::PublicSafe && mode == OutputMode::Full {
            bail!("full output mode is not allowed in public-safe profile");
        }
        let value = self.query_scoped_with_profile(q, kind, namespace, limit, profile)?;
        if mode == OutputMode::Full && profile == SafetyProfile::LocalTrusted {
            return Ok(value);
        }
        Ok(compact_query(value, profile))
    }

    pub(crate) fn query_events(
        &self,
        query: &str,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Bounded<Event>> {
        let limit = clamp_limit(limit);
        let visibility = visibility_clause(profile);
        let sql = format!(
            "SELECT events.id, events.type, events.created_at, events.actor, events.payload_json, events.visibility, events.hash, events.prev_hash
             FROM events
             JOIN events_fts ON events.id = events_fts.id
             WHERE events_fts MATCH ?1
               AND events.{visibility}
             ORDER BY bm25(events_fts), events.created_at DESC, events.id
             LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut events = stmt
            .query_map(params![query, limit + 1], event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = events.len() > limit as usize;
        events.truncate(limit as usize);
        Ok(Bounded {
            items: events,
            truncated,
        })
    }

    pub(crate) fn query_nodes(
        &self,
        query: &str,
        namespace: Option<&str>,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Bounded<Value>> {
        let limit = clamp_limit(limit);
        let visibility = visibility_clause(profile);
        let sql = if namespace.is_some() {
            format!(
                "SELECT graph_nodes.id, graph_nodes.kind, graph_nodes.label, graph_nodes.attrs_json, graph_nodes.source_event_id, graph_nodes.visibility
                 FROM graph_nodes
                 JOIN graph_nodes_fts ON graph_nodes.id = graph_nodes_fts.id
                 WHERE graph_nodes_fts MATCH ?1
                   AND graph_nodes.{visibility}
                   AND graph_nodes_fts.namespace = ?2
                 ORDER BY bm25(graph_nodes_fts), graph_nodes.id
                 LIMIT ?3"
            )
        } else {
            format!(
                "SELECT graph_nodes.id, graph_nodes.kind, graph_nodes.label, graph_nodes.attrs_json, graph_nodes.source_event_id, graph_nodes.visibility
                 FROM graph_nodes
                 JOIN graph_nodes_fts ON graph_nodes.id = graph_nodes_fts.id
                 WHERE graph_nodes_fts MATCH ?1
                   AND graph_nodes.{visibility}
                 ORDER BY bm25(graph_nodes_fts), graph_nodes.id
                 LIMIT ?2"
            )
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut nodes = if let Some(namespace) = namespace {
            stmt.query_map(params![query, namespace, limit + 1], node_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map(params![query, limit + 1], node_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let truncated = nodes.len() > limit as usize;
        nodes.truncate(limit as usize);
        Ok(Bounded {
            items: nodes,
            truncated,
        })
    }

    pub(crate) fn query_edges(
        &self,
        query: &str,
        namespace: Option<&str>,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Bounded<Value>> {
        let limit = clamp_limit(limit);
        let visibility = visibility_clause(profile);
        let sql = if namespace.is_some() {
            format!(
                "SELECT graph_edges.id, graph_edges.from_id, graph_edges.to_id, graph_edges.kind, graph_edges.attrs_json, graph_edges.source_event_id, graph_edges.visibility
                 FROM graph_edges
                 JOIN graph_edges_fts ON graph_edges.id = graph_edges_fts.id
                 WHERE graph_edges_fts MATCH ?1
                   AND graph_edges.{visibility}
                   AND graph_edges_fts.namespace = ?2
                 ORDER BY bm25(graph_edges_fts), graph_edges.id
                 LIMIT ?3"
            )
        } else {
            format!(
                "SELECT graph_edges.id, graph_edges.from_id, graph_edges.to_id, graph_edges.kind, graph_edges.attrs_json, graph_edges.source_event_id, graph_edges.visibility
                 FROM graph_edges
                 JOIN graph_edges_fts ON graph_edges.id = graph_edges_fts.id
                 WHERE graph_edges_fts MATCH ?1
                   AND graph_edges.{visibility}
                 ORDER BY bm25(graph_edges_fts), graph_edges.id
                 LIMIT ?2"
            )
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut edges = if let Some(namespace) = namespace {
            stmt.query_map(params![query, namespace, limit + 1], edge_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map(params![query, limit + 1], edge_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let truncated = edges.len() > limit as usize;
        edges.truncate(limit as usize);
        Ok(Bounded {
            items: edges,
            truncated,
        })
    }
}

fn public_digest_node(node: Value) -> Value {
    let mut out = serde_json::Map::new();
    insert_public_digest_string(&mut out, &node, "id");
    insert_public_digest_string(&mut out, &node, "kind");
    insert_public_digest_string(&mut out, &node, "label");
    insert_public_digest_string(&mut out, &node, "visibility");
    insert_public_digest_string(&mut out, &node, "source_event_id");
    Value::Object(out)
}

fn public_digest_edge(edge: Value) -> Value {
    let mut out = serde_json::Map::new();
    insert_public_digest_string(&mut out, &edge, "id");
    insert_public_digest_string(&mut out, &edge, "from_id");
    insert_public_digest_string(&mut out, &edge, "to_id");
    insert_public_digest_string(&mut out, &edge, "kind");
    insert_public_digest_string(&mut out, &edge, "visibility");
    insert_public_digest_string(&mut out, &edge, "source_event_id");
    Value::Object(out)
}

fn insert_public_digest_string(out: &mut serde_json::Map<String, Value>, value: &Value, key: &str) {
    let Some(field) = value.get(key).and_then(Value::as_str) else {
        return;
    };
    if !looks_path_like(field) {
        out.insert(key.to_string(), json!(field));
    }
}

fn looks_path_like(value: &str) -> bool {
    value.starts_with('/')
        || value.contains(":/")
        || value.contains(":\\")
        || value
            .as_bytes()
            .get(1)
            .is_some_and(|byte| *byte == b':' && value.as_bytes()[0].is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn skill_payload(name: &str) -> Value {
        json!({
            "name": name,
            "version": "0.1.0",
            "kind": "skill",
            "manifest_path": format!("{name}.toml"),
            "entry": "./SKILL.md",
            "permissions": ["read_repo"],
            "description": format!("{name} skill"),
        })
    }

    #[test]
    fn public_safe_query_returns_compact_public_items_only() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"label": "private needle", "note": "hidden"}),
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"label": "public needle", "note": "visible"}),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let result = meshlet.query_scoped_view(
            "needle",
            Some("events"),
            None,
            20,
            OutputMode::Compact,
            SafetyProfile::PublicSafe,
        )?;
        let items = result["events"]["items"].as_array().expect("items");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["visibility"], "public");
        assert!(items[0].get("payload").is_none());
        Ok(())
    }

    #[test]
    fn context_added_materializes_into_contexts() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let event = meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({
                "kind": "decision",
                "namespace": "repo:meshlet",
                "title": "Use contexts",
                "summary": "Contexts are a read model",
                "label": "ignored fallback"
            }),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let contexts = meshlet.list_contexts_limited(10, SafetyProfile::LocalTrusted)?;

        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0]["id"], format!("context:{}", event.id));
        assert_eq!(contexts[0]["kind"], "decision");
        assert_eq!(contexts[0]["namespace"], "repo:meshlet");
        assert_eq!(contexts[0]["title"], "Use contexts");
        assert_eq!(contexts[0]["summary"], "Contexts are a read model");
        assert_eq!(contexts[0]["visibility"], "public");
        assert_eq!(contexts[0]["source_event_id"], event.id);
        assert_eq!(contexts[0]["created_at"], event.created_at);
        assert_eq!(contexts[0]["updated_at"], event.created_at);
        assert_eq!(contexts[0]["schema_version"], CONTEXT_SCHEMA_VERSION);
        assert_eq!(contexts[0]["attrs"]["label"], "ignored fallback");
        Ok(())
    }

    #[test]
    fn contexts_rebuild_is_deterministic() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event(
            "context.added",
            "agent:test",
            json!({"label": "first", "namespace": "repo"}),
        )?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"summary": "second"}),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        let before = meshlet.list_contexts_limited(20, SafetyProfile::LocalTrusted)?;

        meshlet.conn.execute("DELETE FROM contexts", [])?;
        meshlet.rebuild_graph()?;

        assert_eq!(
            before,
            meshlet.list_contexts_limited(20, SafetyProfile::LocalTrusted)?
        );
        Ok(())
    }

    #[test]
    fn public_safe_context_search_filters_visibility_before_limit() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"label": "needle public"}),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        for index in 0..12 {
            let visibility = if index % 2 == 0 {
                EventVisibility::Private
            } else {
                EventVisibility::Local
            };
            meshlet.append_event_with_options(
                "context.added",
                "agent:test",
                json!({"label": format!("needle hidden {index}")}),
                visibility,
                SafetyProfile::LocalTrusted,
            )?;
        }

        let result = meshlet.search_contexts("needle", 1, SafetyProfile::PublicSafe)?;
        let items = result["contexts"]["items"].as_array().expect("contexts");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["summary"], "needle public");
        assert_eq!(items[0]["visibility"], "public");
        Ok(())
    }

    #[test]
    fn contexts_fts_finds_context_by_title_or_summary() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({
                "kind": "decision",
                "title": "FTS title needle",
                "summary": "compact summary target"
            }),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let by_title = meshlet.search_contexts("title needle", 10, SafetyProfile::PublicSafe)?;
        let by_summary =
            meshlet.search_contexts("summary target", 10, SafetyProfile::PublicSafe)?;

        assert_eq!(
            by_title["contexts"]["items"][0]["title"],
            "FTS title needle"
        );
        assert_eq!(
            by_summary["contexts"]["items"][0]["summary"],
            "compact summary target"
        );
        Ok(())
    }

    #[test]
    fn graph_nodes_fts_finds_node_by_label() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "graphify:repo",
                "nodes": [{"id": "node-a", "label": "FTS node needle"}],
                "links": []
            }),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let result =
            meshlet.query_scoped("node needle", Some("nodes"), Some("graphify:repo"), 10)?;
        let items = result["nodes"]["items"].as_array().expect("nodes");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "graphify:repo:node-a");
        Ok(())
    }

    #[test]
    fn graph_edges_fts_finds_edge_by_label_or_kind() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "graphify:repo",
                "nodes": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}],
                "links": [{
                    "source": "a",
                    "target": "b",
                    "relation": "depends_on",
                    "label": "FTS edge needle"
                }]
            }),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let by_label =
            meshlet.query_scoped("edge needle", Some("edges"), Some("graphify:repo"), 10)?;
        let by_kind =
            meshlet.query_scoped("depends on", Some("edges"), Some("graphify:repo"), 10)?;

        assert_eq!(
            by_label["edges"]["items"][0]["id"],
            by_kind["edges"]["items"][0]["id"]
        );
        assert_eq!(by_kind["edges"]["items"][0]["kind"], "depends_on");
        Ok(())
    }

    #[test]
    fn skills_fts_finds_skill_by_name_or_summary() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "skill.added",
            "agent:test",
            json!({
                "name": "fts-skill-needle",
                "version": "0.1.0",
                "kind": "skill",
                "manifest_path": "skill.toml",
                "entry": "./SKILL.md",
                "permissions": ["read_repo"],
                "description": "compact skill summary target"
            }),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let by_name = meshlet.search_skills("skill needle", 10, SafetyProfile::PublicSafe)?;
        let by_summary = meshlet.search_skills("summary target", 10, SafetyProfile::PublicSafe)?;

        assert_eq!(by_name["skills"]["items"][0]["name"], "fts-skill-needle");
        assert_eq!(by_summary["skills"]["items"][0]["name"], "fts-skill-needle");
        Ok(())
    }

    #[test]
    fn events_fts_finds_event_by_compact_text() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"label": "compact event needle", "body": "not indexed"}),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let result = meshlet.query_scoped_view(
            "event needle",
            Some("events"),
            None,
            10,
            OutputMode::Compact,
            SafetyProfile::PublicSafe,
        )?;
        let items = result["events"]["items"].as_array().expect("events");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["label"], "compact event needle");
        Ok(())
    }

    #[test]
    fn fts_public_safe_filters_visibility_before_limit() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"label": "needle public"}),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        for index in 0..12 {
            let visibility = if index % 2 == 0 {
                EventVisibility::Private
            } else {
                EventVisibility::Local
            };
            meshlet.append_event_with_options(
                "context.added",
                "agent:test",
                json!({"label": format!("needle needle needle hidden {index}")}),
                visibility,
                SafetyProfile::LocalTrusted,
            )?;
        }

        let result = meshlet.search_contexts("needle", 1, SafetyProfile::PublicSafe)?;
        let items = result["contexts"]["items"].as_array().expect("contexts");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["summary"], "needle public");
        assert_eq!(items[0]["visibility"], "public");
        Ok(())
    }

    #[test]
    fn rebuild_read_models_rebuilds_fts_deterministically() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event(
            "context.added",
            "agent:test",
            json!({"label": "deterministic context needle"}),
        )?;
        meshlet.append_event(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "graphify:repo",
                "nodes": [{"id": "a", "label": "deterministic node needle"}],
                "links": []
            }),
        )?;
        meshlet.append_event(
            "skill.added",
            "agent:test",
            skill_payload("deterministic-skill"),
        )?;

        let before_contexts =
            meshlet.search_contexts("deterministic", 20, SafetyProfile::LocalTrusted)?;
        let before_nodes = meshlet.query_scoped("deterministic", Some("nodes"), None, 20)?;
        let before_skills =
            meshlet.search_skills("deterministic", 20, SafetyProfile::LocalTrusted)?;

        meshlet.clear_fts()?;
        meshlet.conn.execute("DELETE FROM contexts", [])?;
        meshlet.conn.execute("DELETE FROM graph_nodes", [])?;
        meshlet.conn.execute("DELETE FROM graph_edges", [])?;
        meshlet.conn.execute("DELETE FROM skills", [])?;
        meshlet.rebuild_graph()?;

        assert_eq!(
            before_contexts,
            meshlet.search_contexts("deterministic", 20, SafetyProfile::LocalTrusted)?
        );
        assert_eq!(
            before_nodes,
            meshlet.query_scoped("deterministic", Some("nodes"), None, 20)?
        );
        assert_eq!(
            before_skills,
            meshlet.search_skills("deterministic", 20, SafetyProfile::LocalTrusted)?
        );
        Ok(())
    }

    #[test]
    fn public_safe_graph_query_filters_visibility_before_limit() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        for index in 0..12 {
            let visibility = if index % 2 == 0 {
                EventVisibility::Private
            } else {
                EventVisibility::Local
            };
            meshlet.append_event_with_options(
                "graph.imported",
                "agent:test",
                json!({
                    "source": "graphify",
                    "namespace": "graphify:repo",
                    "nodes": [{"id": format!("a-hidden-{index}"), "label": "needle hidden"}],
                    "links": []
                }),
                visibility,
                SafetyProfile::LocalTrusted,
            )?;
        }
        meshlet.append_event_with_options(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "graphify:repo",
                "nodes": [{"id": "z-public", "label": "needle public"}],
                "links": []
            }),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let result = meshlet.query_scoped_view(
            "needle",
            Some("nodes"),
            Some("graphify:repo"),
            1,
            OutputMode::Compact,
            SafetyProfile::PublicSafe,
        )?;
        let items = result["nodes"]["items"].as_array().expect("nodes");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "graphify:repo:z-public");
        assert_eq!(items[0]["visibility"], "public");
        Ok(())
    }

    #[test]
    fn public_safe_skill_list_filters_visibility_before_limit() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        for index in 0..12 {
            let visibility = if index % 2 == 0 {
                EventVisibility::Private
            } else {
                EventVisibility::Local
            };
            meshlet.append_event_with_options(
                "skill.added",
                "agent:test",
                skill_payload(&format!("aaa-hidden-{index}")),
                visibility,
                SafetyProfile::LocalTrusted,
            )?;
        }
        meshlet.append_event_with_options(
            "skill.added",
            "agent:test",
            skill_payload("zzz-public"),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let skills = meshlet.list_skills_limited(1, SafetyProfile::PublicSafe)?;

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0]["name"], "zzz-public");
        assert_eq!(skills[0]["visibility"], "public");
        Ok(())
    }

    #[test]
    fn public_safe_digest_scopes_counts_and_namespaces() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "public-ns",
                "nodes": [{"id": "public-node", "label": "Public node"}],
                "links": []
            }),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        meshlet.append_event_with_options(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "private-ns",
                "nodes": [{"id": "private-node", "label": "Private node"}],
                "links": []
            }),
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;

        let digest = meshlet.context_digest_limited(1, SafetyProfile::PublicSafe)?;

        assert_eq!(digest["counts"]["events"], 1);
        assert_eq!(digest["counts"]["namespaces"], 1);
        assert_eq!(
            digest["events_recent"]["items"]
                .as_array()
                .expect("events")
                .len(),
            1
        );
        assert_eq!(
            digest["graph"]["nodes"]["items"][0]["id"],
            "public-ns:public-node"
        );
        assert_eq!(digest["graph"]["nodes"]["items"][0]["visibility"], "public");
        Ok(())
    }

    #[test]
    fn query_finds_events_nodes_and_edges() -> Result<()> {
        let dir = tempdir()?;
        let manifest_path = dir.path().join("skill.toml");
        fs::write(
            &manifest_path,
            r#"
name = "query-rust-review"
version = "0.1.0"
kind = "skill"
entry = "./SKILL.md"
permissions = ["read_repo"]
"#,
        )?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.add_skill_manifest(&manifest_path)?;

        let all = meshlet.query("query-rust-review", Some("all"), 20)?;
        assert_eq!(all["kind"], "all");
        assert!(
            all["events"]["items"]
                .as_array()
                .expect("event items")
                .iter()
                .any(|event| event["type"] == "skill.added")
        );
        assert!(
            all["nodes"]["items"]
                .as_array()
                .expect("node items")
                .iter()
                .any(|node| node["kind"] == "skill")
        );

        let edges = meshlet.query("references", Some("edges"), 20)?;
        assert!(
            edges["edges"]["items"]
                .as_array()
                .expect("edge items")
                .iter()
                .any(|edge| edge["kind"] == "references")
        );
        assert!(edges["events"].is_null());
        Ok(())
    }

    #[test]
    fn query_rejects_empty_or_unknown_kind() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;

        assert!(meshlet.query(" ", Some("all"), 20).is_err());
        assert!(meshlet.query("repo", Some("bad"), 20).is_err());
        Ok(())
    }

    #[test]
    fn query_namespace_filters_imported_nodes_and_edges() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "graphify:repo",
                "nodes": [
                    {"id": "a", "label": "Needle"},
                    {"id": "b", "label": "Target"}
                ],
                "links": [{"source": "a", "target": "b", "relation": "uses"}]
            }),
        )?;
        meshlet.append_event(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "graphify:other",
                "nodes": [{"id": "a", "label": "Needle"}],
                "links": []
            }),
        )?;

        let scoped = meshlet.query_scoped("Needle", Some("nodes"), Some("graphify:repo"), 20)?;
        assert_eq!(scoped["namespace"], "graphify:repo");
        assert_eq!(scoped["nodes"]["items"].as_array().expect("nodes").len(), 1);
        assert_eq!(
            scoped["nodes"]["items"][0]["attrs"]["namespace"],
            "graphify:repo"
        );

        let edges = meshlet.query_scoped("uses", Some("edges"), Some("graphify:repo"), 20)?;
        assert_eq!(edges["edges"]["items"].as_array().expect("edges").len(), 1);
        Ok(())
    }
}
