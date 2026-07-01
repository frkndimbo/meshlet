use std::io::{self, BufRead, Write};

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::{
    DEFAULT_LIMIT, EventVisibility, MAX_LIMIT, Meshlet, OutputMode, SafetyProfile, str_field,
};

pub fn run_mcp_stdio(meshlet: Meshlet) -> Result<()> {
    run_mcp_stdio_with_profile(meshlet, SafetyProfile::LocalTrusted)
}

pub fn run_mcp_stdio_with_profile(meshlet: Meshlet, profile: SafetyProfile) -> Result<()> {
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
        let response = handle_mcp_request_with_profile(&meshlet, &request, id, profile);
        write_json(&mut stdout, &response)?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn handle_mcp_request(meshlet: &Meshlet, request: &Value, id: Value) -> Value {
    handle_mcp_request_with_profile(meshlet, request, id, SafetyProfile::LocalTrusted)
}

pub(crate) fn handle_mcp_request_with_profile(
    meshlet: &Meshlet,
    request: &Value,
    id: Value,
    profile: SafetyProfile,
) -> Value {
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
            match mcp_tool_call(meshlet, &params, profile) {
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
            match mcp_read_resource(meshlet, uri, profile) {
                Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Err(error) => json_rpc_error(id, -32602, &error.to_string()),
            }
        }
        _ => json_rpc_error(id, -32601, "method not found"),
    }
}

fn mcp_tool_call(meshlet: &Meshlet, params: &Value, profile: SafetyProfile) -> Result<Value> {
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
            if profile == SafetyProfile::PublicSafe {
                bail!("meshlet_publish_event is disabled in public-safe profile");
            }
            let event_type = str_field(&args, "type")?;
            let actor = args.get("actor").and_then(Value::as_str).unwrap_or("agent");
            let visibility = optional_string_field(&args, "visibility")?
                .map(str::parse::<EventVisibility>)
                .transpose()?
                .unwrap_or(EventVisibility::Private);
            let payload = args
                .get("payload")
                .filter(|payload| payload.is_object())
                .cloned()
                .ok_or_else(|| anyhow!("payload object is required"))?;
            serde_json::to_value(
                meshlet
                    .append_event_with_options(event_type, actor, payload, visibility, profile)?,
            )?
        }
        "meshlet_query" => {
            let q = str_field(&args, "q")?;
            let kind = args.get("kind").and_then(Value::as_str);
            let namespace = optional_string_field(&args, "namespace")?;
            let limit = limit_arg(&args)?;
            let mode = optional_string_field(&args, "mode")?
                .map(str::parse::<OutputMode>)
                .transpose()?
                .unwrap_or(OutputMode::Compact);
            meshlet.query_scoped_view(q, kind, namespace, limit, mode, profile)?
        }
        "meshlet_list_skills" => json!({ "skills": meshlet.list_skills_scoped(profile)? }),
        "meshlet_list_tasks" => {
            json!({ "tasks": meshlet.list_tasks_scoped(limit_arg(&args)?, profile)? })
        }
        "meshlet_get_task" => {
            let id = str_field(&args, "id")?;
            meshlet.show_task_scoped(id, profile)?
        }
        "meshlet_get_digest" => meshlet.context_digest_limited(limit_arg(&args)?, profile)?,
        "meshlet_create_task" => {
            if profile == SafetyProfile::PublicSafe {
                bail!("meshlet_create_task is disabled in public-safe profile");
            }
            serde_json::to_value(
                meshlet.create_task(
                    optional_string_field(&args, "task_id")?,
                    str_field(&args, "title")?,
                    optional_string_field(&args, "status")?,
                    optional_string_field(&args, "assignee")?,
                    optional_string_field(&args, "note")?,
                    optional_string_field(&args, "visibility")?
                        .map(str::parse::<EventVisibility>)
                        .transpose()?
                        .unwrap_or(EventVisibility::Private),
                    profile,
                )?,
            )?
        }
        "meshlet_update_task" => {
            if profile == SafetyProfile::PublicSafe {
                bail!("meshlet_update_task is disabled in public-safe profile");
            }
            serde_json::to_value(
                meshlet.update_task(
                    str_field(&args, "id")?,
                    optional_string_field(&args, "status")?,
                    optional_string_field(&args, "assignee")?,
                    optional_string_field(&args, "note")?,
                    optional_string_field(&args, "visibility")?
                        .map(str::parse::<EventVisibility>)
                        .transpose()?
                        .unwrap_or(EventVisibility::Private),
                    profile,
                )?,
            )?
        }
        "meshlet_send_message" => {
            if profile == SafetyProfile::PublicSafe {
                bail!("meshlet_send_message is disabled in public-safe profile");
            }
            serde_json::to_value(
                meshlet.send_agent_message(
                    str_field(&args, "from")?,
                    str_field(&args, "to")?,
                    str_field(&args, "summary")?,
                    optional_string_field(&args, "task_id")?,
                    optional_string_field(&args, "body")?,
                    optional_string_field(&args, "reply_to")?,
                    optional_string_field(&args, "visibility")?
                        .map(str::parse::<EventVisibility>)
                        .transpose()?
                        .unwrap_or(EventVisibility::Private),
                    profile,
                )?,
            )?
        }
        "meshlet_get_mailbox" => meshlet.list_mailbox(
            str_field(&args, "agent")?,
            optional_string_field(&args, "direction")?.unwrap_or("inbox"),
            limit_arg(&args)?,
            profile,
        )?,
        "meshlet_get_timeline" => {
            meshlet.task_timeline(str_field(&args, "task_id")?, limit_arg(&args)?, profile)?
        }
        "meshlet_get_context" => {
            if profile == SafetyProfile::PublicSafe {
                bail!(
                    "meshlet_get_context is disabled in public-safe profile; use meshlet_get_digest"
                );
            }
            meshlet.context_snapshot_limited(limit_arg(&args)?)?
        }
        other => bail!("unknown tool: {other}"),
    };
    Ok(json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&payload)? }]
    }))
}

fn mcp_read_resource(meshlet: &Meshlet, uri: &str, profile: SafetyProfile) -> Result<Value> {
    let value = match uri {
        "meshlet://skills" => json!({ "skills": meshlet.list_skills_scoped(profile)? }),
        "meshlet://events/recent" => {
            meshlet.context_digest_limited(DEFAULT_LIMIT, profile)?["events_recent"].clone()
        }
        "meshlet://tasks" => json!({
            "limit": DEFAULT_LIMIT,
            "tasks": meshlet.list_tasks_scoped(DEFAULT_LIMIT, profile)?,
        }),
        "meshlet://evidence/recent" => json!({
            "limit": DEFAULT_LIMIT,
            "evidence": meshlet.list_evidence_scoped(DEFAULT_LIMIT, profile)?,
        }),
        "meshlet://graph/namespaces" => json!({
            "namespaces": meshlet.graph_namespaces()?,
        }),
        "meshlet://graph" => {
            meshlet.context_digest_limited(DEFAULT_LIMIT, profile)?["graph"].clone()
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
            "name": "meshlet_list_tasks",
            "description": "List local Meshlet tasks.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                }
            }
        },
        {
            "name": "meshlet_get_task",
            "description": "Read one local Meshlet task.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "meshlet_create_task",
            "description": "Create a typed local Meshlet task event.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "title": { "type": "string" },
                    "status": { "type": "string", "enum": ["open", "in_progress", "blocked", "done", "canceled"] },
                    "assignee": { "type": "string" },
                    "note": { "type": "string" },
                    "visibility": { "type": "string", "enum": ["private", "local", "public"] }
                },
                "required": ["title"]
            }
        },
        {
            "name": "meshlet_update_task",
            "description": "Update a typed local Meshlet task event.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "status": { "type": "string", "enum": ["open", "in_progress", "blocked", "done", "canceled"] },
                    "assignee": { "type": "string" },
                    "note": { "type": "string" },
                    "visibility": { "type": "string", "enum": ["private", "local", "public"] }
                },
                "required": ["id"]
            }
        },
        {
            "name": "meshlet_send_message",
            "description": "Send a typed local agent message.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": { "type": "string" },
                    "to": { "type": "string" },
                    "summary": { "type": "string" },
                    "task_id": { "type": "string" },
                    "body": { "type": "string" },
                    "reply_to": { "type": "string" },
                    "visibility": { "type": "string", "enum": ["private", "local", "public"] }
                },
                "required": ["from", "to", "summary"]
            }
        },
        {
            "name": "meshlet_get_mailbox",
            "description": "Read an agent inbox or outbox.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent": { "type": "string" },
                    "direction": { "type": "string", "enum": ["inbox", "outbox"] },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                },
                "required": ["agent"]
            }
        },
        {
            "name": "meshlet_get_timeline",
            "description": "Replay a compact task timeline.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                },
                "required": ["task_id"]
            }
        },
        {
            "name": "meshlet_query",
            "description": "Search local events, graph nodes, and graph edges.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "q": { "type": "string" },
                    "kind": { "type": "string", "enum": ["all", "events", "nodes", "edges"] },
                    "namespace": { "type": "string" },
                    "mode": { "type": "string", "enum": ["compact", "full"] },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                },
                "required": ["q"]
            }
        },
        {
            "name": "meshlet_get_digest",
            "description": "Read compact public-safe Meshlet state digest.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                }
            }
        },
        {
            "name": "meshlet_get_context",
            "description": "Read recent events, skills, and graph materialization.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                }
            }
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
            "uri": "meshlet://tasks",
            "name": "Meshlet Tasks",
            "description": "Latest local task state.",
            "mimeType": "application/json"
        },
        {
            "uri": "meshlet://evidence/recent",
            "name": "Recent Meshlet Evidence",
            "description": "Recent evidence nodes.",
            "mimeType": "application/json"
        },
        {
            "uri": "meshlet://graph/namespaces",
            "name": "Meshlet Graph Namespaces",
            "description": "Known graph namespaces.",
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

fn limit_arg(value: &Value) -> Result<u32> {
    let Some(raw_limit) = value.get("limit") else {
        return Ok(DEFAULT_LIMIT);
    };
    let limit = raw_limit
        .as_u64()
        .ok_or_else(|| anyhow!("limit must be a positive integer"))?;
    if limit == 0 {
        bail!("limit must be a positive integer");
    }
    Ok(clamp_limit(u32::try_from(limit).unwrap_or(MAX_LIMIT)))
}

fn optional_string_field<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>> {
    match value.get(key) {
        Some(raw) => raw
            .as_str()
            .map(Some)
            .ok_or_else(|| anyhow!("{key} must be a string")),
        None => Ok(None),
    }
}

fn clamp_limit(limit: u32) -> u32 {
    limit.clamp(1, MAX_LIMIT)
}
