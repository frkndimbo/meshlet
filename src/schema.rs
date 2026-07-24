use super::*;

impl Meshlet {
    pub(crate) fn apply_schema(&self) -> Result<()> {
        let had_contexts = self.has_table("contexts")?;
        let had_tasks = self.has_table("tasks")?;
        let had_mailbox_messages = self.has_table("mailbox_messages")?;
        self.conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY,
                seq INTEGER NOT NULL UNIQUE,
                type TEXT NOT NULL,
                created_at TEXT NOT NULL,
                actor TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                visibility TEXT NOT NULL DEFAULT 'private',
                hash TEXT NOT NULL UNIQUE,
                prev_hash TEXT
            );
            CREATE TABLE IF NOT EXISTS graph_nodes (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                label TEXT,
                attrs_json TEXT NOT NULL,
                source_event_id TEXT NOT NULL,
                visibility TEXT NOT NULL DEFAULT 'private'
            );
            CREATE TABLE IF NOT EXISTS graph_edges (
                id TEXT PRIMARY KEY,
                from_id TEXT NOT NULL,
                to_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                attrs_json TEXT NOT NULL,
                source_event_id TEXT NOT NULL,
                visibility TEXT NOT NULL DEFAULT 'private'
            );
            CREATE TABLE IF NOT EXISTS skills (
                name TEXT PRIMARY KEY,
                version TEXT NOT NULL,
                manifest_path TEXT NOT NULL,
                entry TEXT NOT NULL,
                permissions_json TEXT NOT NULL,
                description TEXT,
                source_event_id TEXT NOT NULL,
                visibility TEXT NOT NULL DEFAULT 'private'
            );
            CREATE TABLE IF NOT EXISTS contexts (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                namespace TEXT,
                title TEXT,
                summary TEXT NOT NULL,
                visibility TEXT NOT NULL,
                source_event_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                schema_version INTEGER NOT NULL,
                attrs_json TEXT
            );
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                status TEXT NOT NULL,
                assignee TEXT,
                note TEXT,
                visibility TEXT NOT NULL,
                created_event_id TEXT NOT NULL,
                updated_event_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                schema_version INTEGER NOT NULL,
                attrs_json TEXT
            );
            CREATE TABLE IF NOT EXISTS mailbox_messages (
                id TEXT PRIMARY KEY,
                from_agent TEXT NOT NULL,
                to_agent TEXT NOT NULL,
                task_id TEXT,
                summary TEXT NOT NULL,
                body TEXT,
                reply_to TEXT,
                visibility TEXT NOT NULL,
                source_event_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                schema_version INTEGER NOT NULL,
                attrs_json TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_contexts_visibility_created_at
                ON contexts(visibility, created_at);
            CREATE INDEX IF NOT EXISTS idx_contexts_source_event_id
                ON contexts(source_event_id);
            CREATE INDEX IF NOT EXISTS idx_tasks_visibility_updated_at
                ON tasks(visibility, updated_at);
            CREATE INDEX IF NOT EXISTS idx_tasks_assignee_status
                ON tasks(assignee, status);
            CREATE INDEX IF NOT EXISTS idx_mailbox_to_visibility_created_at
                ON mailbox_messages(to_agent, visibility, created_at);
            CREATE INDEX IF NOT EXISTS idx_mailbox_from_visibility_created_at
                ON mailbox_messages(from_agent, visibility, created_at);
            CREATE INDEX IF NOT EXISTS idx_mailbox_task_id
                ON mailbox_messages(task_id);
            "#,
        )?;
        if !self.has_column("events", "visibility")? {
            self.conn.execute(
                "ALTER TABLE events ADD COLUMN visibility TEXT NOT NULL DEFAULT 'private'",
                [],
            )?;
        }
        let needs_read_model_visibility = self.ensure_read_model_visibility_columns()?;
        self.conn.execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_graph_nodes_visibility
                ON graph_nodes(visibility);
            CREATE INDEX IF NOT EXISTS idx_graph_edges_visibility
                ON graph_edges(visibility);
            CREATE INDEX IF NOT EXISTS idx_skills_visibility
                ON skills(visibility);
            "#,
        )?;
        let needs_fts_backfill = self.ensure_fts_tables()?;
        if needs_read_model_visibility {
            self.backfill_read_model_visibility()?;
        }
        if !had_contexts {
            self.backfill_contexts_from_events()?;
        }
        if !had_tasks || !had_mailbox_messages {
            self.backfill_mailbox_from_events()?;
        }
        if needs_fts_backfill {
            self.rebuild_fts()?;
        }
        self.conn.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES('schema_version', ?1)",
            [SCHEMA_VERSION],
        )?;
        Ok(())
    }

    pub(crate) fn ensure_read_model_visibility_columns(&self) -> Result<bool> {
        let mut changed = false;
        for table in ["graph_nodes", "graph_edges", "skills"] {
            if !self.has_column(table, "visibility")? {
                self.conn.execute(
                    &format!(
                        "ALTER TABLE {table} ADD COLUMN visibility TEXT NOT NULL DEFAULT 'private'"
                    ),
                    [],
                )?;
                changed = true;
            }
        }
        Ok(changed)
    }

    pub(crate) fn ensure_fts_tables(&self) -> Result<bool> {
        let mut changed = false;
        for table in [
            "events_fts",
            "contexts_fts",
            "graph_nodes_fts",
            "graph_edges_fts",
            "skills_fts",
        ] {
            changed |= !self.has_table(table)?;
        }
        self.conn
            .execute_batch(
                r#"
                CREATE VIRTUAL TABLE IF NOT EXISTS events_fts
                    USING fts5(id UNINDEXED, type, actor, text);
                CREATE VIRTUAL TABLE IF NOT EXISTS contexts_fts
                    USING fts5(id UNINDEXED, kind, namespace, title, summary);
                CREATE VIRTUAL TABLE IF NOT EXISTS graph_nodes_fts
                    USING fts5(id UNINDEXED, kind, namespace, label);
                CREATE VIRTUAL TABLE IF NOT EXISTS graph_edges_fts
                    USING fts5(id UNINDEXED, kind, namespace, label);
                CREATE VIRTUAL TABLE IF NOT EXISTS skills_fts
                    USING fts5(name UNINDEXED, searchable_name, kind, summary);
                "#,
            )
            .context("create FTS5 tables")?;
        Ok(changed)
    }

    pub(crate) fn has_table(&self, table: &str) -> Result<bool> {
        let exists = self
            .conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        Ok(exists)
    }

    pub(crate) fn has_column(&self, table: &str, column: &str) -> Result<bool> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(columns.iter().any(|name| name == column))
    }

    pub(crate) fn backfill_contexts_from_events(&self) -> Result<()> {
        for event in self.events_ascending()? {
            if event.event_type == "context.added" {
                self.apply_context_added(&event)?;
            }
        }
        Ok(())
    }

    pub(crate) fn backfill_mailbox_from_events(&self) -> Result<()> {
        for event in self.events_ascending()? {
            if matches!(event.event_type.as_str(), "task.created" | "task.updated") {
                self.apply_task(&event)?;
            } else if event.event_type == "agent.message" {
                self.apply_agent_message(&event)?;
            }
        }
        Ok(())
    }

    pub(crate) fn backfill_read_model_visibility(&self) -> Result<()> {
        let event_visibility = self.event_visibility_by_id()?;
        self.backfill_graph_visibility("graph_nodes", "id", &event_visibility)?;
        self.backfill_graph_visibility("graph_edges", "id", &event_visibility)?;
        self.backfill_skill_visibility(&event_visibility)?;
        Ok(())
    }

    pub(crate) fn event_visibility_by_id(&self) -> Result<BTreeMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT id, visibility FROM events")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, visibility)| {
                strict_visibility(&visibility).map(|visibility| (id, visibility.to_string()))
            })
            .collect())
    }

    pub(crate) fn backfill_graph_visibility(
        &self,
        table: &str,
        id_column: &str,
        event_visibility: &BTreeMap<String, String>,
    ) -> Result<()> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {id_column}, source_event_id, attrs_json FROM {table}"
        ))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, source_event_id, attrs_json) in rows {
            let visibility = event_visibility
                .get(&source_event_id)
                .map(String::as_str)
                .or_else(|| attrs_visibility(&attrs_json))
                .unwrap_or("private");
            self.conn.execute(
                &format!("UPDATE {table} SET visibility = ?1 WHERE {id_column} = ?2"),
                params![visibility, id],
            )?;
        }
        Ok(())
    }

    pub(crate) fn backfill_skill_visibility(
        &self,
        event_visibility: &BTreeMap<String, String>,
    ) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare("SELECT name, source_event_id FROM skills")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (name, source_event_id) in rows {
            let visibility = event_visibility
                .get(&source_event_id)
                .map(String::as_str)
                .unwrap_or("private");
            self.conn.execute(
                "UPDATE skills SET visibility = ?1 WHERE name = ?2",
                params![visibility, name],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn migration_from_v4_creates_and_backfills_fts() -> Result<()> {
        let dir = tempdir()?;
        let db_dir = dir.path().join(DB_DIR);
        fs::create_dir_all(&db_dir)?;
        let conn = Connection::open(db_dir.join(DB_FILE))?;
        conn.execute_batch(
            r#"
            CREATE TABLE meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE events (
                id TEXT PRIMARY KEY,
                seq INTEGER NOT NULL UNIQUE,
                type TEXT NOT NULL,
                created_at TEXT NOT NULL,
                actor TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                visibility TEXT NOT NULL DEFAULT 'private',
                hash TEXT NOT NULL UNIQUE,
                prev_hash TEXT
            );
            CREATE TABLE graph_nodes (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                label TEXT,
                attrs_json TEXT NOT NULL,
                source_event_id TEXT NOT NULL,
                visibility TEXT NOT NULL DEFAULT 'private'
            );
            CREATE TABLE graph_edges (
                id TEXT PRIMARY KEY,
                from_id TEXT NOT NULL,
                to_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                attrs_json TEXT NOT NULL,
                source_event_id TEXT NOT NULL,
                visibility TEXT NOT NULL DEFAULT 'private'
            );
            CREATE TABLE skills (
                name TEXT PRIMARY KEY,
                version TEXT NOT NULL,
                manifest_path TEXT NOT NULL,
                entry TEXT NOT NULL,
                permissions_json TEXT NOT NULL,
                description TEXT,
                source_event_id TEXT NOT NULL,
                visibility TEXT NOT NULL DEFAULT 'private'
            );
            CREATE TABLE contexts (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                namespace TEXT,
                title TEXT,
                summary TEXT NOT NULL,
                visibility TEXT NOT NULL,
                source_event_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                schema_version INTEGER NOT NULL,
                attrs_json TEXT
            );
            INSERT INTO meta(key, value) VALUES('schema_version', '4');
            INSERT INTO events(id, seq, type, created_at, actor, payload_json, visibility, hash, prev_hash)
                VALUES('event-v4', 1, 'context.added', '2026-06-28T00:00:00.000Z', 'agent:test', '{"label":"legacy event needle"}', 'public', 'hash-v4', NULL);
            INSERT INTO contexts(id, kind, namespace, title, summary, visibility, source_event_id, created_at, updated_at, schema_version, attrs_json)
                VALUES('context:event-v4', 'decision', 'graphify:v4', 'legacy context title', 'legacy context needle', 'public', 'event-v4', '2026-06-28T00:00:00.000Z', '2026-06-28T00:00:00.000Z', 1, '{}');
            INSERT INTO graph_nodes(id, kind, label, attrs_json, source_event_id, visibility)
                VALUES('graphify:v4:node-a', 'imported', 'legacy node needle', '{"namespace":"graphify:v4"}', 'event-v4', 'public');
            INSERT INTO graph_edges(id, from_id, to_id, kind, attrs_json, source_event_id, visibility)
                VALUES('edge-v4', 'graphify:v4:node-a', 'graphify:v4:node-b', 'references', '{"namespace":"graphify:v4","label":"legacy edge needle"}', 'event-v4', 'public');
            INSERT INTO skills(name, version, manifest_path, entry, permissions_json, description, source_event_id, visibility)
                VALUES('legacy-skill-needle', '0.1.0', 'skill.toml', './SKILL.md', '[]', 'legacy skill summary', 'event-v4', 'public');
            "#,
        )?;
        drop(conn);

        let meshlet = Meshlet::open(dir.path())?;
        let contexts = meshlet.search_contexts("context needle", 10, SafetyProfile::PublicSafe)?;
        let events = meshlet.query_scoped_view(
            "event needle",
            Some("events"),
            None,
            10,
            OutputMode::Compact,
            SafetyProfile::PublicSafe,
        )?;
        let nodes = meshlet.query_scoped("node needle", Some("nodes"), Some("graphify:v4"), 10)?;
        let edges = meshlet.query_scoped("edge needle", Some("edges"), Some("graphify:v4"), 10)?;
        let skills = meshlet.search_skills("skill summary", 10, SafetyProfile::PublicSafe)?;

        assert_eq!(contexts["contexts"]["items"][0]["id"], "context:event-v4");
        assert_eq!(events["events"]["items"][0]["id"], "event-v4");
        assert_eq!(nodes["nodes"]["items"][0]["id"], "graphify:v4:node-a");
        assert_eq!(edges["edges"]["items"][0]["id"], "edge-v4");
        assert_eq!(skills["skills"]["items"][0]["name"], "legacy-skill-needle");
        Ok(())
    }

    #[test]
    fn migration_from_v2_creates_and_backfills_contexts() -> Result<()> {
        let dir = tempdir()?;
        let db_dir = dir.path().join(DB_DIR);
        fs::create_dir_all(&db_dir)?;
        let db_path = db_dir.join(DB_FILE);
        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            r#"
            CREATE TABLE meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE events (
                id TEXT PRIMARY KEY,
                seq INTEGER NOT NULL UNIQUE,
                type TEXT NOT NULL,
                created_at TEXT NOT NULL,
                actor TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                visibility TEXT NOT NULL DEFAULT 'private',
                hash TEXT NOT NULL UNIQUE,
                prev_hash TEXT
            );
            CREATE TABLE graph_nodes (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                label TEXT,
                attrs_json TEXT NOT NULL,
                source_event_id TEXT NOT NULL
            );
            CREATE TABLE graph_edges (
                id TEXT PRIMARY KEY,
                from_id TEXT NOT NULL,
                to_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                attrs_json TEXT NOT NULL,
                source_event_id TEXT NOT NULL
            );
            CREATE TABLE skills (
                name TEXT PRIMARY KEY,
                version TEXT NOT NULL,
                manifest_path TEXT NOT NULL,
                entry TEXT NOT NULL,
                permissions_json TEXT NOT NULL,
                description TEXT,
                source_event_id TEXT NOT NULL
            );
            INSERT INTO meta(key, value) VALUES('schema_version', '2');
            "#,
        )?;
        let payload = json!({"label": "v2 context", "namespace": "legacy"});
        let created_at = "2026-06-28T00:00:00.000Z";
        let hash = event_hash(
            "v2-context",
            "context.added",
            created_at,
            "agent:test",
            &payload,
            EventVisibility::Private,
            None,
        )?;
        conn.execute(
            "INSERT INTO events(id, seq, type, created_at, actor, payload_json, visibility, hash, prev_hash)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                "v2-context",
                1_i64,
                "context.added",
                created_at,
                "agent:test",
                canonical_json(&payload)?,
                "public",
                hash,
                Option::<String>::None,
            ],
        )?;
        drop(conn);

        let meshlet = Meshlet::open(dir.path())?;
        let contexts = meshlet.list_contexts_limited(10, SafetyProfile::PublicSafe)?;

        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0]["id"], "context:v2-context");
        assert_eq!(contexts[0]["summary"], "v2 context");
        assert_eq!(contexts[0]["namespace"], "legacy");
        assert_eq!(contexts[0]["visibility"], "public");
        Ok(())
    }

    #[test]
    fn migration_from_v3_adds_visibility_columns_and_backfills_safely() -> Result<()> {
        let dir = tempdir()?;
        let db_dir = dir.path().join(DB_DIR);
        fs::create_dir_all(&db_dir)?;
        let conn = Connection::open(db_dir.join(DB_FILE))?;
        conn.execute_batch(
            r#"
            CREATE TABLE meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE events (
                id TEXT PRIMARY KEY,
                seq INTEGER NOT NULL UNIQUE,
                type TEXT NOT NULL,
                created_at TEXT NOT NULL,
                actor TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                visibility TEXT NOT NULL DEFAULT 'private',
                hash TEXT NOT NULL UNIQUE,
                prev_hash TEXT
            );
            CREATE TABLE graph_nodes (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                label TEXT,
                attrs_json TEXT NOT NULL,
                source_event_id TEXT NOT NULL
            );
            CREATE TABLE graph_edges (
                id TEXT PRIMARY KEY,
                from_id TEXT NOT NULL,
                to_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                attrs_json TEXT NOT NULL,
                source_event_id TEXT NOT NULL
            );
            CREATE TABLE skills (
                name TEXT PRIMARY KEY,
                version TEXT NOT NULL,
                manifest_path TEXT NOT NULL,
                entry TEXT NOT NULL,
                permissions_json TEXT NOT NULL,
                description TEXT,
                source_event_id TEXT NOT NULL
            );
            CREATE TABLE contexts (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                namespace TEXT,
                title TEXT,
                summary TEXT NOT NULL,
                visibility TEXT NOT NULL,
                source_event_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                schema_version INTEGER NOT NULL,
                attrs_json TEXT
            );
            INSERT INTO meta(key, value) VALUES('schema_version', '3');
            INSERT INTO events(id, seq, type, created_at, actor, payload_json, visibility, hash, prev_hash)
                VALUES('public-event', 1, 'context.added', '2026-06-28T00:00:00.000Z', 'agent:test', '{}', 'public', 'hash-public', NULL);
            INSERT INTO events(id, seq, type, created_at, actor, payload_json, visibility, hash, prev_hash)
                VALUES('private-event', 2, 'context.added', '2026-06-28T00:00:01.000Z', 'agent:test', '{}', 'private', 'hash-private', 'hash-public');
            INSERT INTO graph_nodes(id, kind, label, attrs_json, source_event_id)
                VALUES('node-from-event', 'context', 'event', '{"visibility":"private"}', 'public-event');
            INSERT INTO graph_nodes(id, kind, label, attrs_json, source_event_id)
                VALUES('node-from-attrs', 'context', 'attrs', '{"visibility":"public"}', 'missing-event');
            INSERT INTO graph_nodes(id, kind, label, attrs_json, source_event_id)
                VALUES('node-weird', 'context', 'weird', '{"visibility":"PUBLIC"}', 'missing-event');
            INSERT INTO graph_edges(id, from_id, to_id, kind, attrs_json, source_event_id)
                VALUES('edge-from-event', 'a', 'b', 'uses', '{"visibility":"public"}', 'private-event');
            INSERT INTO graph_edges(id, from_id, to_id, kind, attrs_json, source_event_id)
                VALUES('edge-from-attrs', 'a', 'c', 'uses', '{"visibility":"local"}', 'missing-event');
            INSERT INTO skills(name, version, manifest_path, entry, permissions_json, description, source_event_id)
                VALUES('public-skill', '0.1.0', 'public.toml', './SKILL.md', '[]', NULL, 'public-event');
            INSERT INTO skills(name, version, manifest_path, entry, permissions_json, description, source_event_id)
                VALUES('missing-skill', '0.1.0', 'missing.toml', './SKILL.md', '[]', NULL, 'missing-event');
            "#,
        )?;
        drop(conn);

        let meshlet = Meshlet::open(dir.path())?;
        let nodes = meshlet.graph_nodes_limited(None, 20)?;
        let edges = meshlet.graph_edges_limited(None, 20)?;
        let public_skill = meshlet.show_skill("public-skill")?;
        let missing_skill = meshlet.show_skill("missing-skill")?;

        assert_eq!(
            nodes
                .iter()
                .find(|node| node["id"] == "node-from-event")
                .expect("event node")["visibility"],
            "public"
        );
        assert_eq!(
            nodes
                .iter()
                .find(|node| node["id"] == "node-from-attrs")
                .expect("attrs node")["visibility"],
            "public"
        );
        assert_eq!(
            nodes
                .iter()
                .find(|node| node["id"] == "node-weird")
                .expect("weird node")["visibility"],
            "private"
        );
        assert_eq!(
            edges
                .iter()
                .find(|edge| edge["id"] == "edge-from-event")
                .expect("event edge")["visibility"],
            "private"
        );
        assert_eq!(
            edges
                .iter()
                .find(|edge| edge["id"] == "edge-from-attrs")
                .expect("attrs edge")["visibility"],
            "local"
        );
        assert_eq!(public_skill["visibility"], "public");
        assert_eq!(missing_skill["visibility"], "private");
        Ok(())
    }
}
