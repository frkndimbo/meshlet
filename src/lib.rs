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

mod compact;
pub mod config;
mod event_log;
mod evidence;
mod graph;
mod mailbox;
mod mcp;
mod okf;
mod public_export;
mod public_safe;
mod query;
mod refresh;
mod schema;
mod skills;
mod tasks;

pub(crate) use compact::*;
pub use mcp::{run_mcp_stdio, run_mcp_stdio_with_profile};
pub(crate) use okf::*;
pub(crate) use public_safe::*;
pub use refresh::RefreshOptions;

pub const DB_DIR: &str = ".meshlet";
pub const DB_FILE: &str = "meshlet.db";
pub const DEFAULT_LIMIT: u32 = 20;
pub const MAX_LIMIT: u32 = 100;
const SCHEMA_VERSION: &str = "6";
const CONTEXT_SCHEMA_VERSION: i64 = 1;
const TASK_SCHEMA_VERSION: i64 = 1;
const MESSAGE_SCHEMA_VERSION: i64 = 1;

const TASK_STATUSES: &[&str] = &["open", "in_progress", "blocked", "done", "canceled"];

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EventVisibility {
    #[default]
    Private,
    Local,
    Public,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    #[default]
    Compact,
    Full,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SafetyProfile {
    #[default]
    LocalTrusted,
    PublicSafe,
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
            if payload.get("task_id").is_some() {
                required_nonempty_string(payload, "task_id")?;
            }
            let status = nonempty_string(payload, "status").unwrap_or("open");
            if !valid_task_status(status) {
                bail!("task status must be open, in_progress, blocked, done, or canceled");
            }
            if payload.get("assignee").is_some() {
                required_nonempty_string(payload, "assignee")?;
            }
        }
        "task.updated" => {
            required_nonempty_string(payload, "task_id")?;
            if !["status", "assignee", "note"]
                .iter()
                .any(|key| payload.get(*key).is_some())
            {
                bail!("task.updated requires status, assignee, or note");
            }
            if let Some(status) = nonempty_string(payload, "status") {
                if !valid_task_status(status) {
                    bail!("task status must be open, in_progress, blocked, done, or canceled");
                }
            } else if payload.get("status").is_some() {
                required_nonempty_string(payload, "status")?;
            }
            if payload.get("assignee").is_some() {
                required_nonempty_string(payload, "assignee")?;
            }
        }
        "agent.message" => {
            required_nonempty_string(payload, "from")?;
            required_nonempty_string(payload, "to")?;
            required_nonempty_string(payload, "summary")?;
            if payload.get("task_id").is_some() {
                required_nonempty_string(payload, "task_id")?;
            }
            if payload.get("reply_to").is_some() {
                required_nonempty_string(payload, "reply_to")?;
            }
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

fn merge_task_payload(task: &mut serde_json::Map<String, Value>, event: &Event, created: bool) {
    if created && event.payload.get("status").is_none() {
        task.insert("status".to_string(), json!("open"));
    }
    for key in ["title", "status", "assignee", "note"] {
        if let Some(value) = event.payload.get(key) {
            task.insert(key.to_string(), value.clone());
        }
    }
    task.entry("title")
        .or_insert_with(|| json!(event_task_id(event).unwrap_or_else(|| event.id.clone())));
    task.entry("status").or_insert_with(|| json!("open"));
    task.insert("visibility".to_string(), json!(event.visibility.as_str()));
    task.insert("schema_version".to_string(), json!(TASK_SCHEMA_VERSION));
    task.insert("updated_event_id".to_string(), json!(event.id));
    task.insert("updated_at".to_string(), json!(event.created_at));
}

fn valid_task_status(status: &str) -> bool {
    TASK_STATUSES.contains(&status)
}

fn validate_task_transition(current: &str, next: &str) -> Result<()> {
    if current == next {
        return Ok(());
    }
    let allowed = match current {
        "open" => matches!(next, "in_progress" | "blocked" | "canceled"),
        "in_progress" => matches!(next, "blocked" | "done" | "canceled"),
        "blocked" => matches!(next, "in_progress" | "canceled"),
        "done" | "canceled" => false,
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        bail!("invalid task status transition: {current} -> {next}");
    }
}

fn event_task_id(event: &Event) -> Option<String> {
    match event.event_type.as_str() {
        "task.created" => event
            .payload
            .get("task_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| Some(event.id.clone())),
        "task.updated" | "agent.message" | "evidence.attached" => event
            .payload
            .get("task_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        "context.added" => event
            .payload
            .get("task_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        _ => None,
    }
}

fn fts_query(input: &str) -> Result<String> {
    let terms = input
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{term}\""))
        .collect::<Vec<_>>();
    if terms.is_empty() {
        bail!("query has no searchable terms");
    }
    Ok(terms.join(" "))
}

fn graph_namespace(attrs: &Value) -> Option<&str> {
    attrs.get("namespace").and_then(Value::as_str)
}

fn graph_edge_label(attrs: &Value) -> Option<&str> {
    ["label", "relation", "field", "title", "summary", "note"]
        .into_iter()
        .find_map(|key| attrs.get(key).and_then(Value::as_str))
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
    visibility: EventVisibility,
    prev_hash: Option<&str>,
) -> Result<String> {
    let body = json!({
        "id": id,
        "type": event_type,
        "created_at": created_at,
        "actor": actor,
        "visibility": visibility.as_str(),
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

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let attrs_json: Option<String> = row.get(11)?;
    Ok(json!({
        "id": row.get::<_, String>(0)?,
        "title": row.get::<_, String>(1)?,
        "status": row.get::<_, String>(2)?,
        "assignee": row.get::<_, Option<String>>(3)?,
        "note": row.get::<_, Option<String>>(4)?,
        "visibility": row.get::<_, String>(5)?,
        "created_event_id": row.get::<_, String>(6)?,
        "updated_event_id": row.get::<_, String>(7)?,
        "created_at": row.get::<_, String>(8)?,
        "updated_at": row.get::<_, String>(9)?,
        "schema_version": row.get::<_, i64>(10)?,
        "attrs": attrs_json
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .unwrap_or_else(|| json!({})),
    }))
}

fn message_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let attrs_json: Option<String> = row.get(11)?;
    Ok(json!({
        "id": row.get::<_, String>(0)?,
        "from": row.get::<_, String>(1)?,
        "to": row.get::<_, String>(2)?,
        "task_id": row.get::<_, Option<String>>(3)?,
        "summary": row.get::<_, String>(4)?,
        "body": row.get::<_, Option<String>>(5)?,
        "reply_to": row.get::<_, Option<String>>(6)?,
        "visibility": row.get::<_, String>(7)?,
        "source_event_id": row.get::<_, String>(8)?,
        "created_at": row.get::<_, String>(9)?,
        "schema_version": row.get::<_, i64>(10)?,
        "attrs": attrs_json
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .unwrap_or_else(|| json!({})),
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

