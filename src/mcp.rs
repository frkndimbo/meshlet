use std::io::{self, BufRead, Write};

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::{DEFAULT_LIMIT, MAX_LIMIT, Meshlet, str_field};

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

pub(crate) fn handle_mcp_request(meshlet: &Meshlet, request: &Value, id: Value) -> Value {
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
        "meshlet_query" => {
            let q = str_field(&args, "q")?;
            let kind = args.get("kind").and_then(Value::as_str);
            let limit = limit_arg(&args)?;
            meshlet.query(q, kind, limit)?
        }
        "meshlet_list_skills" => json!({ "skills": meshlet.list_skills()? }),
        "meshlet_list_tasks" => json!({ "tasks": meshlet.list_tasks(limit_arg(&args)?)? }),
        "meshlet_get_task" => {
            let id = str_field(&args, "id")?;
            meshlet.show_task(id)?
        }
        "meshlet_get_context" => meshlet.context_snapshot_limited(limit_arg(&args)?)?,
        other => bail!("unknown tool: {other}"),
    };
    Ok(json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&payload)? }]
    }))
}

fn mcp_read_resource(meshlet: &Meshlet, uri: &str) -> Result<Value> {
    let value = match uri {
        "meshlet://skills" => json!({ "skills": meshlet.list_skills()? }),
        "meshlet://events/recent" => {
            let events = meshlet.list_events_bounded(DEFAULT_LIMIT)?;
            json!({
                "limit": DEFAULT_LIMIT,
                "events": {
                    "items": events.items,
                    "truncated": events.truncated,
                }
            })
        }
        "meshlet://tasks" => json!({
            "limit": DEFAULT_LIMIT,
            "tasks": meshlet.list_tasks(DEFAULT_LIMIT)?,
        }),
        "meshlet://evidence/recent" => json!({
            "limit": DEFAULT_LIMIT,
            "evidence": meshlet.list_evidence(DEFAULT_LIMIT)?,
        }),
        "meshlet://graph" => {
            let nodes = meshlet.graph_nodes_bounded(None, DEFAULT_LIMIT)?;
            let edges = meshlet.graph_edges_bounded(None, DEFAULT_LIMIT)?;
            json!({
                "limit": DEFAULT_LIMIT,
                "nodes": {
                    "items": nodes.items,
                    "truncated": nodes.truncated,
                },
                "edges": {
                    "items": edges.items,
                    "truncated": edges.truncated,
                }
            })
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
            "name": "meshlet_query",
            "description": "Search local events, graph nodes, and graph edges.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "q": { "type": "string" },
                    "kind": { "type": "string", "enum": ["all", "events", "nodes", "edges"] },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                },
                "required": ["q"]
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

fn clamp_limit(limit: u32) -> u32 {
    limit.clamp(1, MAX_LIMIT)
}
