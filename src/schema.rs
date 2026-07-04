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
