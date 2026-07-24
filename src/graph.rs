use super::*;

impl Meshlet {
    pub fn rebuild_graph(&self) -> Result<()> {
        self.rebuild_read_models()
    }

    pub(crate) fn rebuild_read_models(&self) -> Result<()> {
        self.conn.execute("DELETE FROM contexts", [])?;
        self.conn.execute("DELETE FROM graph_edges", [])?;
        self.conn.execute("DELETE FROM graph_nodes", [])?;
        self.conn.execute("DELETE FROM skills", [])?;
        self.conn.execute("DELETE FROM tasks", [])?;
        self.conn.execute("DELETE FROM mailbox_messages", [])?;
        self.clear_fts()?;
        let events = self.events_ascending()?;
        for event in events {
            self.upsert_event_fts(&event)?;
            self.apply_event(&event)?;
        }
        Ok(())
    }

    pub fn graph_nodes(&self, kind: Option<&str>) -> Result<Vec<Value>> {
        Ok(self
            .graph_nodes_bounded(kind, DEFAULT_LIMIT, SafetyProfile::LocalTrusted)?
            .items)
    }

    pub fn graph_nodes_limited(&self, kind: Option<&str>, limit: u32) -> Result<Vec<Value>> {
        Ok(self
            .graph_nodes_bounded(kind, limit, SafetyProfile::LocalTrusted)?
            .items)
    }

    pub(crate) fn graph_nodes_bounded(
        &self,
        kind: Option<&str>,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Bounded<Value>> {
        let limit = clamp_limit(limit);
        let visibility = visibility_clause(profile);
        let (sql, params_value): (String, Vec<String>) = match kind {
            Some(kind) => (
                format!(
                    "SELECT id, kind, label, attrs_json, source_event_id, visibility FROM graph_nodes WHERE {visibility} AND kind = ?1 ORDER BY id LIMIT ?2"
                ),
                vec![kind.to_string(), (limit + 1).to_string()],
            ),
            None => (
                format!(
                    "SELECT id, kind, label, attrs_json, source_event_id, visibility FROM graph_nodes WHERE {visibility} ORDER BY id LIMIT ?1"
                ),
                vec![(limit + 1).to_string()],
            ),
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut nodes = stmt
            .query_map(rusqlite::params_from_iter(params_value), node_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = nodes.len() > limit as usize;
        nodes.truncate(limit as usize);
        Ok(Bounded {
            items: nodes,
            truncated,
        })
    }

    pub fn graph_edges(&self, from_id: Option<&str>) -> Result<Vec<Value>> {
        Ok(self
            .graph_edges_bounded(from_id, DEFAULT_LIMIT, SafetyProfile::LocalTrusted)?
            .items)
    }

    pub fn graph_edges_limited(&self, from_id: Option<&str>, limit: u32) -> Result<Vec<Value>> {
        Ok(self
            .graph_edges_bounded(from_id, limit, SafetyProfile::LocalTrusted)?
            .items)
    }

    pub fn graph_namespaces(&self) -> Result<Vec<String>> {
        self.graph_namespaces_scoped(SafetyProfile::LocalTrusted)
    }

    pub fn graph_namespaces_scoped(&self, profile: SafetyProfile) -> Result<Vec<String>> {
        let mut namespaces = BTreeSet::new();
        let visibility = visibility_clause(profile);
        let sql = format!(
            "SELECT attrs_json FROM graph_nodes WHERE {visibility}
             UNION ALL
             SELECT attrs_json FROM graph_edges WHERE {visibility}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let attrs = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for attr_json in attrs {
            if let Ok(value) = serde_json::from_str::<Value>(&attr_json) {
                if let Some(namespace) = value.get("namespace").and_then(Value::as_str) {
                    namespaces.insert(namespace.to_string());
                }
            }
        }
        Ok(namespaces.into_iter().collect())
    }

    pub fn import_graph_file(
        &self,
        graph_path: impl AsRef<Path>,
        source: &str,
        namespace: &str,
    ) -> Result<Value> {
        self.import_graph_file_with_visibility(
            graph_path,
            source,
            namespace,
            EventVisibility::Private,
        )
    }

    pub fn import_graph_file_with_visibility(
        &self,
        graph_path: impl AsRef<Path>,
        source: &str,
        namespace: &str,
        visibility: EventVisibility,
    ) -> Result<Value> {
        if source.trim().is_empty() {
            bail!("source must not be empty");
        }
        if namespace.trim().is_empty() {
            bail!("namespace must not be empty");
        }
        let graph_path = graph_path.as_ref();
        let bytes =
            fs::read(graph_path).with_context(|| format!("read graph {}", graph_path.display()))?;
        let graph: Value = serde_json::from_slice(&bytes).context("parse graph JSON")?;
        let nodes = graph
            .get("nodes")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| anyhow!("graph JSON nodes must be an array"))?;
        let links = graph
            .get("links")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| anyhow!("graph JSON links must be an array"))?;
        let source_sha256 = sha256_hex(&bytes);
        let profile = if visibility == EventVisibility::Public {
            SafetyProfile::PublicSafe
        } else {
            SafetyProfile::LocalTrusted
        };
        self.append_event_with_options(
            "graph.imported",
            "cli",
            json!({
                "source": source,
                "namespace": namespace,
                "source_path": graph_path.display().to_string(),
                "source_sha256": source_sha256,
                "nodes": nodes,
                "links": links,
            }),
            visibility,
            profile,
        )?;
        Ok(json!({
            "status": "imported",
            "source": source,
            "namespace": namespace,
            "visibility": visibility.as_str(),
            "source_sha256": source_sha256,
            "nodes": nodes.len(),
            "edges": links.len(),
        }))
    }

    pub(crate) fn graph_edges_bounded(
        &self,
        from_id: Option<&str>,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Bounded<Value>> {
        let limit = clamp_limit(limit);
        let visibility = visibility_clause(profile);
        let (sql, params_value): (String, Vec<String>) = match from_id {
            Some(from_id) => (
                format!(
                    "SELECT id, from_id, to_id, kind, attrs_json, source_event_id, visibility FROM graph_edges WHERE {visibility} AND from_id = ?1 ORDER BY id LIMIT ?2"
                ),
                vec![from_id.to_string(), (limit + 1).to_string()],
            ),
            None => (
                format!(
                    "SELECT id, from_id, to_id, kind, attrs_json, source_event_id, visibility FROM graph_edges WHERE {visibility} ORDER BY id LIMIT ?1"
                ),
                vec![(limit + 1).to_string()],
            ),
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut edges = stmt
            .query_map(rusqlite::params_from_iter(params_value), edge_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = edges.len() > limit as usize;
        edges.truncate(limit as usize);
        Ok(Bounded {
            items: edges,
            truncated,
        })
    }

    pub(crate) fn apply_event(&self, event: &Event) -> Result<()> {
        match event.event_type.as_str() {
            "repo.initialized" => {
                let root = event
                    .payload
                    .get("root")
                    .and_then(Value::as_str)
                    .unwrap_or("repo");
                self.upsert_node(
                    "repo:current",
                    "repo",
                    Some(root),
                    json!({ "root": root, "visibility": event.visibility.as_str() }),
                    &event.id,
                    event.visibility,
                )?;
            }
            "skill.added" => self.apply_skill_added(event)?,
            "context.added" => {
                self.apply_context_added(event)?;
                self.upsert_node(
                    &format!("context:{}", event.id),
                    "context",
                    event.payload.get("label").and_then(Value::as_str),
                    attrs_with_visibility(event.payload.clone(), event.visibility),
                    &event.id,
                    event.visibility,
                )?;
            }
            "agent.message" => self.apply_agent_message(event)?,
            "evidence.attached" => self.apply_evidence(event)?,
            "task.created" | "task.updated" => self.apply_task(event)?,
            "graph.imported" => self.apply_graph_imported(event)?,
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn apply_context_added(&self, event: &Event) -> Result<()> {
        let kind = nonempty_string(&event.payload, "kind").unwrap_or("context");
        let namespace = nonempty_string(&event.payload, "namespace");
        let title = nonempty_string(&event.payload, "title")
            .or_else(|| nonempty_string(&event.payload, "label"));
        let summary = nonempty_string(&event.payload, "summary")
            .or_else(|| nonempty_string(&event.payload, "label"))
            .or_else(|| nonempty_string(&event.payload, "title"))
            .unwrap_or("context.added");
        self.conn.execute(
            "INSERT OR REPLACE INTO contexts(
                id, kind, namespace, title, summary, visibility, source_event_id,
                created_at, updated_at, schema_version, attrs_json
             )
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                format!("context:{}", event.id),
                kind,
                namespace,
                title,
                summary,
                event.visibility.as_str(),
                event.id,
                event.created_at,
                event.created_at,
                CONTEXT_SCHEMA_VERSION,
                canonical_json(&event.payload)?,
            ],
        )?;
        self.upsert_context_fts(
            &format!("context:{}", event.id),
            kind,
            namespace,
            title,
            summary,
        )?;
        Ok(())
    }

    pub(crate) fn apply_graph_imported(&self, event: &Event) -> Result<()> {
        let source = str_field(&event.payload, "source")?;
        let namespace = str_field(&event.payload, "namespace")?;
        let nodes = event
            .payload
            .get("nodes")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("graph.imported nodes must be an array"))?;
        let links = event
            .payload
            .get("links")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("graph.imported links must be an array"))?;

        for node in nodes {
            let external_id = str_field(node, "id")?;
            let node_id = imported_node_id(namespace, external_id);
            let label = node.get("label").and_then(Value::as_str);
            let mut attrs = node.as_object().cloned().unwrap_or_default();
            attrs.insert("namespace".to_string(), json!(namespace));
            attrs.insert("source".to_string(), json!(source));
            attrs.insert("external_id".to_string(), json!(external_id));
            attrs.insert("visibility".to_string(), json!(event.visibility.as_str()));
            if let Some(origin) = attrs.remove("_origin") {
                attrs.insert("origin".to_string(), origin);
            }
            self.upsert_node(
                &node_id,
                "imported",
                label,
                Value::Object(attrs),
                &event.id,
                event.visibility,
            )?;
        }

        for (index, link) in links.iter().enumerate() {
            let source_id = str_field(link, "source")?;
            let target_id = str_field(link, "target")?;
            let relation = link
                .get("relation")
                .and_then(Value::as_str)
                .unwrap_or("references");
            let from_id = imported_node_id(namespace, source_id);
            let to_id = imported_node_id(namespace, target_id);
            let mut attrs = link.as_object().cloned().unwrap_or_default();
            attrs.insert("namespace".to_string(), json!(namespace));
            attrs.insert("source".to_string(), json!(source));
            attrs.insert("relation".to_string(), json!(relation));
            attrs.insert("visibility".to_string(), json!(event.visibility.as_str()));
            self.upsert_edge(
                &format!("edge:{}:graphify:{index}", event.id),
                &from_id,
                &to_id,
                graph_import_edge_kind(relation),
                Value::Object(attrs),
                &event.id,
                event.visibility,
            )?;
        }
        Ok(())
    }

    pub(crate) fn upsert_node(
        &self,
        id: &str,
        kind: &str,
        label: Option<&str>,
        attrs: Value,
        source_event_id: &str,
        visibility: EventVisibility,
    ) -> Result<()> {
        let namespace = graph_namespace(&attrs);
        self.conn.execute(
            "INSERT OR REPLACE INTO graph_nodes(id, kind, label, attrs_json, source_event_id, visibility)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                kind,
                label,
                canonical_json(&attrs)?,
                source_event_id,
                visibility.as_str(),
            ],
        )?;
        self.upsert_graph_node_fts(id, kind, namespace, label)?;
        Ok(())
    }

    // ponytail: keep the call sites flat; a request struct is more churn than value here.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn upsert_edge(
        &self,
        id: &str,
        from_id: &str,
        to_id: &str,
        kind: &str,
        attrs: Value,
        source_event_id: &str,
        visibility: EventVisibility,
    ) -> Result<()> {
        let namespace = graph_namespace(&attrs);
        let label = graph_edge_label(&attrs);
        self.conn.execute(
            "INSERT OR REPLACE INTO graph_edges(id, from_id, to_id, kind, attrs_json, source_event_id, visibility)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                from_id,
                to_id,
                kind,
                canonical_json(&attrs)?,
                source_event_id,
                visibility.as_str(),
            ],
        )?;
        self.upsert_graph_edge_fts(id, kind, namespace, label)?;
        Ok(())
    }

    pub(crate) fn clear_fts(&self) -> Result<()> {
        for table in [
            "events_fts",
            "contexts_fts",
            "graph_nodes_fts",
            "graph_edges_fts",
            "skills_fts",
        ] {
            self.conn.execute(&format!("DELETE FROM {table}"), [])?;
        }
        Ok(())
    }

    pub(crate) fn rebuild_fts(&self) -> Result<()> {
        self.clear_fts()?;
        for event in self.events_ascending()? {
            self.upsert_event_fts(&event)?;
        }

        let context_rows = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, kind, namespace, title, summary FROM contexts ORDER BY id")?;
            stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (id, kind, namespace, title, summary) in context_rows {
            self.upsert_context_fts(&id, &kind, namespace.as_deref(), title.as_deref(), &summary)?;
        }

        let node_rows = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, kind, label, attrs_json FROM graph_nodes ORDER BY id")?;
            stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (id, kind, label, attrs_json) in node_rows {
            let attrs = serde_json::from_str::<Value>(&attrs_json).unwrap_or_else(|_| json!({}));
            self.upsert_graph_node_fts(&id, &kind, graph_namespace(&attrs), label.as_deref())?;
        }

        let edge_rows = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, kind, attrs_json FROM graph_edges ORDER BY id")?;
            stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (id, kind, attrs_json) in edge_rows {
            let attrs = serde_json::from_str::<Value>(&attrs_json).unwrap_or_else(|_| json!({}));
            self.upsert_graph_edge_fts(
                &id,
                &kind,
                graph_namespace(&attrs),
                graph_edge_label(&attrs),
            )?;
        }

        let skill_rows = {
            let mut stmt = self
                .conn
                .prepare("SELECT name, description FROM skills ORDER BY name")?;
            stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (name, description) in skill_rows {
            self.upsert_skill_fts(&name, description.as_deref())?;
        }
        Ok(())
    }

    pub(crate) fn upsert_event_fts(&self, event: &Event) -> Result<()> {
        self.conn
            .execute("DELETE FROM events_fts WHERE id = ?1", [event.id.as_str()])?;
        self.conn.execute(
            "INSERT INTO events_fts(id, type, actor, text) VALUES(?1, ?2, ?3, ?4)",
            params![
                event.id.as_str(),
                event.event_type.as_str(),
                event.actor.as_str(),
                compact_event_text(event),
            ],
        )?;
        Ok(())
    }

    pub(crate) fn upsert_context_fts(
        &self,
        id: &str,
        kind: &str,
        namespace: Option<&str>,
        title: Option<&str>,
        summary: &str,
    ) -> Result<()> {
        self.conn
            .execute("DELETE FROM contexts_fts WHERE id = ?1", [id])?;
        self.conn.execute(
            "INSERT INTO contexts_fts(id, kind, namespace, title, summary)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![id, kind, namespace, title, summary],
        )?;
        Ok(())
    }

    pub(crate) fn upsert_graph_node_fts(
        &self,
        id: &str,
        kind: &str,
        namespace: Option<&str>,
        label: Option<&str>,
    ) -> Result<()> {
        self.conn
            .execute("DELETE FROM graph_nodes_fts WHERE id = ?1", [id])?;
        self.conn.execute(
            "INSERT INTO graph_nodes_fts(id, kind, namespace, label)
             VALUES(?1, ?2, ?3, ?4)",
            params![id, kind, namespace, label],
        )?;
        Ok(())
    }

    pub(crate) fn upsert_graph_edge_fts(
        &self,
        id: &str,
        kind: &str,
        namespace: Option<&str>,
        label: Option<&str>,
    ) -> Result<()> {
        self.conn
            .execute("DELETE FROM graph_edges_fts WHERE id = ?1", [id])?;
        self.conn.execute(
            "INSERT INTO graph_edges_fts(id, kind, namespace, label)
             VALUES(?1, ?2, ?3, ?4)",
            params![id, kind, namespace, label],
        )?;
        Ok(())
    }
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
    fn graph_nodes_visibility_column_materializes_from_event_visibility() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"label": "public context"}),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let nodes = meshlet.graph_nodes(Some("context"))?;

        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["visibility"], "public");
        assert_eq!(nodes[0]["attrs"]["visibility"], "public");
        Ok(())
    }

    #[test]
    fn graph_edges_visibility_column_materializes_from_event_visibility() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "graphify:repo",
                "nodes": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}],
                "links": [{"source": "a", "target": "b", "relation": "uses"}]
            }),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let edges = meshlet.graph_edges_limited(Some("graphify:repo:a"), 20)?;

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["visibility"], "public");
        assert_eq!(edges[0]["attrs"]["visibility"], "public");
        Ok(())
    }

    #[test]
    fn rebuild_read_models_preserves_visibility_columns() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "skill.added",
            "agent:test",
            skill_payload("local-skill"),
            EventVisibility::Local,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.append_event_with_options(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "graphify:repo",
                "nodes": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}],
                "links": [{"source": "a", "target": "b", "relation": "uses"}]
            }),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        let before_nodes = meshlet.graph_nodes_limited(None, 20)?;
        let before_edges = meshlet.graph_edges_limited(None, 20)?;
        let before_skills = meshlet.list_skills()?;

        meshlet.conn.execute("DELETE FROM graph_nodes", [])?;
        meshlet.conn.execute("DELETE FROM graph_edges", [])?;
        meshlet.conn.execute("DELETE FROM skills", [])?;
        meshlet.rebuild_graph()?;

        assert_eq!(before_nodes, meshlet.graph_nodes_limited(None, 20)?);
        assert_eq!(before_edges, meshlet.graph_edges_limited(None, 20)?);
        assert_eq!(before_skills, meshlet.list_skills()?);
        Ok(())
    }

    #[test]
    fn graph_rebuild_is_deterministic() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event(
            "evidence.attached",
            "agent:test",
            json!({"path": "src/main.rs", "note": "entry"}),
        )?;
        let before = meshlet.graph_nodes(None)?;
        meshlet.conn.execute("DELETE FROM graph_nodes", [])?;
        meshlet.conn.execute("DELETE FROM graph_edges", [])?;
        meshlet.rebuild_graph()?;
        assert_eq!(before, meshlet.graph_nodes(None)?);
        Ok(())
    }

    #[test]
    fn graph_import_event_accepts_valid_payload() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;

        meshlet.append_event(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "graphify:repo",
                "source_path": "graphify-out/graph.json",
                "source_sha256": "abc123",
                "nodes": [],
                "links": []
            }),
        )?;

        assert_eq!(meshlet.event_count()?, 2);
        assert!(meshlet.verify_event_chain()?.ok);
        Ok(())
    }

    #[test]
    fn graph_import_event_rejects_invalid_payload() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;

        assert!(
            meshlet
                .append_event(
                    "graph.imported",
                    "agent:test",
                    json!({"namespace": "graphify:repo", "nodes": [], "links": []})
                )
                .is_err()
        );
        assert!(
            meshlet
                .append_event(
                    "graph.imported",
                    "agent:test",
                    json!({"source": "graphify", "namespace": "", "nodes": [], "links": []})
                )
                .is_err()
        );
        assert!(
            meshlet
                .append_event(
                    "graph.imported",
                    "agent:test",
                    json!({"source": "graphify", "namespace": "graphify:repo", "nodes": {}, "links": []})
                )
                .is_err()
        );
        assert!(
            meshlet
                .append_event(
                    "graph.imported",
                    "agent:test",
                    json!({"source": "graphify", "namespace": "graphify:repo", "nodes": [], "links": {}})
                )
                .is_err()
        );
        assert_eq!(meshlet.event_count()?, 1);
        Ok(())
    }

    #[test]
    fn graph_import_materializes_namespaced_nodes_and_edges() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "graphify:repo",
                "nodes": [
                    {"id": "a", "label": "A", "source_file": "a.rs", "_origin": "ast"},
                    {"id": "b", "label": "B", "source_file": "b.rs", "_origin": "ast"}
                ],
                "links": [
                    {"source": "a", "target": "b", "relation": "uses", "confidence": "EXTRACTED"}
                ]
            }),
        )?;

        let nodes = meshlet.graph_nodes_limited(None, 20)?;
        assert!(
            nodes.iter().any(|node| node["id"] == "graphify:repo:a"
                && node["attrs"]["namespace"] == "graphify:repo")
        );
        let edges = meshlet.graph_edges_limited(Some("graphify:repo:a"), 20)?;
        assert!(edges.iter().any(|edge| {
            edge["to_id"] == "graphify:repo:b"
                && edge["kind"] == "uses"
                && edge["attrs"]["relation"] == "uses"
        }));
        Ok(())
    }

    #[test]
    fn graph_rebuild_preserves_imported_graph() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "graphify:repo",
                "nodes": [
                    {"id": "a", "label": "A"},
                    {"id": "b", "label": "B"}
                ],
                "links": [
                    {"source": "a", "target": "b", "relation": "unknown_relation"}
                ]
            }),
        )?;
        let before_nodes = meshlet.graph_nodes_limited(None, 20)?;
        let before_edges = meshlet.graph_edges_limited(None, 20)?;

        meshlet.conn.execute("DELETE FROM graph_nodes", [])?;
        meshlet.conn.execute("DELETE FROM graph_edges", [])?;
        meshlet.rebuild_graph()?;

        assert_eq!(before_nodes, meshlet.graph_nodes_limited(None, 20)?);
        assert_eq!(before_edges, meshlet.graph_edges_limited(None, 20)?);
        assert!(
            meshlet
                .graph_edges_limited(Some("graphify:repo:a"), 20)?
                .iter()
                .any(|edge| edge["kind"] == "references"
                    && edge["attrs"]["relation"] == "unknown_relation")
        );
        Ok(())
    }

    #[test]
    fn graph_import_file_appends_event_with_digest_and_counts() -> Result<()> {
        let dir = tempdir()?;
        let graph_path = dir.path().join("graph.json");
        fs::write(
            &graph_path,
            r#"{
                "nodes": [
                    {"id": "a", "label": "A"},
                    {"id": "b", "label": "B"}
                ],
                "links": [
                    {"source": "a", "target": "b", "relation": "uses"}
                ]
            }"#,
        )?;
        let meshlet = Meshlet::init(dir.path())?;

        let report = meshlet.import_graph_file(&graph_path, "graphify", "graphify:repo")?;

        assert_eq!(report["status"], "imported");
        assert_eq!(report["source"], "graphify");
        assert_eq!(report["namespace"], "graphify:repo");
        assert_eq!(report["visibility"], "private");
        assert_eq!(report["nodes"], 2);
        assert_eq!(report["edges"], 1);
        assert!(report["source_sha256"].as_str().expect("digest").len() == 64);
        assert_eq!(meshlet.event_count()?, 2);
        Ok(())
    }

    #[test]
    fn graph_import_file_default_visibility_is_private() -> Result<()> {
        let dir = tempdir()?;
        let graph_path = dir.path().join("graph.json");
        fs::write(
            &graph_path,
            r#"{
                "nodes": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}],
                "links": [{"source": "a", "target": "b", "relation": "uses"}]
            }"#,
        )?;
        let meshlet = Meshlet::init(dir.path())?;

        meshlet.import_graph_file(&graph_path, "graphify", "graphify:repo")?;

        let event = meshlet
            .list_events(20)?
            .into_iter()
            .find(|event| event.event_type == "graph.imported")
            .expect("graph import event");
        let nodes = meshlet.graph_nodes_limited(Some("imported"), 20)?;
        let edges = meshlet.graph_edges_limited(Some("graphify:repo:a"), 20)?;

        assert_eq!(event.visibility, EventVisibility::Private);
        assert!(nodes.iter().all(|node| node["visibility"] == "private"));
        assert!(edges.iter().all(|edge| edge["visibility"] == "private"));
        Ok(())
    }

    #[test]
    fn graph_import_file_local_visibility_materializes_local_graph() -> Result<()> {
        let dir = tempdir()?;
        let graph_path = dir.path().join("graph.json");
        fs::write(
            &graph_path,
            r#"{
                "nodes": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}],
                "links": [{"source": "a", "target": "b", "relation": "uses"}]
            }"#,
        )?;
        let meshlet = Meshlet::init(dir.path())?;

        meshlet.import_graph_file_with_visibility(
            &graph_path,
            "graphify",
            "graphify:repo",
            EventVisibility::Local,
        )?;

        let nodes = meshlet.graph_nodes_limited(Some("imported"), 20)?;
        let edges = meshlet.graph_edges_limited(Some("graphify:repo:a"), 20)?;

        assert_eq!(nodes.len(), 2);
        assert_eq!(edges.len(), 1);
        assert!(nodes.iter().all(|node| node["visibility"] == "local"));
        assert!(edges.iter().all(|edge| edge["visibility"] == "local"));
        assert!(
            nodes
                .iter()
                .all(|node| node["attrs"]["visibility"] == "local")
        );
        assert!(
            edges
                .iter()
                .all(|edge| edge["attrs"]["visibility"] == "local")
        );
        Ok(())
    }

    #[test]
    fn public_safe_graph_reads_exclude_private_and_local_imports() -> Result<()> {
        let dir = tempdir()?;
        let private_path = dir.path().join("private-graph.json");
        let local_path = dir.path().join("local-graph.json");
        fs::write(
            &private_path,
            r#"{
                "nodes": [{"id": "private-a", "label": "Hidden private"}],
                "links": []
            }"#,
        )?;
        fs::write(
            &local_path,
            r#"{
                "nodes": [{"id": "local-a", "label": "Hidden local"}, {"id": "local-b", "label": "Hidden local target"}],
                "links": [{"source": "local-a", "target": "local-b", "relation": "uses"}]
            }"#,
        )?;
        let meshlet = Meshlet::init(dir.path())?;

        meshlet.import_graph_file(&private_path, "graphify", "graphify:repo")?;
        meshlet.import_graph_file_with_visibility(
            &local_path,
            "graphify",
            "graphify:repo",
            EventVisibility::Local,
        )?;

        assert_eq!(
            meshlet
                .graph_nodes_bounded(None, 20, SafetyProfile::PublicSafe)?
                .items
                .len(),
            0
        );
        assert_eq!(
            meshlet
                .graph_edges_bounded(None, 20, SafetyProfile::PublicSafe)?
                .items
                .len(),
            0
        );
        Ok(())
    }

    #[test]
    fn public_graph_import_is_included_in_public_safe_reads_and_export() -> Result<()> {
        let dir = tempdir()?;
        let graph_path = dir.path().join("graph.json");
        fs::write(
            &graph_path,
            r#"{
                "nodes": [{"id": "public-a", "label": "Public graph"}, {"id": "public-b", "label": "Public target"}],
                "links": [{"source": "public-a", "target": "public-b", "relation": "uses"}]
            }"#,
        )?;
        let meshlet = Meshlet::init(dir.path())?;

        meshlet.import_graph_file_with_visibility(
            &graph_path,
            "graphify",
            "graphify:repo",
            EventVisibility::Public,
        )?;

        let nodes = meshlet
            .graph_nodes_bounded(None, 20, SafetyProfile::PublicSafe)?
            .items;
        let edges = meshlet
            .graph_edges_bounded(None, 20, SafetyProfile::PublicSafe)?
            .items;
        let export = meshlet.public_export(20)?;

        assert!(
            nodes
                .iter()
                .any(|node| node["id"] == "graphify:repo:public-a")
        );
        assert!(
            edges
                .iter()
                .any(|edge| edge["from_id"] == "graphify:repo:public-a")
        );
        assert!(
            export["graph"]["nodes"]
                .as_array()
                .expect("export nodes")
                .iter()
                .any(|node| node["id"] == "graphify:repo:public-a")
        );
        assert!(
            export["graph"]["edges"]
                .as_array()
                .expect("export edges")
                .iter()
                .any(|edge| edge["from_id"] == "graphify:repo:public-a")
        );
        Ok(())
    }

    #[test]
    fn graph_namespaces_lists_imported_namespaces() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "graphify:repo",
                "nodes": [{"id": "a", "label": "A"}],
                "links": []
            }),
        )?;

        assert_eq!(meshlet.graph_namespaces()?, vec!["graphify:repo"]);
        Ok(())
    }
}
