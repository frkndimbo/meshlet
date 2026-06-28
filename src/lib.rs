use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

mod mcp;

#[cfg(test)]
use mcp::{handle_mcp_request, handle_mcp_request_with_profile};
pub use mcp::{run_mcp_stdio, run_mcp_stdio_with_profile};

pub const DB_DIR: &str = ".meshlet";
pub const DB_FILE: &str = "meshlet.db";
pub const DEFAULT_LIMIT: u32 = 20;
pub const MAX_LIMIT: u32 = 100;
const SCHEMA_VERSION: &str = "4";
const CONTEXT_SCHEMA_VERSION: i64 = 1;

const EVENT_TYPES: &[&str] = &[
    "repo.initialized",
    "skill.added",
    "context.added",
    "agent.message",
    "evidence.attached",
    "task.created",
    "task.updated",
    "graph.imported",
];

const SECRET_KEY_DENYLIST: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "apikey",
    "authorization",
    "accesstoken",
    "refreshtoken",
    "privatekey",
];

const ALLOWED_SKILL_PERMISSIONS: &[&str] = &["read_repo", "read_files", "write_docs", "run_check"];

const SECRET_VALUE_MARKERS: &[&str] = &[
    "bearer ",
    "begin private key",
    "begin rsa private key",
    "begin openSSH private key",
    "ghp_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "sk-",
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventVisibility {
    Private,
    Local,
    Public,
}

impl Default for EventVisibility {
    fn default() -> Self {
        Self::Private
    }
}

impl EventVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Local => "local",
            Self::Public => "public",
        }
    }

    fn from_db(value: Option<&str>) -> Self {
        match value {
            Some("local") => Self::Local,
            Some("public") => Self::Public,
            _ => Self::Private,
        }
    }
}

impl FromStr for EventVisibility {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "private" => Ok(Self::Private),
            "local" => Ok(Self::Local),
            "public" => Ok(Self::Public),
            _ => bail!("visibility must be private, local, or public"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    Compact,
    Full,
}

impl Default for OutputMode {
    fn default() -> Self {
        Self::Compact
    }
}

impl FromStr for OutputMode {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "compact" => Ok(Self::Compact),
            "full" => Ok(Self::Full),
            _ => bail!("mode must be compact or full"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SafetyProfile {
    LocalTrusted,
    PublicSafe,
}

impl Default for SafetyProfile {
    fn default() -> Self {
        Self::LocalTrusted
    }
}

impl SafetyProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalTrusted => "local-trusted",
            Self::PublicSafe => "public-safe",
        }
    }
}

impl FromStr for SafetyProfile {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "local-trusted" => Ok(Self::LocalTrusted),
            "public-safe" => Ok(Self::PublicSafe),
            _ => bail!("profile must be local-trusted or public-safe"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RedactionReport {
    pub blocked_keys: Vec<String>,
    pub suspicious_values: Vec<String>,
    pub output_redactions: u64,
}

impl RedactionReport {
    fn merge(&mut self, other: RedactionReport) {
        self.blocked_keys.extend(other.blocked_keys);
        self.suspicious_values.extend(other.suspicious_values);
        self.output_redactions += other.output_redactions;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Event {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub created_at: String,
    pub actor: String,
    #[serde(default)]
    pub visibility: EventVisibility,
    pub payload: Value,
    pub hash: String,
    pub prev_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationReport {
    pub ok: bool,
    pub events: u64,
    pub checked_until_seq: i64,
    pub first_invalid_seq: Option<i64>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SkillManifest {
    pub name: String,
    pub version: String,
    pub kind: String,
    pub entry: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
}

pub struct Meshlet {
    root: PathBuf,
    conn: Connection,
}

struct Bounded<T> {
    items: Vec<T>,
    truncated: bool,
}

impl Meshlet {
    pub fn init(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join(DB_DIR)).context("create .meshlet dir")?;
        let db_path = root.join(DB_DIR).join(DB_FILE);
        let meshlet = Self {
            root,
            conn: Connection::open(db_path).context("open sqlite database")?,
        };
        meshlet.apply_schema()?;
        if meshlet.event_count()? == 0 {
            meshlet.append_event(
                "repo.initialized",
                "cli",
                json!({ "root": meshlet.root.display().to_string() }),
            )?;
        }
        Ok(meshlet)
    }

    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let db_path = root.join(DB_DIR).join(DB_FILE);
        if !db_path.exists() {
            bail!("Meshlet DB not found. Run `meshlet init` first.");
        }
        let meshlet = Self {
            root,
            conn: Connection::open(db_path).context("open sqlite database")?,
        };
        meshlet.apply_schema()?;
        Ok(meshlet)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn apply_schema(&self) -> Result<()> {
        let had_contexts = self.has_table("contexts")?;
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
            CREATE INDEX IF NOT EXISTS idx_contexts_visibility_created_at
                ON contexts(visibility, created_at);
            CREATE INDEX IF NOT EXISTS idx_contexts_source_event_id
                ON contexts(source_event_id);
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
        if needs_read_model_visibility {
            self.backfill_read_model_visibility()?;
        }
        if !had_contexts {
            self.backfill_contexts_from_events()?;
        }
        self.conn.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES('schema_version', ?1)",
            [SCHEMA_VERSION],
        )?;
        Ok(())
    }

    fn ensure_read_model_visibility_columns(&self) -> Result<bool> {
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

    fn has_table(&self, table: &str) -> Result<bool> {
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

    fn has_column(&self, table: &str, column: &str) -> Result<bool> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(columns.iter().any(|name| name == column))
    }

    pub fn event_count(&self) -> Result<u64> {
        let count: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
        Ok(count)
    }

    pub fn verify_event_chain(&self) -> Result<VerificationReport> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, id, type, created_at, actor, payload_json, visibility, hash, prev_hash
             FROM events ORDER BY seq ASC",
        )?;
        let rows = stmt
            .query_map([], event_record_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut expected_prev_hash: Option<String> = None;
        let mut checked_until_seq = 0;

        for record in &rows {
            checked_until_seq = record.seq;
            if record.event.prev_hash != expected_prev_hash {
                return Ok(VerificationReport {
                    ok: false,
                    events: rows.len() as u64,
                    checked_until_seq,
                    first_invalid_seq: Some(record.seq),
                    reason: Some("prev_hash_mismatch".to_string()),
                });
            }
            let expected_hash = event_hash(
                &record.event.id,
                &record.event.event_type,
                &record.event.created_at,
                &record.event.actor,
                &record.event.payload,
                record.event.prev_hash.as_deref(),
            )?;
            if record.event.hash != expected_hash {
                return Ok(VerificationReport {
                    ok: false,
                    events: rows.len() as u64,
                    checked_until_seq,
                    first_invalid_seq: Some(record.seq),
                    reason: Some("hash_mismatch".to_string()),
                });
            }
            expected_prev_hash = Some(record.event.hash.clone());
        }

        Ok(VerificationReport {
            ok: true,
            events: rows.len() as u64,
            checked_until_seq,
            first_invalid_seq: None,
            reason: None,
        })
    }

    pub fn append_event(&self, event_type: &str, actor: &str, payload: Value) -> Result<Event> {
        self.append_event_with_options(
            event_type,
            actor,
            payload,
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )
    }

    pub fn append_event_with_options(
        &self,
        event_type: &str,
        actor: &str,
        payload: Value,
        visibility: EventVisibility,
        profile: SafetyProfile,
    ) -> Result<Event> {
        validate_event_type(event_type)?;
        if actor.trim().is_empty() {
            bail!("actor must not be empty");
        }
        validate_payload_safety(&payload, profile)?;
        validate_event_payload(event_type, &payload)?;
        let prev_hash = self.latest_hash()?;
        let id = Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let hash = event_hash(
            &id,
            event_type,
            &created_at,
            actor,
            &payload,
            prev_hash.as_deref(),
        )?;
        let next_seq = self.next_seq()?;
        self.conn.execute(
            "INSERT INTO events(id, seq, type, created_at, actor, payload_json, visibility, hash, prev_hash)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                next_seq,
                event_type,
                created_at,
                actor,
                canonical_json(&payload)?,
                visibility.as_str(),
                hash,
                prev_hash
            ],
        )?;
        let event = self
            .get_event_by_seq(next_seq)?
            .ok_or_else(|| anyhow!("inserted event not found"))?;
        self.apply_event(&event)?;
        Ok(event)
    }

    pub fn list_events(&self, limit: u32) -> Result<Vec<Event>> {
        let limit = clamp_limit(limit);
        let mut stmt = self.conn.prepare(
            "SELECT id, type, created_at, actor, payload_json, visibility, hash, prev_hash
             FROM events ORDER BY seq DESC LIMIT ?1",
        )?;
        let events = stmt
            .query_map([limit], event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(events)
    }

    fn list_events_bounded(&self, limit: u32) -> Result<Bounded<Event>> {
        let limit = clamp_limit(limit);
        let mut stmt = self.conn.prepare(
            "SELECT id, type, created_at, actor, payload_json, visibility, hash, prev_hash
             FROM events ORDER BY seq DESC LIMIT ?1",
        )?;
        let mut events = stmt
            .query_map([limit + 1], event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = events.len() > limit as usize;
        events.truncate(limit as usize);
        Ok(Bounded {
            items: events,
            truncated,
        })
    }

    pub fn show_event(&self, id: &str) -> Result<Event> {
        self.conn
            .query_row(
                "SELECT id, type, created_at, actor, payload_json, visibility, hash, prev_hash
                 FROM events WHERE id = ?1",
                [id],
                event_from_row,
            )
            .optional()?
            .ok_or_else(|| anyhow!("event not found: {id}"))
    }

    pub fn add_skill_manifest(&self, manifest_path: impl AsRef<Path>) -> Result<Event> {
        let manifest_path = manifest_path.as_ref();
        let text = fs::read_to_string(manifest_path)
            .with_context(|| format!("read skill manifest {}", manifest_path.display()))?;
        let manifest: SkillManifest = toml::from_str(&text).context("parse skill manifest TOML")?;
        validate_skill_manifest(&manifest)?;
        let payload = json!({
            "name": manifest.name,
            "version": manifest.version,
            "kind": manifest.kind,
            "manifest_path": manifest_path.display().to_string(),
            "entry": manifest.entry,
            "permissions": manifest.permissions,
            "description": manifest.description,
        });
        self.append_event("skill.added", "cli", payload)
    }

    pub fn list_skills(&self) -> Result<Vec<Value>> {
        self.list_skills_scoped(SafetyProfile::LocalTrusted)
    }

    pub fn list_skills_scoped(&self, profile: SafetyProfile) -> Result<Vec<Value>> {
        Ok(self.skills_bounded(None, profile)?.items)
    }

    pub fn list_skills_limited(&self, limit: u32, profile: SafetyProfile) -> Result<Vec<Value>> {
        Ok(self.skills_bounded(Some(limit), profile)?.items)
    }

    fn skills_bounded(&self, limit: Option<u32>, profile: SafetyProfile) -> Result<Bounded<Value>> {
        let visibility = visibility_clause(profile);
        let limit = limit.map(clamp_limit);
        let sql = if limit.is_some() {
            format!(
                "SELECT name, version, manifest_path, entry, permissions_json, description, source_event_id, visibility
                 FROM skills
                 WHERE {visibility}
                 ORDER BY name
                 LIMIT ?1"
            )
        } else {
            format!(
                "SELECT name, version, manifest_path, entry, permissions_json, description, source_event_id, visibility
                 FROM skills
                 WHERE {visibility}
                 ORDER BY name"
            )
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut skills = if let Some(limit) = limit {
            stmt.query_map([limit + 1], skill_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map([], skill_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let truncated = limit.is_some_and(|limit| skills.len() > limit as usize);
        if let Some(limit) = limit {
            skills.truncate(limit as usize);
        }
        Ok(Bounded {
            items: skills,
            truncated,
        })
    }

    pub fn show_skill(&self, name: &str) -> Result<Value> {
        self.conn
            .query_row(
                "SELECT name, version, manifest_path, entry, permissions_json, description, source_event_id, visibility
                 FROM skills WHERE name = ?1",
                [name],
                skill_from_row,
            )
            .optional()?
            .ok_or_else(|| anyhow!("skill not found: {name}"))
    }

    pub fn list_tasks(&self, limit: u32) -> Result<Vec<Value>> {
        let limit = clamp_limit(limit) as usize;
        let mut tasks = self.task_views()?;
        tasks.truncate(limit);
        Ok(tasks)
    }

    pub fn show_task(&self, id: &str) -> Result<Value> {
        self.task_views()?
            .into_iter()
            .find(|task| task["id"] == id)
            .ok_or_else(|| anyhow!("task not found: {id}"))
    }

    pub fn list_evidence(&self, limit: u32) -> Result<Vec<Value>> {
        self.graph_nodes_limited(Some("evidence"), limit)
    }

    pub fn show_evidence(&self, id: &str) -> Result<Value> {
        let node_id = if id.starts_with("evidence:") {
            id.to_string()
        } else {
            format!("evidence:{id}")
        };
        self.conn
            .query_row(
                "SELECT id, kind, label, attrs_json, source_event_id, visibility
                 FROM graph_nodes WHERE id = ?1 AND kind = 'evidence'",
                [node_id.as_str()],
                node_from_row,
            )
            .optional()?
            .ok_or_else(|| anyhow!("evidence not found: {id}"))
    }

    pub fn attach_evidence_file(
        &self,
        path: impl AsRef<Path>,
        task_id: Option<&str>,
        sha256: Option<&str>,
    ) -> Result<Value> {
        let path = path.as_ref();
        let bytes = fs::read(path).with_context(|| format!("read evidence {}", path.display()))?;
        let digest = match sha256 {
            Some("auto") | None => sha256_hex(&bytes),
            Some(value) if value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit()) => {
                value.to_ascii_lowercase()
            }
            Some(_) => bail!("sha256 must be `auto` or a 64-character hex digest"),
        };
        let mut payload = json!({
            "path": path.display().to_string(),
            "sha256": digest,
        });
        if let Some(task_id) = task_id {
            payload["task_id"] = json!(task_id);
        }
        let event = self.append_event("evidence.attached", "cli", payload)?;
        Ok(json!({
            "event_id": event.id,
            "evidence_id": format!("evidence:{}", event.id),
            "path": path.display().to_string(),
            "sha256": digest,
        }))
    }

    pub fn verify_evidence(&self, id: &str) -> Result<Value> {
        let evidence = self.show_evidence(id)?;
        let attrs = evidence
            .get("attrs")
            .and_then(Value::as_object)
            .ok_or_else(|| anyhow!("evidence attrs missing"))?;
        let path = attrs
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("evidence path missing"))?;
        let expected = attrs
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("evidence sha256 missing"))?;
        let bytes = fs::read(path).with_context(|| format!("read evidence {path}"))?;
        let actual = sha256_hex(&bytes);
        Ok(json!({
            "evidence_id": evidence["id"],
            "path": path,
            "ok": actual == expected,
            "expected_sha256": expected,
            "actual_sha256": actual,
        }))
    }

    pub fn rebuild_graph(&self) -> Result<()> {
        self.rebuild_read_models()
    }

    fn rebuild_read_models(&self) -> Result<()> {
        self.conn.execute("DELETE FROM contexts", [])?;
        self.conn.execute("DELETE FROM graph_edges", [])?;
        self.conn.execute("DELETE FROM graph_nodes", [])?;
        self.conn.execute("DELETE FROM skills", [])?;
        let events = self.events_ascending()?;
        for event in events {
            self.apply_event(&event)?;
        }
        Ok(())
    }

    pub fn list_contexts_limited(&self, limit: u32, profile: SafetyProfile) -> Result<Vec<Value>> {
        Ok(self.contexts_bounded(None, limit, profile)?.items)
    }

    pub fn search_contexts(&self, q: &str, limit: u32, profile: SafetyProfile) -> Result<Value> {
        let needle = q.trim().to_lowercase();
        if needle.is_empty() {
            bail!("query must not be empty");
        }
        let bounded = self.contexts_bounded(Some(&needle), limit, profile)?;
        Ok(json!({
            "q": q,
            "limit": clamp_limit(limit),
            "contexts": {
                "items": bounded.items,
                "truncated": bounded.truncated,
            },
        }))
    }

    fn contexts_bounded(
        &self,
        needle: Option<&str>,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Bounded<Value>> {
        let limit = clamp_limit(limit);
        let visibility = visibility_clause(profile);
        let sql = if needle.is_some() {
            format!(
                "SELECT id, kind, namespace, title, summary, visibility, source_event_id, created_at, updated_at, schema_version, attrs_json
                 FROM contexts
                 WHERE {visibility}
                   AND (instr(lower(kind), ?1) > 0
                    OR instr(lower(coalesce(namespace, '')), ?1) > 0
                    OR instr(lower(coalesce(title, '')), ?1) > 0
                    OR instr(lower(summary), ?1) > 0)
                 ORDER BY created_at DESC, id
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
        let mut contexts = if let Some(needle) = needle {
            stmt.query_map(params![needle, limit + 1], context_from_row)?
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

    fn graph_nodes_bounded(
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
        let mut namespaces = BTreeSet::new();
        let mut stmt = self.conn.prepare(
            "SELECT attrs_json FROM graph_nodes UNION ALL SELECT attrs_json FROM graph_edges",
        )?;
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

    pub fn context_digest_limited(&self, limit: u32, profile: SafetyProfile) -> Result<Value> {
        let limit = clamp_limit(limit);
        let events = self.list_events_bounded(limit)?;
        let event_items = events
            .items
            .iter()
            .filter(|event| {
                profile != SafetyProfile::PublicSafe || event.visibility == EventVisibility::Public
            })
            .map(compact_event)
            .collect::<Vec<_>>();
        let nodes = self.graph_nodes_bounded(None, limit, profile)?;
        let edges = self.graph_edges_bounded(None, limit, profile)?;
        Ok(json!({
            "root": self.root.display().to_string(),
            "profile": match profile {
                SafetyProfile::LocalTrusted => "local-trusted",
                SafetyProfile::PublicSafe => "public-safe",
            },
            "limit": limit,
            "counts": {
                "events": self.event_count()?,
                "skills": self.list_skills_scoped(profile)?.len(),
                "tasks": self.list_tasks(limit)?.len(),
                "namespaces": self.graph_namespaces()?.len(),
            },
            "events_recent": {
                "items": event_items,
                "truncated": events.truncated,
            },
            "tasks": self.list_tasks(limit)?,
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

    fn graph_edges_bounded(
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

    fn query_scoped_with_profile(
        &self,
        q: &str,
        kind: Option<&str>,
        namespace: Option<&str>,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Value> {
        let needle = q.trim().to_lowercase();
        if needle.is_empty() {
            bail!("query must not be empty");
        }
        if namespace.is_some_and(|value| value.trim().is_empty()) {
            bail!("namespace must not be empty");
        }
        let kind = kind.unwrap_or("all");
        if !matches!(kind, "all" | "events" | "nodes" | "edges") {
            bail!("query kind must be all, events, nodes, or edges");
        }
        let limit = clamp_limit(limit);
        let events = if matches!(kind, "all" | "events") {
            Some(self.query_events(&needle, limit)?)
        } else {
            None
        };
        let nodes = if matches!(kind, "all" | "nodes") {
            Some(self.query_nodes(&needle, namespace, limit, profile)?)
        } else {
            None
        };
        let edges = if matches!(kind, "all" | "edges") {
            Some(self.query_edges(&needle, namespace, limit, profile)?)
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

    pub fn public_export(&self, limit: u32) -> Result<Value> {
        let limit = clamp_limit(limit);
        let events = self
            .list_events(limit)?
            .into_iter()
            .filter(|event| event.visibility == EventVisibility::Public)
            .map(|event| compact_event(&event))
            .collect::<Vec<_>>();
        let nodes = filter_public_values(
            self.graph_nodes_limited(None, limit)?,
            SafetyProfile::PublicSafe,
        )
        .into_iter()
        .map(compact_node)
        .collect::<Vec<_>>();
        let edges = filter_public_values(
            self.graph_edges_limited(None, limit)?,
            SafetyProfile::PublicSafe,
        )
        .into_iter()
        .map(compact_edge)
        .collect::<Vec<_>>();
        Ok(json!({
            "format": "meshlet-public-export-v1",
            "profile": "public-safe",
            "limit": limit,
            "events": events,
            "graph": {
                "nodes": nodes,
                "edges": edges,
            },
            "redaction_report": self.public_doctor()?["redaction_report"].clone(),
        }))
    }

    pub fn public_doctor(&self) -> Result<Value> {
        let chain = self.verify_event_chain()?;
        let events = self.events_ascending()?;
        let mut report = RedactionReport::default();
        let mut visibility_counts: BTreeMap<String, u64> = BTreeMap::new();
        for event in &events {
            *visibility_counts
                .entry(event.visibility.as_str().to_string())
                .or_insert(0) += 1;
            report.merge(scan_payload_safety(&event.payload));
        }
        for node in self.graph_nodes_limited(None, MAX_LIMIT)? {
            if let Some(attrs) = node.get("attrs") {
                report.merge(scan_payload_safety(attrs));
            }
        }
        for edge in self.graph_edges_limited(None, MAX_LIMIT)? {
            if let Some(attrs) = edge.get("attrs") {
                report.merge(scan_payload_safety(attrs));
            }
        }
        let ok = chain.ok && report.blocked_keys.is_empty() && report.suspicious_values.is_empty();
        Ok(json!({
            "ok": ok,
            "chain": chain,
            "events": events.len(),
            "visibility": visibility_counts,
            "redaction_report": report,
            "blockers": if ok { json!([]) } else { json!(["stored state is not public-safe"]) },
        }))
    }

    fn query_events(&self, needle: &str, limit: u32) -> Result<Bounded<Event>> {
        let limit = clamp_limit(limit);
        let mut stmt = self.conn.prepare(
            "SELECT id, type, created_at, actor, payload_json, visibility, hash, prev_hash
             FROM events
             WHERE instr(lower(type), ?1) > 0
                OR instr(lower(actor), ?1) > 0
                OR instr(lower(payload_json), ?1) > 0
             ORDER BY seq DESC
             LIMIT ?2",
        )?;
        let mut events = stmt
            .query_map(params![needle, limit + 1], event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = events.len() > limit as usize;
        events.truncate(limit as usize);
        Ok(Bounded {
            items: events,
            truncated,
        })
    }

    fn query_nodes(
        &self,
        needle: &str,
        namespace: Option<&str>,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Bounded<Value>> {
        let limit = clamp_limit(limit);
        let visibility = visibility_clause(profile);
        let namespace_filter = namespace.map(namespace_json_fragment).transpose()?;
        let sql = if namespace_filter.is_some() {
            format!(
                "SELECT id, kind, label, attrs_json, source_event_id, visibility
             FROM graph_nodes
             WHERE {visibility}
               AND (instr(lower(id), ?1) > 0
                OR instr(lower(kind), ?1) > 0
                OR instr(lower(coalesce(label, '')), ?1) > 0
                OR instr(lower(attrs_json), ?1) > 0)
               AND instr(attrs_json, ?2) > 0
             ORDER BY id
             LIMIT ?3"
            )
        } else {
            format!(
                "SELECT id, kind, label, attrs_json, source_event_id, visibility
             FROM graph_nodes
             WHERE {visibility}
               AND (instr(lower(id), ?1) > 0
                OR instr(lower(kind), ?1) > 0
                OR instr(lower(coalesce(label, '')), ?1) > 0
                OR instr(lower(attrs_json), ?1) > 0)
             ORDER BY id
             LIMIT ?2"
            )
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut nodes = if let Some(filter) = namespace_filter {
            stmt.query_map(params![needle, filter, limit + 1], node_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map(params![needle, limit + 1], node_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let truncated = nodes.len() > limit as usize;
        nodes.truncate(limit as usize);
        Ok(Bounded {
            items: nodes,
            truncated,
        })
    }

    fn query_edges(
        &self,
        needle: &str,
        namespace: Option<&str>,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Bounded<Value>> {
        let limit = clamp_limit(limit);
        let visibility = visibility_clause(profile);
        let namespace_filter = namespace.map(namespace_json_fragment).transpose()?;
        let sql = if namespace_filter.is_some() {
            format!(
                "SELECT id, from_id, to_id, kind, attrs_json, source_event_id, visibility
             FROM graph_edges
             WHERE {visibility}
               AND (instr(lower(id), ?1) > 0
                OR instr(lower(from_id), ?1) > 0
                OR instr(lower(to_id), ?1) > 0
                OR instr(lower(kind), ?1) > 0
                OR instr(lower(attrs_json), ?1) > 0)
               AND instr(attrs_json, ?2) > 0
             ORDER BY id
             LIMIT ?3"
            )
        } else {
            format!(
                "SELECT id, from_id, to_id, kind, attrs_json, source_event_id, visibility
             FROM graph_edges
             WHERE {visibility}
               AND (instr(lower(id), ?1) > 0
                OR instr(lower(from_id), ?1) > 0
                OR instr(lower(to_id), ?1) > 0
                OR instr(lower(kind), ?1) > 0
                OR instr(lower(attrs_json), ?1) > 0)
             ORDER BY id
             LIMIT ?2"
            )
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut edges = if let Some(filter) = namespace_filter {
            stmt.query_map(params![needle, filter, limit + 1], edge_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map(params![needle, limit + 1], edge_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let truncated = edges.len() > limit as usize;
        edges.truncate(limit as usize);
        Ok(Bounded {
            items: edges,
            truncated,
        })
    }

    fn task_views(&self) -> Result<Vec<Value>> {
        let mut tasks: BTreeMap<String, serde_json::Map<String, Value>> = BTreeMap::new();
        for event in self.events_ascending()? {
            match event.event_type.as_str() {
                "task.created" => {
                    let id = event
                        .payload
                        .get("task_id")
                        .and_then(Value::as_str)
                        .unwrap_or(&event.id)
                        .to_string();
                    let task = tasks.entry(id.clone()).or_default();
                    task.entry("id").or_insert_with(|| json!(id));
                    task.entry("created_event_id")
                        .or_insert_with(|| json!(event.id.clone()));
                    task.entry("created_at")
                        .or_insert_with(|| json!(event.created_at.clone()));
                    merge_task_payload(task, &event);
                }
                "task.updated" => {
                    let Some(id) = event.payload.get("task_id").and_then(Value::as_str) else {
                        continue;
                    };
                    let task = tasks.entry(id.to_string()).or_default();
                    task.entry("id").or_insert_with(|| json!(id));
                    merge_task_payload(task, &event);
                }
                _ => {}
            }
        }
        Ok(tasks.into_values().map(Value::Object).collect())
    }

    fn next_seq(&self) -> Result<i64> {
        let seq: i64 =
            self.conn
                .query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM events", [], |row| {
                    row.get(0)
                })?;
        Ok(seq)
    }

    fn latest_hash(&self) -> Result<Option<String>> {
        let hash = self
            .conn
            .query_row(
                "SELECT hash FROM events ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(hash)
    }

    fn get_event_by_seq(&self, seq: i64) -> Result<Option<Event>> {
        let event = self
            .conn
            .query_row(
                "SELECT id, type, created_at, actor, payload_json, visibility, hash, prev_hash
                 FROM events WHERE seq = ?1",
                [seq],
                event_from_row,
            )
            .optional()?;
        Ok(event)
    }

    fn events_ascending(&self) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, type, created_at, actor, payload_json, visibility, hash, prev_hash
             FROM events ORDER BY seq ASC",
        )?;
        let events = stmt
            .query_map([], event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(events)
    }

    fn apply_event(&self, event: &Event) -> Result<()> {
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

    fn apply_context_added(&self, event: &Event) -> Result<()> {
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
        Ok(())
    }

    fn apply_skill_added(&self, event: &Event) -> Result<()> {
        let name = str_field(&event.payload, "name")?;
        let version = str_field(&event.payload, "version")?;
        let manifest_path = str_field(&event.payload, "manifest_path")?;
        let entry = str_field(&event.payload, "entry")?;
        let permissions = event
            .payload
            .get("permissions")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let description = event.payload.get("description").and_then(Value::as_str);
        self.conn.execute(
            "INSERT OR REPLACE INTO skills(name, version, manifest_path, entry, permissions_json, description, source_event_id, visibility)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                name,
                version,
                manifest_path,
                entry,
                canonical_json(&permissions)?,
                description,
                event.id,
                event.visibility.as_str(),
            ],
        )?;
        let skill_id = format!("skill:{name}");
        self.upsert_node(
            &skill_id,
            "skill",
            Some(name),
            json!({
                "name": name,
                "version": version,
                "permissions": permissions,
                "description": description,
            }),
            &event.id,
            event.visibility,
        )?;
        let file_id = format!("file:{entry}");
        self.upsert_node(
            &file_id,
            "file",
            Some(entry),
            json!({ "path": entry }),
            &event.id,
            event.visibility,
        )?;
        self.upsert_edge(
            &format!("edge:{}:skill-entry", event.id),
            &skill_id,
            &file_id,
            "references",
            json!({ "field": "entry" }),
            &event.id,
            event.visibility,
        )?;
        Ok(())
    }

    fn apply_agent_message(&self, event: &Event) -> Result<()> {
        let agent = event
            .payload
            .get("agent")
            .and_then(Value::as_str)
            .unwrap_or(&event.actor);
        let agent_id = format!("agent:{agent}");
        let message_id = format!("message:{}", event.id);
        self.upsert_node(
            &agent_id,
            "agent",
            Some(agent),
            json!({ "name": agent }),
            &event.id,
            event.visibility,
        )?;
        self.upsert_node(
            &message_id,
            "message",
            event.payload.get("summary").and_then(Value::as_str),
            attrs_with_visibility(event.payload.clone(), event.visibility),
            &event.id,
            event.visibility,
        )?;
        self.upsert_edge(
            &format!("edge:{}:agent-message", event.id),
            &agent_id,
            &message_id,
            "produced",
            json!({}),
            &event.id,
            event.visibility,
        )?;
        Ok(())
    }

    fn apply_evidence(&self, event: &Event) -> Result<()> {
        let path = event
            .payload
            .get("path")
            .and_then(Value::as_str)
            .or_else(|| event.payload.get("ref").and_then(Value::as_str))
            .ok_or_else(|| anyhow!("evidence.attached requires path or ref"))?;
        let evidence_id = format!("evidence:{}", event.id);
        let file_id = format!("file:{path}");
        self.upsert_node(
            &evidence_id,
            "evidence",
            event.payload.get("note").and_then(Value::as_str),
            attrs_with_visibility(event.payload.clone(), event.visibility),
            &event.id,
            event.visibility,
        )?;
        self.upsert_node(
            &file_id,
            "file",
            Some(path),
            json!({ "path": path }),
            &event.id,
            event.visibility,
        )?;
        self.upsert_edge(
            &format!("edge:{}:evidence-file", event.id),
            &evidence_id,
            &file_id,
            "references",
            json!({}),
            &event.id,
            event.visibility,
        )?;
        if let Some(task_id) = event.payload.get("task_id").and_then(Value::as_str) {
            self.upsert_edge(
                &format!("edge:{}:evidence-task", event.id),
                &evidence_id,
                &format!("task:{task_id}"),
                "supports",
                json!({}),
                &event.id,
                event.visibility,
            )?;
        }
        Ok(())
    }

    fn apply_task(&self, event: &Event) -> Result<()> {
        let task_id = event
            .payload
            .get("task_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| event.id.clone());
        self.upsert_node(
            &format!("task:{task_id}"),
            "task",
            event.payload.get("title").and_then(Value::as_str),
            attrs_with_visibility(event.payload.clone(), event.visibility),
            &event.id,
            event.visibility,
        )?;
        Ok(())
    }

    fn apply_graph_imported(&self, event: &Event) -> Result<()> {
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

    fn upsert_node(
        &self,
        id: &str,
        kind: &str,
        label: Option<&str>,
        attrs: Value,
        source_event_id: &str,
        visibility: EventVisibility,
    ) -> Result<()> {
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
        Ok(())
    }

    fn upsert_edge(
        &self,
        id: &str,
        from_id: &str,
        to_id: &str,
        kind: &str,
        attrs: Value,
        source_event_id: &str,
        visibility: EventVisibility,
    ) -> Result<()> {
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
        Ok(())
    }

    fn backfill_contexts_from_events(&self) -> Result<()> {
        for event in self.events_ascending()? {
            if event.event_type == "context.added" {
                self.apply_context_added(&event)?;
            }
        }
        Ok(())
    }

    fn backfill_read_model_visibility(&self) -> Result<()> {
        let event_visibility = self.event_visibility_by_id()?;
        self.backfill_graph_visibility("graph_nodes", "id", &event_visibility)?;
        self.backfill_graph_visibility("graph_edges", "id", &event_visibility)?;
        self.backfill_skill_visibility(&event_visibility)?;
        Ok(())
    }

    fn event_visibility_by_id(&self) -> Result<BTreeMap<String, String>> {
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

    fn backfill_graph_visibility(
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

    fn backfill_skill_visibility(&self, event_visibility: &BTreeMap<String, String>) -> Result<()> {
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

pub fn find_project_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("current dir")?;
    for dir in cwd.ancestors() {
        if dir.join(DB_DIR).join(DB_FILE).exists() {
            return Ok(dir.to_path_buf());
        }
    }
    Ok(cwd)
}

pub fn parse_json_arg(input: &str) -> Result<Value> {
    let path = Path::new(input);
    let text = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("read JSON {}", path.display()))?
    } else {
        input.to_string()
    };
    let value: Value = serde_json::from_str(&text).context("parse JSON payload")?;
    Ok(value)
}

pub fn parse_visibility_arg(input: &str) -> Result<EventVisibility> {
    EventVisibility::from_str(input)
}

pub fn parse_output_mode_arg(input: &str) -> Result<OutputMode> {
    OutputMode::from_str(input)
}

pub fn parse_safety_profile_arg(input: &str) -> Result<SafetyProfile> {
    SafetyProfile::from_str(input)
}

fn validate_event_type(event_type: &str) -> Result<()> {
    if EVENT_TYPES.contains(&event_type) {
        Ok(())
    } else {
        bail!("unsupported event type: {event_type}")
    }
}

fn validate_payload_safety(value: &Value, profile: SafetyProfile) -> Result<()> {
    let report = scan_payload_safety(value);
    if let Some(path) = report.blocked_keys.first() {
        bail!("event payload contains prohibited secret key: {path}");
    }
    if profile == SafetyProfile::PublicSafe {
        if let Some(path) = report.suspicious_values.first() {
            bail!("event payload contains suspicious secret-looking value at: {path}");
        }
    }
    Ok(())
}

fn validate_skill_manifest(manifest: &SkillManifest) -> Result<()> {
    if manifest.name.trim().is_empty() {
        bail!("skill name must not be empty");
    }
    if manifest.version.trim().is_empty() {
        bail!("skill version must not be empty");
    }
    if manifest.kind != "skill" {
        bail!("skill manifest kind must be `skill`");
    }
    if manifest.entry.trim().is_empty() {
        bail!("skill entry must not be empty");
    }
    let entry = Path::new(&manifest.entry);
    if entry.is_absolute() {
        bail!("skill entry must be relative");
    }
    if entry
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("skill entry must not contain parent directory components");
    }
    for permission in &manifest.permissions {
        if !ALLOWED_SKILL_PERMISSIONS.contains(&permission.as_str()) {
            bail!("unsupported skill permission: {permission}");
        }
    }
    Ok(())
}

fn validate_event_payload(event_type: &str, payload: &Value) -> Result<()> {
    match event_type {
        "task.created" => {
            required_nonempty_string(payload, "title")?;
        }
        "task.updated" => {
            required_nonempty_string(payload, "task_id")?;
        }
        "evidence.attached" => {
            if nonempty_string(payload, "path").is_none()
                && nonempty_string(payload, "ref").is_none()
            {
                bail!("evidence.attached requires path or ref");
            }
            if payload.get("task_id").is_some() {
                required_nonempty_string(payload, "task_id")?;
            }
        }
        "graph.imported" => {
            required_nonempty_string(payload, "source")?;
            required_nonempty_string(payload, "namespace")?;
            let Some(nodes) = payload.get("nodes").and_then(Value::as_array) else {
                bail!("graph.imported nodes must be an array");
            };
            let Some(links) = payload.get("links").and_then(Value::as_array) else {
                bail!("graph.imported links must be an array");
            };
            for node in nodes {
                required_nonempty_string(node, "id")?;
            }
            for link in links {
                required_nonempty_string(link, "source")?;
                required_nonempty_string(link, "target")?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn scan_payload_safety(value: &Value) -> RedactionReport {
    let mut report = RedactionReport::default();
    scan_payload_safety_at(value, "payload", &mut report);
    report
}

fn scan_payload_safety_at(value: &Value, path: &str, report: &mut RedactionReport) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let child_path = format!("{path}.{key}");
                if SECRET_KEY_DENYLIST.contains(&normalize_key(key).as_str()) {
                    report.blocked_keys.push(child_path.clone());
                }
                scan_payload_safety_at(child, &child_path, report);
            }
        }
        Value::Array(items) => {
            for item in items {
                scan_payload_safety_at(item, &format!("{path}[]"), report);
            }
        }
        Value::String(value) => {
            let lowered = value.to_lowercase();
            if SECRET_VALUE_MARKERS
                .iter()
                .any(|marker| lowered.contains(&marker.to_lowercase()))
            {
                report.suspicious_values.push(path.to_string());
            }
        }
        _ => {}
    }
}

fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn str_field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing string field: {key}"))
}

fn required_nonempty_string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    nonempty_string(value, key).ok_or_else(|| anyhow!("missing non-empty string field: {key}"))
}

fn nonempty_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
}

fn merge_task_payload(task: &mut serde_json::Map<String, Value>, event: &Event) {
    for key in ["title", "status", "note"] {
        if let Some(value) = event.payload.get(key) {
            task.insert(key.to_string(), value.clone());
        }
    }
    task.insert("updated_event_id".to_string(), json!(event.id));
    task.insert("updated_at".to_string(), json!(event.created_at));
}

fn attrs_with_visibility(attrs: Value, visibility: EventVisibility) -> Value {
    match attrs {
        Value::Object(mut map) => {
            map.insert("visibility".to_string(), json!(visibility.as_str()));
            Value::Object(map)
        }
        other => json!({
            "value": other,
            "visibility": visibility.as_str(),
        }),
    }
}

fn visibility_clause(profile: SafetyProfile) -> &'static str {
    match profile {
        SafetyProfile::PublicSafe => "visibility = 'public'",
        SafetyProfile::LocalTrusted => "visibility IN ('private', 'local', 'public')",
    }
}

fn attrs_visibility(attrs_json: &str) -> Option<&'static str> {
    serde_json::from_str::<Value>(attrs_json)
        .ok()
        .and_then(|attrs| {
            attrs
                .get("visibility")
                .and_then(Value::as_str)
                .and_then(strict_visibility)
        })
}

fn strict_visibility(value: &str) -> Option<&'static str> {
    match value {
        "private" => Some("private"),
        "local" => Some("local"),
        "public" => Some("public"),
        _ => None,
    }
}

fn compact_event(event: &Event) -> Value {
    let label = event
        .payload
        .get("label")
        .or_else(|| event.payload.get("title"))
        .or_else(|| event.payload.get("status"))
        .cloned();
    json!({
        "id": event.id,
        "type": event.event_type,
        "created_at": event.created_at,
        "actor": event.actor,
        "visibility": event.visibility.as_str(),
        "label": label,
        "hash": event.hash,
    })
}

fn compact_node(node: Value) -> Value {
    json!({
        "id": node.get("id").cloned().unwrap_or(Value::Null),
        "kind": node.get("kind").cloned().unwrap_or(Value::Null),
        "label": node.get("label").cloned().unwrap_or(Value::Null),
        "visibility": node
            .get("visibility")
            .or_else(|| node.get("attrs")
            .and_then(|attrs| attrs.get("visibility"))
            )
            .cloned()
            .unwrap_or_else(|| json!("private")),
        "source_event_id": node.get("source_event_id").cloned().unwrap_or(Value::Null),
    })
}

fn compact_edge(edge: Value) -> Value {
    json!({
        "id": edge.get("id").cloned().unwrap_or(Value::Null),
        "from_id": edge.get("from_id").cloned().unwrap_or(Value::Null),
        "to_id": edge.get("to_id").cloned().unwrap_or(Value::Null),
        "kind": edge.get("kind").cloned().unwrap_or(Value::Null),
        "visibility": edge
            .get("visibility")
            .or_else(|| edge.get("attrs")
            .and_then(|attrs| attrs.get("visibility"))
            )
            .cloned()
            .unwrap_or_else(|| json!("private")),
        "source_event_id": edge.get("source_event_id").cloned().unwrap_or(Value::Null),
    })
}

fn compact_query(mut value: Value, profile: SafetyProfile) -> Value {
    for key in ["events", "nodes", "edges"] {
        let Some(section) = value.get_mut(key).and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(items) = section.get_mut("items").and_then(Value::as_array_mut) else {
            continue;
        };
        let compacted = items
            .drain(..)
            .filter_map(|item| match key {
                "events" => serde_json::from_value::<Event>(item)
                    .ok()
                    .and_then(|event| {
                        if profile != SafetyProfile::PublicSafe
                            || event.visibility == EventVisibility::Public
                        {
                            Some(compact_event(&event))
                        } else {
                            None
                        }
                    }),
                "nodes" => {
                    if profile != SafetyProfile::PublicSafe || value_is_public(&item) {
                        Some(compact_node(item))
                    } else {
                        None
                    }
                }
                "edges" => {
                    if profile != SafetyProfile::PublicSafe || value_is_public(&item) {
                        Some(compact_edge(item))
                    } else {
                        None
                    }
                }
                _ => Some(item),
            })
            .collect::<Vec<_>>();
        *items = compacted;
    }
    value
}

fn filter_public_values(values: Vec<Value>, profile: SafetyProfile) -> Vec<Value> {
    if profile != SafetyProfile::PublicSafe {
        return values;
    }
    values.into_iter().filter(value_is_public).collect()
}

fn value_is_public(value: &Value) -> bool {
    value
        .get("visibility")
        .or_else(|| value.get("attrs").and_then(|attrs| attrs.get("visibility")))
        .and_then(Value::as_str)
        == Some("public")
}

fn imported_node_id(namespace: &str, external_id: &str) -> String {
    format!("{namespace}:{external_id}")
}

fn graph_import_edge_kind(relation: &str) -> &str {
    match relation {
        "references" | "produced" | "supports" | "depends_on" | "uses" | "derived_from"
        | "updates" => relation,
        _ => "references",
    }
}

fn namespace_json_fragment(namespace: &str) -> Result<String> {
    Ok(format!(
        "\"namespace\":{}",
        serde_json::to_string(namespace)?
    ))
}

fn clamp_limit(limit: u32) -> u32 {
    limit.clamp(1, MAX_LIMIT)
}

fn canonical_json(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn event_hash(
    id: &str,
    event_type: &str,
    created_at: &str,
    actor: &str,
    payload: &Value,
    prev_hash: Option<&str>,
) -> Result<String> {
    let body = json!({
        "id": id,
        "type": event_type,
        "created_at": created_at,
        "actor": actor,
        "payload": payload,
        "prev_hash": prev_hash,
    });
    let mut hasher = Sha256::new();
    hasher.update(canonical_json(&body)?.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    let payload_json: String = row.get(4)?;
    let payload = serde_json::from_str(&payload_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(Event {
        id: row.get(0)?,
        event_type: row.get(1)?,
        created_at: row.get(2)?,
        actor: row.get(3)?,
        payload,
        visibility: EventVisibility::from_db(row.get::<_, Option<String>>(5)?.as_deref()),
        hash: row.get(6)?,
        prev_hash: row.get(7)?,
    })
}

struct EventRecord {
    seq: i64,
    event: Event,
}

fn event_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EventRecord> {
    let payload_json: String = row.get(5)?;
    let payload = serde_json::from_str(&payload_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(EventRecord {
        seq: row.get(0)?,
        event: Event {
            id: row.get(1)?,
            event_type: row.get(2)?,
            created_at: row.get(3)?,
            actor: row.get(4)?,
            payload,
            visibility: EventVisibility::from_db(row.get::<_, Option<String>>(6)?.as_deref()),
            hash: row.get(7)?,
            prev_hash: row.get(8)?,
        },
    })
}

fn node_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let attrs_json: String = row.get(3)?;
    Ok(json!({
        "id": row.get::<_, String>(0)?,
        "kind": row.get::<_, String>(1)?,
        "label": row.get::<_, Option<String>>(2)?,
        "attrs": serde_json::from_str::<Value>(&attrs_json).unwrap_or_else(|_| json!({})),
        "source_event_id": row.get::<_, String>(4)?,
        "visibility": row.get::<_, String>(5)?,
    }))
}

fn edge_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let attrs_json: String = row.get(4)?;
    Ok(json!({
        "id": row.get::<_, String>(0)?,
        "from_id": row.get::<_, String>(1)?,
        "to_id": row.get::<_, String>(2)?,
        "kind": row.get::<_, String>(3)?,
        "attrs": serde_json::from_str::<Value>(&attrs_json).unwrap_or_else(|_| json!({})),
        "source_event_id": row.get::<_, String>(5)?,
        "visibility": row.get::<_, String>(6)?,
    }))
}

fn skill_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let permissions: String = row.get(4)?;
    Ok(json!({
        "name": row.get::<_, String>(0)?,
        "version": row.get::<_, String>(1)?,
        "manifest_path": row.get::<_, String>(2)?,
        "entry": row.get::<_, String>(3)?,
        "permissions": serde_json::from_str::<Value>(&permissions).unwrap_or_else(|_| json!([])),
        "description": row.get::<_, Option<String>>(5)?,
        "source_event_id": row.get::<_, String>(6)?,
        "visibility": row.get::<_, String>(7)?,
    }))
}

fn context_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let attrs_json: Option<String> = row.get(10)?;
    Ok(json!({
        "id": row.get::<_, String>(0)?,
        "kind": row.get::<_, String>(1)?,
        "namespace": row.get::<_, Option<String>>(2)?,
        "title": row.get::<_, Option<String>>(3)?,
        "summary": row.get::<_, String>(4)?,
        "visibility": row.get::<_, String>(5)?,
        "source_event_id": row.get::<_, String>(6)?,
        "created_at": row.get::<_, String>(7)?,
        "updated_at": row.get::<_, String>(8)?,
        "schema_version": row.get::<_, i64>(9)?,
        "attrs": attrs_json
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .unwrap_or_else(|| json!({})),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn mcp_request(meshlet: &Meshlet, method: &str, params: Value) -> Value {
        let request = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        handle_mcp_request(meshlet, &request, json!(1))
    }

    fn mcp_content_text(response: &Value) -> &str {
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("MCP content text")
    }

    fn mcp_resource_text(response: &Value) -> &str {
        response["result"]["contents"][0]["text"]
            .as_str()
            .expect("MCP resource text")
    }

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
    fn init_creates_repo_event() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        assert_eq!(meshlet.event_count()?, 1);
        assert!(dir.path().join(DB_DIR).join(DB_FILE).exists());
        Ok(())
    }

    #[test]
    fn append_event_chains_hashes() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let first = meshlet.append_event("context.added", "agent:test", json!({"label": "a"}))?;
        let second = meshlet.append_event("context.added", "agent:test", json!({"label": "b"}))?;
        assert_eq!(second.prev_hash.as_deref(), Some(first.hash.as_str()));
        Ok(())
    }

    #[test]
    fn append_event_rejects_secret_key_names() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;

        let direct = meshlet.append_event(
            "context.added",
            "agent:test",
            json!({"label": "bad", "api_key": "value"}),
        );
        let nested = meshlet.append_event(
            "context.added",
            "agent:test",
            json!({"label": "bad", "nested": {"access-token": "value"}}),
        );

        assert!(direct.is_err());
        assert!(nested.is_err());
        assert_eq!(meshlet.event_count()?, 1);
        Ok(())
    }

    #[test]
    fn append_event_accepts_safe_payload_keys() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;

        meshlet.append_event(
            "context.added",
            "agent:test",
            json!({"label": "safe", "note": "public context"}),
        )?;

        assert_eq!(meshlet.event_count()?, 2);
        Ok(())
    }

    #[test]
    fn public_safe_append_rejects_secret_looking_values() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;

        let result = meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"label": "bad", "note": "Bearer abc123"}),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        );

        assert!(result.is_err());
        assert_eq!(meshlet.event_count()?, 1);
        Ok(())
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
    fn skills_visibility_column_materializes_from_event_visibility() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "skill.added",
            "agent:test",
            skill_payload("public-skill"),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let skill = meshlet.show_skill("public-skill")?;

        assert_eq!(skill["visibility"], "public");
        assert_eq!(skill["source_event_id"].as_str().expect("source").len(), 36);
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

    #[test]
    fn public_doctor_reports_stored_secret_like_values() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"label": "local", "note": "Bearer abc123"}),
            EventVisibility::Local,
            SafetyProfile::LocalTrusted,
        )?;

        let report = meshlet.public_doctor()?;

        assert_eq!(report["ok"], false);
        assert!(
            !report["redaction_report"]["suspicious_values"]
                .as_array()
                .expect("suspicious")
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn public_export_contains_only_public_compact_state() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"label": "private note"}),
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"label": "public note"}),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let export = meshlet.public_export(20)?;
        let events = export["events"].as_array().expect("events");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["visibility"], "public");
        assert!(events[0].get("payload").is_none());
        Ok(())
    }

    #[test]
    fn verify_event_chain_accepts_clean_events() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event("context.added", "agent:test", json!({"label": "clean"}))?;

        let report = meshlet.verify_event_chain()?;

        assert!(report.ok);
        assert_eq!(report.events, 2);
        assert_eq!(report.first_invalid_seq, None);
        assert_eq!(report.reason, None);
        Ok(())
    }

    #[test]
    fn verify_event_chain_detects_tampered_payload() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let event =
            meshlet.append_event("context.added", "agent:test", json!({"label": "safe"}))?;
        meshlet.conn.execute(
            "UPDATE events SET payload_json = ?1 WHERE id = ?2",
            params![r#"{"label":"tampered"}"#, event.id],
        )?;

        let report = meshlet.verify_event_chain()?;

        assert!(!report.ok);
        assert_eq!(report.first_invalid_seq, Some(2));
        assert_eq!(report.reason.as_deref(), Some("hash_mismatch"));
        Ok(())
    }

    #[test]
    fn verify_event_chain_detects_tampered_prev_hash() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let event =
            meshlet.append_event("context.added", "agent:test", json!({"label": "safe"}))?;
        meshlet.conn.execute(
            "UPDATE events SET prev_hash = ?1 WHERE id = ?2",
            params!["wrong", event.id],
        )?;

        let report = meshlet.verify_event_chain()?;

        assert!(!report.ok);
        assert_eq!(report.first_invalid_seq, Some(2));
        assert_eq!(report.reason.as_deref(), Some("prev_hash_mismatch"));
        Ok(())
    }

    #[test]
    fn skill_manifest_roundtrip_materializes_skill_and_graph() -> Result<()> {
        let dir = tempdir()?;
        let manifest_path = dir.path().join("skill.toml");
        fs::write(
            &manifest_path,
            r#"
name = "rust-review"
version = "0.1.0"
kind = "skill"
entry = "./SKILL.md"
permissions = ["read_repo", "run_check"]
description = "Review Rust code."
"#,
        )?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.add_skill_manifest(&manifest_path)?;
        let skill = meshlet.show_skill("rust-review")?;
        assert_eq!(skill["name"], "rust-review");
        let skill_nodes = meshlet.graph_nodes(Some("skill"))?;
        assert_eq!(skill_nodes.len(), 1);
        Ok(())
    }

    #[test]
    fn skill_manifest_rejects_unknown_permission_and_unsafe_entry() -> Result<()> {
        let dir = tempdir()?;
        let bad_permission = dir.path().join("bad-permission.toml");
        fs::write(
            &bad_permission,
            r#"
name = "bad-permission"
version = "0.1.0"
kind = "skill"
entry = "./SKILL.md"
permissions = ["network"]
"#,
        )?;
        let absolute_entry = dir.path().join("absolute-entry.toml");
        fs::write(
            &absolute_entry,
            r#"
name = "absolute-entry"
version = "0.1.0"
kind = "skill"
entry = "/tmp/SKILL.md"
permissions = ["read_repo"]
"#,
        )?;
        let parent_entry = dir.path().join("parent-entry.toml");
        fs::write(
            &parent_entry,
            r#"
name = "parent-entry"
version = "0.1.0"
kind = "skill"
entry = "../SKILL.md"
permissions = ["read_repo"]
"#,
        )?;
        let meshlet = Meshlet::init(dir.path())?;

        assert!(meshlet.add_skill_manifest(&bad_permission).is_err());
        assert!(meshlet.add_skill_manifest(&absolute_entry).is_err());
        assert!(meshlet.add_skill_manifest(&parent_entry).is_err());
        assert_eq!(meshlet.event_count()?, 1);
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
    fn task_and_evidence_views_track_latest_state_and_graph_edges() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event(
            "task.created",
            "agent:test",
            json!({"task_id": "task-1", "title": "Ship journal", "status": "open"}),
        )?;
        meshlet.append_event(
            "task.updated",
            "agent:test",
            json!({"task_id": "task-1", "status": "done", "note": "verified"}),
        )?;
        let evidence = meshlet.append_event(
            "evidence.attached",
            "agent:test",
            json!({"path": "src/lib.rs", "task_id": "task-1", "note": "impl"}),
        )?;

        let task = meshlet.show_task("task-1")?;
        assert_eq!(task["status"], "done");
        assert_eq!(task["note"], "verified");
        assert_eq!(meshlet.list_tasks(20)?.len(), 1);
        assert_eq!(meshlet.list_evidence(20)?.len(), 1);
        assert_eq!(
            meshlet.show_evidence(&format!("evidence:{}", evidence.id))?["kind"],
            "evidence"
        );
        assert!(
            meshlet
                .graph_edges_limited(Some(&format!("evidence:{}", evidence.id)), 20)?
                .iter()
                .any(|edge| edge["kind"] == "supports" && edge["to_id"] == "task:task-1")
        );
        Ok(())
    }

    #[test]
    fn task_and_evidence_payloads_require_core_fields() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;

        assert!(
            meshlet
                .append_event("task.created", "agent:test", json!({"status": "open"}))
                .is_err()
        );
        assert!(
            meshlet
                .append_event("task.updated", "agent:test", json!({"status": "done"}))
                .is_err()
        );
        assert!(
            meshlet
                .append_event(
                    "evidence.attached",
                    "agent:test",
                    json!({"note": "missing"})
                )
                .is_err()
        );
        assert_eq!(meshlet.event_count()?, 1);
        Ok(())
    }

    #[test]
    fn evidence_attach_auto_sha256_and_verify_passes() -> Result<()> {
        let dir = tempdir()?;
        let file_path = dir.path().join("evidence.txt");
        fs::write(&file_path, "stable evidence")?;
        let meshlet = Meshlet::init(dir.path())?;

        let attached = meshlet.attach_evidence_file(&file_path, Some("task-1"), Some("auto"))?;
        let verified = meshlet.verify_evidence(attached["evidence_id"].as_str().expect("id"))?;

        assert_eq!(attached["sha256"].as_str().expect("digest").len(), 64);
        assert_eq!(verified["ok"], true);
        assert_eq!(verified["expected_sha256"], verified["actual_sha256"]);
        Ok(())
    }

    #[test]
    fn evidence_verify_fails_for_changed_or_missing_file() -> Result<()> {
        let dir = tempdir()?;
        let file_path = dir.path().join("evidence.txt");
        fs::write(&file_path, "before")?;
        let meshlet = Meshlet::init(dir.path())?;
        let attached = meshlet.attach_evidence_file(&file_path, None, Some("auto"))?;
        let evidence_id = attached["evidence_id"].as_str().expect("id");

        fs::write(&file_path, "after")?;
        let changed = meshlet.verify_evidence(evidence_id)?;
        assert_eq!(changed["ok"], false);

        fs::remove_file(&file_path)?;
        assert!(meshlet.verify_evidence(evidence_id).is_err());
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
        assert_eq!(report["nodes"], 2);
        assert_eq!(report["edges"], 1);
        assert!(report["source_sha256"].as_str().expect("digest").len() == 64);
        assert_eq!(meshlet.event_count()?, 2);
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

    #[test]
    fn mcp_initialize_advertises_server_capabilities() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let response = mcp_request(&meshlet, "initialize", json!({}));

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        assert!(response["result"]["capabilities"]["tools"].is_object());
        assert!(response["result"]["capabilities"]["resources"].is_object());
        assert_eq!(response["result"]["serverInfo"]["name"], "meshlet");
        Ok(())
    }

    #[test]
    fn mcp_tools_list_advertises_core_tools() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let response = mcp_request(&meshlet, "tools/list", json!({}));

        let tools = response["result"]["tools"].as_array().expect("tools array");
        let names = tools
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"meshlet_publish_event"));
        assert!(names.contains(&"meshlet_list_skills"));
        assert!(names.contains(&"meshlet_list_tasks"));
        assert!(names.contains(&"meshlet_get_task"));
        assert!(names.contains(&"meshlet_query"));
        assert!(names.contains(&"meshlet_get_context"));
        Ok(())
    }

    #[test]
    fn mcp_list_skills_returns_registered_skill() -> Result<()> {
        let dir = tempdir()?;
        let manifest_path = dir.path().join("skill.toml");
        fs::write(
            &manifest_path,
            r#"
name = "mcp-rust-review"
version = "0.1.0"
kind = "skill"
entry = "./SKILL.md"
permissions = ["read_repo"]
description = "Review Rust code through MCP."
"#,
        )?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.add_skill_manifest(&manifest_path)?;

        let response = mcp_request(
            &meshlet,
            "tools/call",
            json!({ "name": "meshlet_list_skills", "arguments": {} }),
        );

        assert!(mcp_content_text(&response).contains("mcp-rust-review"));
        Ok(())
    }

    #[test]
    fn mcp_publish_event_appends_event_and_returns_event_details() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let before = meshlet.event_count()?;

        let response = mcp_request(
            &meshlet,
            "tools/call",
            json!({
                "name": "meshlet_publish_event",
                "arguments": {
                    "type": "context.added",
                    "actor": "mcp:test",
                    "payload": { "label": "MCP context" }
                }
            }),
        );

        assert_eq!(meshlet.event_count()?, before + 1);
        let text = mcp_content_text(&response);
        assert!(text.contains("context.added"));
        assert!(text.contains("mcp:test"));
        Ok(())
    }

    #[test]
    fn mcp_query_returns_search_results() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event(
            "context.added",
            "agent:test",
            json!({"label": "needle context"}),
        )?;

        let response = mcp_request(
            &meshlet,
            "tools/call",
            json!({
                "name": "meshlet_query",
                "arguments": { "q": "needle", "kind": "events", "limit": 5 }
            }),
        );
        let value: Value = serde_json::from_str(mcp_content_text(&response))?;

        assert_eq!(value["kind"], "events");
        assert_eq!(value["limit"], 5);
        assert_eq!(value["events"]["items"][0]["type"], "context.added");
        assert!(value["nodes"].is_null());
        Ok(())
    }

    #[test]
    fn mcp_get_context_respects_limit() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        for index in 0..25 {
            meshlet.append_event("context.added", "agent:test", json!({"label": index}))?;
        }

        let response = mcp_request(
            &meshlet,
            "tools/call",
            json!({ "name": "meshlet_get_context", "arguments": { "limit": 3 } }),
        );
        let value: Value = serde_json::from_str(mcp_content_text(&response))?;

        assert_eq!(value["limit"], 3);
        assert_eq!(
            value["events_recent"]["items"]
                .as_array()
                .expect("event items")
                .len(),
            3
        );
        assert_eq!(value["events_recent"]["truncated"], true);
        Ok(())
    }

    #[test]
    fn mcp_resources_list_advertises_core_resources() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let response = mcp_request(&meshlet, "resources/list", json!({}));

        let resources = response["result"]["resources"]
            .as_array()
            .expect("resources array");
        let uris = resources
            .iter()
            .filter_map(|resource| resource["uri"].as_str())
            .collect::<Vec<_>>();
        assert!(uris.contains(&"meshlet://skills"));
        assert!(uris.contains(&"meshlet://events/recent"));
        assert!(uris.contains(&"meshlet://tasks"));
        assert!(uris.contains(&"meshlet://evidence/recent"));
        assert!(uris.contains(&"meshlet://graph/namespaces"));
        assert!(uris.contains(&"meshlet://graph"));
        Ok(())
    }

    #[test]
    fn mcp_read_skills_resource_returns_json_skill_list() -> Result<()> {
        let dir = tempdir()?;
        let manifest_path = dir.path().join("skill.toml");
        fs::write(
            &manifest_path,
            r#"
name = "resource-rust-review"
version = "0.1.0"
kind = "skill"
entry = "./SKILL.md"
permissions = ["read_repo"]
description = "Review Rust code through resources."
"#,
        )?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.add_skill_manifest(&manifest_path)?;

        let response = mcp_request(
            &meshlet,
            "resources/read",
            json!({ "uri": "meshlet://skills" }),
        );

        assert_eq!(
            response["result"]["contents"][0]["mimeType"],
            "application/json"
        );
        let value: Value = serde_json::from_str(mcp_resource_text(&response))?;
        assert_eq!(value["skills"][0]["name"], "resource-rust-review");
        Ok(())
    }

    #[test]
    fn mcp_task_tools_and_resources_return_task_views() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event(
            "task.created",
            "agent:test",
            json!({"task_id": "mcp-task", "title": "Expose tasks", "status": "open"}),
        )?;

        let list = mcp_request(
            &meshlet,
            "tools/call",
            json!({ "name": "meshlet_list_tasks", "arguments": { "limit": 5 } }),
        );
        let listed: Value = serde_json::from_str(mcp_content_text(&list))?;
        assert_eq!(listed["tasks"][0]["id"], "mcp-task");

        let show = mcp_request(
            &meshlet,
            "tools/call",
            json!({ "name": "meshlet_get_task", "arguments": { "id": "mcp-task" } }),
        );
        let shown: Value = serde_json::from_str(mcp_content_text(&show))?;
        assert_eq!(shown["title"], "Expose tasks");

        let resource = mcp_request(
            &meshlet,
            "resources/read",
            json!({ "uri": "meshlet://tasks" }),
        );
        let value: Value = serde_json::from_str(mcp_resource_text(&resource))?;
        assert_eq!(value["tasks"][0]["id"], "mcp-task");
        Ok(())
    }

    #[test]
    fn mcp_graph_namespaces_resource_returns_namespaces() -> Result<()> {
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

        let response = mcp_request(
            &meshlet,
            "resources/read",
            json!({ "uri": "meshlet://graph/namespaces" }),
        );
        let value: Value = serde_json::from_str(mcp_resource_text(&response))?;

        assert_eq!(value["namespaces"][0], "graphify:repo");
        Ok(())
    }

    #[test]
    fn mcp_query_accepts_namespace_and_rejects_invalid_namespace() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "graphify:repo",
                "nodes": [{"id": "a", "label": "Needle"}],
                "links": []
            }),
        )?;

        let scoped = mcp_request(
            &meshlet,
            "tools/call",
            json!({
                "name": "meshlet_query",
                "arguments": { "q": "Needle", "kind": "nodes", "namespace": "graphify:repo" }
            }),
        );
        let value: Value = serde_json::from_str(mcp_content_text(&scoped))?;
        assert_eq!(value["namespace"], "graphify:repo");
        assert_eq!(value["nodes"]["items"].as_array().expect("nodes").len(), 1);

        let invalid = mcp_request(
            &meshlet,
            "tools/call",
            json!({
                "name": "meshlet_query",
                "arguments": { "q": "Needle", "namespace": 1 }
            }),
        );
        assert_eq!(invalid["error"]["code"], -32602);
        Ok(())
    }

    #[test]
    fn mcp_public_safe_rejects_mutation_and_full_output() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let publish = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "meshlet_publish_event",
                "arguments": {
                    "type": "context.added",
                    "visibility": "public",
                    "payload": { "label": "public" }
                }
            }
        });
        let full_query = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "meshlet_query",
                "arguments": { "q": "repo", "mode": "full" }
            }
        });

        let publish_response = handle_mcp_request_with_profile(
            &meshlet,
            &publish,
            json!(1),
            SafetyProfile::PublicSafe,
        );
        let query_response = handle_mcp_request_with_profile(
            &meshlet,
            &full_query,
            json!(2),
            SafetyProfile::PublicSafe,
        );

        assert_eq!(publish_response["error"]["code"], -32602);
        assert_eq!(query_response["error"]["code"], -32602);
        Ok(())
    }

    #[test]
    fn mcp_get_task_returns_json_rpc_error_for_missing_task() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;

        let response = mcp_request(
            &meshlet,
            "tools/call",
            json!({ "name": "meshlet_get_task", "arguments": { "id": "missing" } }),
        );

        assert_eq!(response["error"]["code"], -32602);
        Ok(())
    }

    #[test]
    fn mcp_unknown_tool_returns_json_rpc_error() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;

        let response = mcp_request(
            &meshlet,
            "tools/call",
            json!({ "name": "meshlet_missing_tool", "arguments": {} }),
        );

        assert_eq!(response["error"]["code"], -32602);
        assert!(
            response["error"]["message"]
                .as_str()
                .expect("error message")
                .contains("unknown tool")
        );
        Ok(())
    }

    #[test]
    fn mcp_publish_event_rejects_missing_required_params() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;

        let missing_type = mcp_request(
            &meshlet,
            "tools/call",
            json!({
                "name": "meshlet_publish_event",
                "arguments": { "payload": { "label": "missing type" } }
            }),
        );
        let missing_payload = mcp_request(
            &meshlet,
            "tools/call",
            json!({
                "name": "meshlet_publish_event",
                "arguments": { "type": "context.added" }
            }),
        );

        assert_eq!(missing_type["error"]["code"], -32602);
        assert_eq!(missing_payload["error"]["code"], -32602);
        Ok(())
    }
}
