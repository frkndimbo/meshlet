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
            "namespaces": meshlet.graph_namespaces_scoped(profile)?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Event;
    use std::fs;
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

        for (index, params) in [
            json!({
                "name": "meshlet_publish_event",
                "arguments": {
                    "type": "context.added",
                    "visibility": "public",
                    "payload": { "label": "public" }
                }
            }),
            json!({
                "name": "meshlet_create_task",
                "arguments": { "title": "public task", "visibility": "public" }
            }),
            json!({
                "name": "meshlet_update_task",
                "arguments": { "id": "task-1", "status": "done" }
            }),
            json!({
                "name": "meshlet_send_message",
                "arguments": { "from": "agent:a", "to": "agent:b", "summary": "blocked" }
            }),
            json!({ "name": "meshlet_get_context", "arguments": {} }),
            json!({ "name": "meshlet_query", "arguments": { "q": "repo", "mode": "full" } }),
        ]
        .into_iter()
        .enumerate()
        {
            let request = json!({
                "jsonrpc": "2.0",
                "id": index + 1,
                "method": "tools/call",
                "params": params
            });
            let response = handle_mcp_request_with_profile(
                &meshlet,
                &request,
                json!(index + 1),
                SafetyProfile::PublicSafe,
            );

            assert_eq!(response["error"]["code"], -32602);
        }
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
