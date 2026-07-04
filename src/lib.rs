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

mod event_log;
mod evidence;
mod graph;
mod mailbox;
mod mcp;
mod public_export;
mod query;
mod schema;
mod skills;
mod tasks;

#[cfg(test)]
use mcp::{handle_mcp_request, handle_mcp_request_with_profile};
pub use mcp::{run_mcp_stdio, run_mcp_stdio_with_profile};

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

struct OkfDocument {
    id: String,
    item_type: String,
    title: String,
    description: String,
    resource: String,
    tags: Vec<String>,
    timestamp: String,
    source_event_id: String,
    visibility: String,
    relative_path: String,
    body: String,
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
        _ => None,
    }
}

fn compact_timeline_event(seq: i64, event: &Event) -> Value {
    let mut payload = serde_json::Map::new();
    for key in [
        "task_id", "title", "status", "assignee", "note", "from", "to", "summary", "reply_to",
        "path", "ref",
    ] {
        if let Some(value) = event.payload.get(key) {
            payload.insert(key.to_string(), value.clone());
        }
    }
    json!({
        "seq": seq,
        "id": event.id,
        "type": event.event_type,
        "created_at": event.created_at,
        "actor": event.actor,
        "visibility": event.visibility.as_str(),
        "payload": payload,
    })
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

fn compact_event_text(event: &Event) -> String {
    [
        "label", "title", "summary", "status", "name", "assignee", "from", "to",
    ]
    .into_iter()
    .filter_map(|key| nonempty_string(&event.payload, key))
    .collect::<Vec<_>>()
    .join(" ")
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

fn compact_message(message: Value) -> Value {
    json!({
        "id": message.get("id").cloned().unwrap_or(Value::Null),
        "from": message.get("from").cloned().unwrap_or(Value::Null),
        "to": message.get("to").cloned().unwrap_or(Value::Null),
        "task_id": message.get("task_id").cloned().unwrap_or(Value::Null),
        "summary": message.get("summary").cloned().unwrap_or(Value::Null),
        "reply_to": message.get("reply_to").cloned().unwrap_or(Value::Null),
        "visibility": message.get("visibility").cloned().unwrap_or(Value::Null),
        "source_event_id": message.get("source_event_id").cloned().unwrap_or(Value::Null),
        "created_at": message.get("created_at").cloned().unwrap_or(Value::Null),
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

fn prepare_okf_output_dir(path: &Path) -> Result<()> {
    if path.exists() {
        if !path.is_dir() {
            bail!("OKF output path must be a directory");
        }
        if fs::read_dir(path)?.next().transpose()?.is_some() {
            bail!("OKF output directory must be empty");
        }
    } else {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

fn okf_index(documents: &[OkfDocument]) -> String {
    let mut text = String::from(
        "---\nokf_version: \"0.1\"\ntype: \"Meshlet OKF Bundle\"\ntitle: \"Meshlet Public Export\"\n---\n# Meshlet Public Export\n\n",
    );
    for doc in documents {
        text.push_str(&format!(
            "- [{}]({}) - {}\n",
            doc.title, doc.relative_path, doc.item_type
        ));
    }
    text
}

fn okf_log(events: &[Event]) -> String {
    let mut text = String::from("# Meshlet Public Event Log\n\n");
    for event in events {
        text.push_str(&format!(
            "- {} `{}` by `{}` (`{}`)\n",
            event.created_at, event.event_type, event.actor, event.id
        ));
    }
    text
}

fn okf_document_text(doc: &OkfDocument, relations: &str) -> String {
    let mut text = format!(
        "---\ntype: {}\ntitle: {}\ndescription: {}\nresource: {}\ntags:\n{}\ntimestamp: {}\nmeshlet_id: {}\nmeshlet_event_id: {}\nsource_event_id: {}\nvisibility: {}\n---\n# {}\n\n{}",
        yaml_string(&doc.item_type),
        yaml_string(&doc.title),
        yaml_string(&doc.description),
        yaml_string(&doc.resource),
        doc.tags
            .iter()
            .map(|tag| format!("  - {}\n", yaml_string(tag)))
            .collect::<String>(),
        yaml_string(&doc.timestamp),
        yaml_string(&doc.id),
        yaml_string(&doc.source_event_id),
        yaml_string(&doc.source_event_id),
        yaml_string(&doc.visibility),
        doc.title,
        doc.body,
    );
    if !relations.is_empty() {
        text.push_str("\n## Relations\n");
        text.push_str(relations);
    }
    text
}

fn okf_relation_lines(
    id: &str,
    edges: &[Value],
    path_by_id: &BTreeMap<String, String>,
    relative_path: &str,
) -> String {
    let mut lines = Vec::new();
    for edge in edges {
        let kind = string_value(edge, "kind").unwrap_or("references");
        if string_value(edge, "from_id") == Some(id) {
            if let Some(to_id) = string_value(edge, "to_id")
                && let Some(path) = path_by_id.get(to_id)
            {
                lines.push(format!(
                    "- `{kind}` [{}]({})\n",
                    to_id,
                    relative_link(relative_path, path)
                ));
            }
        } else if string_value(edge, "to_id") == Some(id) {
            if let Some(from_id) = string_value(edge, "from_id")
                && let Some(path) = path_by_id.get(from_id)
            {
                lines.push(format!(
                    "- [{}]({}) `{kind}` this\n",
                    from_id,
                    relative_link(relative_path, path)
                ));
            }
        }
    }
    lines.concat()
}

fn relative_link(from_file: &str, to_file: &str) -> String {
    let from_depth = from_file.matches('/').count();
    let mut link = String::new();
    for _ in 0..from_depth {
        link.push_str("../");
    }
    link.push_str(to_file);
    link
}

fn metadata_line(label: &str, value: Option<&str>) -> String {
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("- {label}: `{value}`\n"))
        .unwrap_or_default()
}

fn string_value<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn okf_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "item".to_string()
    } else {
        slug.to_string()
    }
}

fn collect_markdown_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(path);
        }
    }
    Ok(())
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn okf_frontmatter_type(text: &str) -> Option<String> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    for line in rest[..end].lines() {
        let Some(value) = line.trim().strip_prefix("type:") else {
            continue;
        };
        return Some(
            value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string(),
        );
    }
    Some(String::new())
}

fn markdown_links(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find(')') else {
            break;
        };
        links.push(after_start[..end].trim().to_string());
        rest = &after_start[end + 1..];
    }
    links
}

fn link_is_external_or_anchor(link: &str) -> bool {
    link.starts_with('#') || link.contains("://") || link.starts_with("mailto:")
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
    fn public_doctor_scans_all_graph_attrs() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        for index in 0..MAX_LIMIT {
            meshlet.conn.execute(
                "INSERT INTO graph_nodes(id, kind, label, attrs_json, source_event_id, visibility)
                 VALUES(?1, 'context', 'safe', '{}', 'manual', 'private')",
                params![format!("aaa-safe-{index:03}")],
            )?;
        }
        meshlet.conn.execute(
            "INSERT INTO graph_nodes(id, kind, label, attrs_json, source_event_id, visibility)
             VALUES('zzz-secret', 'context', 'secret', ?1, 'manual', 'private')",
            params![r#"{"note":"Bearer leaked-token"}"#],
        )?;

        let report = meshlet.public_doctor()?;

        assert_eq!(report["ok"], false);
        assert!(
            report["redaction_report"]["suspicious_values"]
                .as_array()
                .expect("suspicious")
                .iter()
                .any(|path| path == "payload.note")
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
        meshlet.create_task(
            Some("public-task"),
            "Public Task",
            None,
            Some("agent:b"),
            None,
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        meshlet.send_agent_message(
            "agent:a",
            "agent:b",
            "Public handoff",
            Some("public-task"),
            Some("public body omitted"),
            None,
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        meshlet.send_agent_message(
            "agent:a",
            "agent:b",
            "Private handoff",
            Some("public-task"),
            Some("private body omitted"),
            None,
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;

        let export = meshlet.public_export(20)?;
        let events = export["events"].as_array().expect("events");
        let export_text = export.to_string();

        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|event| event["visibility"] == "public"));
        assert!(events.iter().all(|event| event.get("payload").is_none()));
        assert_eq!(export["tasks"][0]["id"], "public-task");
        assert_eq!(
            export["mailbox"]["messages"]["items"][0]["summary"],
            "Public handoff"
        );
        assert!(
            export["mailbox"]["messages"]["items"][0]
                .get("body")
                .is_none()
        );
        assert!(
            export["timelines"][0]["items"]
                .as_array()
                .expect("timeline")
                .iter()
                .all(|item| item["visibility"] == "public")
        );
        assert!(!export_text.contains("public body omitted"));
        assert!(!export_text.contains("Private handoff"));
        assert!(!export_text.contains("private body omitted"));
        Ok(())
    }

    #[test]
    fn public_export_filters_public_rows_before_limit() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "zzz-public",
                "nodes": [{"id": "public-node", "label": "Public node"}],
                "links": [{"source": "public-node", "target": "public-node", "relation": "references"}]
            }),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        for index in 0..3 {
            meshlet.append_event_with_options(
                "graph.imported",
                "agent:test",
                json!({
                    "source": "graphify",
                    "namespace": "aaa-private",
                    "nodes": [{"id": format!("hidden-node-{index}"), "label": "Hidden node"}],
                    "links": [{"source": format!("hidden-node-{index}"), "target": format!("hidden-node-{index}"), "relation": "references"}]
                }),
                EventVisibility::Private,
                SafetyProfile::LocalTrusted,
            )?;
        }

        let export = meshlet.public_export(1)?;
        let events = export["events"].as_array().expect("events");
        let nodes = export["graph"]["nodes"].as_array().expect("nodes");
        let edges = export["graph"]["edges"].as_array().expect("edges");

        assert_eq!(events.len(), 1);
        assert!(events.iter().all(|event| event["visibility"] == "public"));
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["id"], "zzz-public:public-node");
        assert_eq!(nodes[0]["visibility"], "public");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["visibility"], "public");
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
    fn public_okf_export_writes_public_markdown_bundle() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"title": "Private Context", "summary": "private note"}),
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"title": "Public Context", "summary": "public note"}),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        meshlet.append_event_with_options(
            "skill.added",
            "agent:test",
            skill_payload("public-skill"),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        meshlet.create_task(
            Some("item-1"),
            "Public Task",
            None,
            None,
            None,
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        meshlet.send_agent_message(
            "agent:a",
            "agent:b",
            "Public OKF handoff",
            Some("item-1"),
            Some("public OKF body omitted"),
            None,
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        meshlet.send_agent_message(
            "agent:a",
            "agent:b",
            "Private OKF handoff",
            Some("item-1"),
            Some("private OKF body omitted"),
            None,
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.append_event_with_options(
            "evidence.attached",
            "agent:test",
            json!({"path": "src/lib.rs", "sha256": "0".repeat(64), "task_id": "item-1"}),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        let out = dir.path().join("okf");

        let export = meshlet.public_export_okf(&out, 20)?;
        let doctor = Meshlet::okf_doctor(&out)?;
        let bundle = read_dir_text(&out)?;

        assert_eq!(export["format"], "meshlet-okf-public-export-v1");
        assert_eq!(doctor["ok"], true);
        assert!(out.join("index.md").exists());
        assert!(out.join("log.md").exists());
        assert!(bundle.contains(r#"type: "Meshlet Context""#));
        assert!(bundle.contains(r#"type: "Meshlet Task""#));
        assert!(bundle.contains(r#"type: "Meshlet Message""#));
        assert!(bundle.contains(r#"type: "Meshlet Skill""#));
        assert!(bundle.contains(r#"type: "Meshlet Evidence""#));
        assert!(bundle.contains("# Citations"));
        assert!(bundle.contains("## Timeline"));
        assert!(bundle.contains("Public OKF handoff"));
        assert!(bundle.contains("public note"));
        assert!(!bundle.contains("public OKF body omitted"));
        assert!(!bundle.contains("Private OKF handoff"));
        assert!(!bundle.contains("private OKF body omitted"));
        assert!(!bundle.contains("private note"));
        Ok(())
    }

    #[test]
    fn okf_doctor_reports_malformed_docs_and_broken_links() -> Result<()> {
        let dir = tempdir()?;
        let contexts = dir.path().join("contexts");
        fs::create_dir_all(&contexts)?;
        fs::write(contexts.join("bad.md"), "# Missing frontmatter\n")?;
        fs::write(
            contexts.join("link.md"),
            "---\ntype: Meshlet Context\n---\n# Link\n[missing](missing.md)\n",
        )?;

        let report = Meshlet::okf_doctor(dir.path())?;

        assert_eq!(report["ok"], false);
        assert!(
            report["errors"][0]
                .as_str()
                .expect("error")
                .contains("bad.md")
        );
        assert!(
            report["warnings"][0]
                .as_str()
                .expect("warning")
                .contains("missing.md")
        );
        Ok(())
    }

    fn read_dir_text(path: &Path) -> Result<String> {
        let mut text = String::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                text.push_str(&read_dir_text(&path)?);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
                text.push_str(&fs::read_to_string(path)?);
            }
        }
        Ok(text)
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
    fn verify_event_chain_detects_tampered_visibility() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let event = meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"label": "private"}),
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.conn.execute(
            "UPDATE events SET visibility = ?1 WHERE id = ?2",
            params!["public", event.id],
        )?;

        let report = meshlet.verify_event_chain()?;

        assert!(!report.ok);
        assert_eq!(report.first_invalid_seq, Some(2));
        assert_eq!(report.reason.as_deref(), Some("hash_mismatch"));
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
            json!({"task_id": "task-1", "status": "in_progress"}),
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
    fn public_safe_task_reads_replay_public_events_only() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "task.created",
            "agent:test",
            json!({"task_id": "public-task", "title": "Public task"}),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        meshlet.append_event_with_options(
            "task.updated",
            "agent:test",
            json!({"task_id": "public-task", "status": "blocked", "note": "hidden"}),
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.append_event_with_options(
            "task.created",
            "agent:test",
            json!({"task_id": "private-task", "title": "Private task"}),
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;

        let public_tasks = meshlet.list_tasks_scoped(20, SafetyProfile::PublicSafe)?;
        let public_task = meshlet.show_task_scoped("public-task", SafetyProfile::PublicSafe)?;

        assert_eq!(public_tasks.len(), 1);
        assert_eq!(public_task["status"], "open");
        assert!(public_task.get("note").is_none_or(Value::is_null));
        assert!(
            meshlet
                .show_task_scoped("private-task", SafetyProfile::PublicSafe)
                .is_err()
        );
        assert_eq!(meshlet.show_task("public-task")?["status"], "blocked");
        Ok(())
    }

    #[test]
    fn task_state_machine_rejects_invalid_transition() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.create_task(
            Some("state-task"),
            "State task",
            None,
            None,
            None,
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;

        assert!(
            meshlet
                .update_task(
                    "state-task",
                    Some("done"),
                    None,
                    None,
                    EventVisibility::Private,
                    SafetyProfile::LocalTrusted,
                )
                .is_err()
        );
        meshlet.update_task(
            "state-task",
            Some("in_progress"),
            None,
            None,
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.update_task(
            "state-task",
            Some("done"),
            None,
            None,
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        assert!(
            meshlet
                .update_task(
                    "state-task",
                    Some("in_progress"),
                    None,
                    None,
                    EventVisibility::Private,
                    SafetyProfile::LocalTrusted,
                )
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn mailbox_views_track_inbox_outbox_and_task_timeline() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.create_task(
            Some("mail-task"),
            "Mail task",
            None,
            Some("agent:b"),
            None,
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        meshlet.send_agent_message(
            "agent:a",
            "agent:b",
            "Please handle this",
            Some("mail-task"),
            Some("body"),
            None,
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let inbox = meshlet.list_mailbox("agent:b", "inbox", 20, SafetyProfile::PublicSafe)?;
        let outbox = meshlet.list_mailbox("agent:a", "outbox", 20, SafetyProfile::PublicSafe)?;
        let timeline = meshlet.task_timeline("mail-task", 20, SafetyProfile::PublicSafe)?;

        assert_eq!(inbox["messages"]["items"][0]["from"], "agent:a");
        assert_eq!(outbox["messages"]["items"][0]["to"], "agent:b");
        assert_eq!(timeline["items"].as_array().expect("timeline").len(), 2);
        Ok(())
    }

    #[test]
    fn public_safe_timeline_excludes_hidden_events() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.create_task(
            Some("timeline-task"),
            "Timeline task",
            None,
            None,
            None,
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        meshlet.send_agent_message(
            "agent:a",
            "agent:b",
            "Hidden note",
            Some("timeline-task"),
            None,
            None,
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.append_event_with_options(
            "evidence.attached",
            "agent:test",
            json!({"path": "src/lib.rs", "task_id": "timeline-task"}),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let timeline = meshlet.task_timeline("timeline-task", 20, SafetyProfile::PublicSafe)?;
        let items = timeline["items"].as_array().expect("timeline");

        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| item["visibility"] == "public"));
        assert!(items.iter().all(|item| item["type"] != "agent.message"));
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
        assert!(names.contains(&"meshlet_create_task"));
        assert!(names.contains(&"meshlet_update_task"));
        assert!(names.contains(&"meshlet_send_message"));
        assert!(names.contains(&"meshlet_get_mailbox"));
        assert!(names.contains(&"meshlet_get_timeline"));
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
    fn mcp_v4_tools_create_message_and_timeline() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let create = mcp_request(
            &meshlet,
            "tools/call",
            json!({
                "name": "meshlet_create_task",
                "arguments": {
                    "task_id": "mcp-v4",
                    "title": "MCP v4",
                    "visibility": "public"
                }
            }),
        );
        let created: Event = serde_json::from_str(mcp_content_text(&create))?;
        assert_eq!(created.event_type, "task.created");

        let message = mcp_request(
            &meshlet,
            "tools/call",
            json!({
                "name": "meshlet_send_message",
                "arguments": {
                    "from": "agent:a",
                    "to": "agent:b",
                    "summary": "Handle MCP task",
                    "task_id": "mcp-v4",
                    "visibility": "public"
                }
            }),
        );
        let sent: Event = serde_json::from_str(mcp_content_text(&message))?;
        assert_eq!(sent.event_type, "agent.message");

        let mailbox = mcp_request(
            &meshlet,
            "tools/call",
            json!({
                "name": "meshlet_get_mailbox",
                "arguments": { "agent": "agent:b", "direction": "inbox" }
            }),
        );
        let mailbox_value: Value = serde_json::from_str(mcp_content_text(&mailbox))?;
        assert_eq!(
            mailbox_value["messages"]["items"][0]["summary"],
            "Handle MCP task"
        );

        let timeline = mcp_request(
            &meshlet,
            "tools/call",
            json!({
                "name": "meshlet_get_timeline",
                "arguments": { "task_id": "mcp-v4" }
            }),
        );
        let timeline_value: Value = serde_json::from_str(mcp_content_text(&timeline))?;
        assert_eq!(timeline_value["items"].as_array().expect("items").len(), 2);
        Ok(())
    }

    #[test]
    fn mcp_public_safe_task_reads_exclude_private_tasks() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.create_task(
            Some("visible-task"),
            "Visible",
            None,
            None,
            None,
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        meshlet.create_task(
            Some("hidden-task"),
            "Hidden",
            None,
            None,
            None,
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        let list = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "meshlet_list_tasks",
                "arguments": { "limit": 20 }
            }
        });
        let digest = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "meshlet_get_digest",
                "arguments": { "limit": 20 }
            }
        });

        let list_response =
            handle_mcp_request_with_profile(&meshlet, &list, json!(1), SafetyProfile::PublicSafe);
        let listed: Value = serde_json::from_str(mcp_content_text(&list_response))?;
        let digest_response =
            handle_mcp_request_with_profile(&meshlet, &digest, json!(2), SafetyProfile::PublicSafe);
        let digested: Value = serde_json::from_str(mcp_content_text(&digest_response))?;

        assert_eq!(listed["tasks"].as_array().expect("tasks").len(), 1);
        assert_eq!(listed["tasks"][0]["id"], "visible-task");
        assert_eq!(digested["tasks"].as_array().expect("tasks").len(), 1);
        assert_eq!(digested["counts"]["tasks"], 1);
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
    fn mcp_public_safe_graph_namespaces_resource_filters_visibility() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "graph.imported",
            "agent:test",
            json!({
                "source": "graphify",
                "namespace": "public-ns",
                "nodes": [{"id": "a", "label": "A"}],
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
                "nodes": [{"id": "b", "label": "B"}],
                "links": []
            }),
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "resources/read",
            "params": { "uri": "meshlet://graph/namespaces" }
        });

        let response = handle_mcp_request_with_profile(
            &meshlet,
            &request,
            json!(1),
            SafetyProfile::PublicSafe,
        );
        let value: Value = serde_json::from_str(mcp_resource_text(&response))?;

        assert_eq!(value["namespaces"].as_array().expect("namespaces").len(), 1);
        assert_eq!(value["namespaces"][0], "public-ns");
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
