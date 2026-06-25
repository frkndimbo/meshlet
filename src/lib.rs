use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const DB_DIR: &str = ".meshlet";
pub const DB_FILE: &str = "meshlet.db";

const EVENT_TYPES: &[&str] = &[
    "repo.initialized",
    "skill.added",
    "context.added",
    "agent.message",
    "evidence.attached",
    "task.created",
    "task.updated",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Event {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub created_at: String,
    pub actor: String,
    pub payload: Value,
    pub hash: String,
    pub prev_hash: Option<String>,
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
                hash TEXT NOT NULL UNIQUE,
                prev_hash TEXT
            );
            CREATE TABLE IF NOT EXISTS graph_nodes (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                label TEXT,
                attrs_json TEXT NOT NULL,
                source_event_id TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS graph_edges (
                id TEXT PRIMARY KEY,
                from_id TEXT NOT NULL,
                to_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                attrs_json TEXT NOT NULL,
                source_event_id TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS skills (
                name TEXT PRIMARY KEY,
                version TEXT NOT NULL,
                manifest_path TEXT NOT NULL,
                entry TEXT NOT NULL,
                permissions_json TEXT NOT NULL,
                description TEXT,
                source_event_id TEXT NOT NULL
            );
            INSERT OR REPLACE INTO meta(key, value) VALUES('schema_version', '1');
            "#,
        )?;
        Ok(())
    }

    pub fn event_count(&self) -> Result<u64> {
        let count: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
        Ok(count)
    }

    pub fn append_event(&self, event_type: &str, actor: &str, payload: Value) -> Result<Event> {
        validate_event_type(event_type)?;
        if actor.trim().is_empty() {
            bail!("actor must not be empty");
        }
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
            "INSERT INTO events(id, seq, type, created_at, actor, payload_json, hash, prev_hash)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                next_seq,
                event_type,
                created_at,
                actor,
                canonical_json(&payload)?,
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
        let mut stmt = self.conn.prepare(
            "SELECT id, type, created_at, actor, payload_json, hash, prev_hash
             FROM events ORDER BY seq DESC LIMIT ?1",
        )?;
        let events = stmt
            .query_map([limit], event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(events)
    }

    pub fn show_event(&self, id: &str) -> Result<Event> {
        self.conn
            .query_row(
                "SELECT id, type, created_at, actor, payload_json, hash, prev_hash
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
        let mut stmt = self.conn.prepare(
            "SELECT name, version, manifest_path, entry, permissions_json, description, source_event_id
             FROM skills ORDER BY name",
        )?;
        let skills = stmt
            .query_map([], |row| {
                let permissions: String = row.get(4)?;
                Ok(json!({
                    "name": row.get::<_, String>(0)?,
                    "version": row.get::<_, String>(1)?,
                    "manifest_path": row.get::<_, String>(2)?,
                    "entry": row.get::<_, String>(3)?,
                    "permissions": serde_json::from_str::<Value>(&permissions).unwrap_or_else(|_| json!([])),
                    "description": row.get::<_, Option<String>>(5)?,
                    "source_event_id": row.get::<_, String>(6)?,
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(skills)
    }

    pub fn show_skill(&self, name: &str) -> Result<Value> {
        self.conn
            .query_row(
                "SELECT name, version, manifest_path, entry, permissions_json, description, source_event_id
                 FROM skills WHERE name = ?1",
                [name],
                |row| {
                    let permissions: String = row.get(4)?;
                    Ok(json!({
                        "name": row.get::<_, String>(0)?,
                        "version": row.get::<_, String>(1)?,
                        "manifest_path": row.get::<_, String>(2)?,
                        "entry": row.get::<_, String>(3)?,
                        "permissions": serde_json::from_str::<Value>(&permissions).unwrap_or_else(|_| json!([])),
                        "description": row.get::<_, Option<String>>(5)?,
                        "source_event_id": row.get::<_, String>(6)?,
                    }))
                },
            )
            .optional()?
            .ok_or_else(|| anyhow!("skill not found: {name}"))
    }

    pub fn rebuild_graph(&self) -> Result<()> {
        self.conn.execute("DELETE FROM graph_edges", [])?;
        self.conn.execute("DELETE FROM graph_nodes", [])?;
        self.conn.execute("DELETE FROM skills", [])?;
        let events = self.events_ascending()?;
        for event in events {
            self.apply_event(&event)?;
        }
        Ok(())
    }

    pub fn graph_nodes(&self, kind: Option<&str>) -> Result<Vec<Value>> {
        let (sql, params_value): (&str, Vec<String>) = match kind {
            Some(kind) => (
                "SELECT id, kind, label, attrs_json, source_event_id FROM graph_nodes WHERE kind = ?1 ORDER BY id",
                vec![kind.to_string()],
            ),
            None => (
                "SELECT id, kind, label, attrs_json, source_event_id FROM graph_nodes ORDER BY id",
                vec![],
            ),
        };
        let mut stmt = self.conn.prepare(sql)?;
        let nodes = stmt
            .query_map(rusqlite::params_from_iter(params_value), node_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(nodes)
    }

    pub fn graph_edges(&self, from_id: Option<&str>) -> Result<Vec<Value>> {
        let (sql, params_value): (&str, Vec<String>) = match from_id {
            Some(from_id) => (
                "SELECT id, from_id, to_id, kind, attrs_json, source_event_id FROM graph_edges WHERE from_id = ?1 ORDER BY id",
                vec![from_id.to_string()],
            ),
            None => (
                "SELECT id, from_id, to_id, kind, attrs_json, source_event_id FROM graph_edges ORDER BY id",
                vec![],
            ),
        };
        let mut stmt = self.conn.prepare(sql)?;
        let edges = stmt
            .query_map(rusqlite::params_from_iter(params_value), edge_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(edges)
    }

    pub fn context_snapshot(&self) -> Result<Value> {
        Ok(json!({
            "root": self.root.display().to_string(),
            "events_recent": self.list_events(20)?,
            "skills": self.list_skills()?,
            "graph": {
                "nodes": self.graph_nodes(None)?,
                "edges": self.graph_edges(None)?,
            }
        }))
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
                "SELECT id, type, created_at, actor, payload_json, hash, prev_hash
                 FROM events WHERE seq = ?1",
                [seq],
                event_from_row,
            )
            .optional()?;
        Ok(event)
    }

    fn events_ascending(&self) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, type, created_at, actor, payload_json, hash, prev_hash
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
                    json!({ "root": root }),
                    &event.id,
                )?;
            }
            "skill.added" => self.apply_skill_added(event)?,
            "context.added" => {
                self.upsert_node(
                    &format!("context:{}", event.id),
                    "context",
                    event.payload.get("label").and_then(Value::as_str),
                    event.payload.clone(),
                    &event.id,
                )?;
            }
            "agent.message" => self.apply_agent_message(event)?,
            "evidence.attached" => self.apply_evidence(event)?,
            "task.created" | "task.updated" => self.apply_task(event)?,
            _ => {}
        }
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
            "INSERT OR REPLACE INTO skills(name, version, manifest_path, entry, permissions_json, description, source_event_id)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                name,
                version,
                manifest_path,
                entry,
                canonical_json(&permissions)?,
                description,
                event.id
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
        )?;
        let file_id = format!("file:{entry}");
        self.upsert_node(
            &file_id,
            "file",
            Some(entry),
            json!({ "path": entry }),
            &event.id,
        )?;
        self.upsert_edge(
            &format!("edge:{}:skill-entry", event.id),
            &skill_id,
            &file_id,
            "references",
            json!({ "field": "entry" }),
            &event.id,
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
        )?;
        self.upsert_node(
            &message_id,
            "message",
            event.payload.get("summary").and_then(Value::as_str),
            event.payload.clone(),
            &event.id,
        )?;
        self.upsert_edge(
            &format!("edge:{}:agent-message", event.id),
            &agent_id,
            &message_id,
            "produced",
            json!({}),
            &event.id,
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
            event.payload.clone(),
            &event.id,
        )?;
        self.upsert_node(
            &file_id,
            "file",
            Some(path),
            json!({ "path": path }),
            &event.id,
        )?;
        self.upsert_edge(
            &format!("edge:{}:evidence-file", event.id),
            &evidence_id,
            &file_id,
            "references",
            json!({}),
            &event.id,
        )?;
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
            event.payload.clone(),
            &event.id,
        )?;
        Ok(())
    }

    fn upsert_node(
        &self,
        id: &str,
        kind: &str,
        label: Option<&str>,
        attrs: Value,
        source_event_id: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO graph_nodes(id, kind, label, attrs_json, source_event_id)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![id, kind, label, canonical_json(&attrs)?, source_event_id],
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
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO graph_edges(id, from_id, to_id, kind, attrs_json, source_event_id)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                from_id,
                to_id,
                kind,
                canonical_json(&attrs)?,
                source_event_id
            ],
        )?;
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

pub fn run_mcp_stdio(meshlet: Meshlet) -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                write_json(
                    &mut stdout,
                    &json_rpc_error(Value::Null, -32700, &format!("parse error: {error}")),
                )?;
                continue;
            }
        };
        if request.get("id").is_none() {
            continue;
        }
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let response = handle_mcp_request(&meshlet, &request, id);
        write_json(&mut stdout, &response)?;
    }
    Ok(())
}

fn handle_mcp_request(meshlet: &Meshlet, request: &Value, id: Value) -> Value {
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    match method {
        "initialize" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2025-06-18",
                "capabilities": {
                    "tools": { "listChanged": false },
                    "resources": { "subscribe": false, "listChanged": false }
                },
                "serverInfo": { "name": "meshlet", "version": env!("CARGO_PKG_VERSION") }
            }
        }),
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": mcp_tools() }
        }),
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
            match mcp_tool_call(meshlet, &params) {
                Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Err(error) => json_rpc_error(id, -32602, &error.to_string()),
            }
        }
        "resources/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "resources": mcp_resources() }
        }),
        "resources/read" => {
            let uri = request
                .get("params")
                .and_then(|params| params.get("uri"))
                .and_then(Value::as_str)
                .unwrap_or("");
            match mcp_read_resource(meshlet, uri) {
                Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Err(error) => json_rpc_error(id, -32602, &error.to_string()),
            }
        }
        _ => json_rpc_error(id, -32601, "method not found"),
    }
}

fn mcp_tool_call(meshlet: &Meshlet, params: &Value) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("tool name is required"))?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let payload = match name {
        "meshlet_publish_event" => {
            let event_type = str_field(&args, "type")?;
            let actor = args.get("actor").and_then(Value::as_str).unwrap_or("agent");
            let payload = args
                .get("payload")
                .filter(|payload| payload.is_object())
                .cloned()
                .ok_or_else(|| anyhow!("payload object is required"))?;
            serde_json::to_value(meshlet.append_event(event_type, actor, payload)?)?
        }
        "meshlet_list_skills" => json!({ "skills": meshlet.list_skills()? }),
        "meshlet_get_context" => meshlet.context_snapshot()?,
        other => bail!("unknown tool: {other}"),
    };
    Ok(json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&payload)? }]
    }))
}

fn mcp_read_resource(meshlet: &Meshlet, uri: &str) -> Result<Value> {
    let value = match uri {
        "meshlet://skills" => json!({ "skills": meshlet.list_skills()? }),
        "meshlet://events/recent" => json!({ "events": meshlet.list_events(20)? }),
        "meshlet://graph" => {
            json!({ "nodes": meshlet.graph_nodes(None)?, "edges": meshlet.graph_edges(None)? })
        }
        other => bail!("unknown resource: {other}"),
    };
    Ok(json!({
        "contents": [{
            "uri": uri,
            "mimeType": "application/json",
            "text": serde_json::to_string_pretty(&value)?
        }]
    }))
}

fn mcp_tools() -> Value {
    json!([
        {
            "name": "meshlet_publish_event",
            "description": "Append an event to the local Meshlet event log.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": { "type": "string" },
                    "actor": { "type": "string" },
                    "payload": { "type": "object" }
                },
                "required": ["type", "payload"]
            }
        },
        {
            "name": "meshlet_list_skills",
            "description": "List local Meshlet skill manifests.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "meshlet_get_context",
            "description": "Read recent events, skills, and graph materialization.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

fn mcp_resources() -> Value {
    json!([
        {
            "uri": "meshlet://skills",
            "name": "Meshlet Skills",
            "description": "Registered local skills.",
            "mimeType": "application/json"
        },
        {
            "uri": "meshlet://events/recent",
            "name": "Recent Meshlet Events",
            "description": "Recent append-only events.",
            "mimeType": "application/json"
        },
        {
            "uri": "meshlet://graph",
            "name": "Meshlet Graph",
            "description": "Materialized context graph.",
            "mimeType": "application/json"
        }
    ])
}

fn write_json(stdout: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *stdout, value)?;
    writeln!(stdout)?;
    stdout.flush()?;
    Ok(())
}

fn json_rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn validate_event_type(event_type: &str) -> Result<()> {
    if EVENT_TYPES.contains(&event_type) {
        Ok(())
    } else {
        bail!("unsupported event type: {event_type}")
    }
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
    Ok(())
}

fn str_field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing string field: {key}"))
}

fn canonical_json(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?)
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
        hash: row.get(5)?,
        prev_hash: row.get(6)?,
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
