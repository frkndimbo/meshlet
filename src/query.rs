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
                    "items": nodes.items.into_iter().map(compact_node).collect::<Vec<_>>(),
                    "truncated": nodes.truncated,
                },
                "edges": {
                    "items": edges.items.into_iter().map(compact_edge).collect::<Vec<_>>(),
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
