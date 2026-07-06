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
        self.append_event(
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
        )?;
        Ok(json!({
            "status": "imported",
            "source": source,
            "namespace": namespace,
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
