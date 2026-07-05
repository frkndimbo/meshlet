use serde_json::{Value, json};

use super::sha256_hex;

pub(crate) fn public_safe_evidence_node(node: Value) -> Value {
    let attrs = node.get("attrs").unwrap_or(&Value::Null);
    let mut out = serde_json::Map::new();

    out.insert(
        "id".to_string(),
        node.get("id").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "kind".to_string(),
        node.get("kind")
            .cloned()
            .unwrap_or_else(|| json!("evidence")),
    );
    out.insert(
        "source_event_id".to_string(),
        node.get("source_event_id").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "visibility".to_string(),
        node.get("visibility")
            .cloned()
            .unwrap_or_else(|| json!("public")),
    );

    if let Some(sha256) = attrs.get("sha256").and_then(Value::as_str)
        && is_valid_sha256(sha256)
    {
        out.insert("sha256".to_string(), json!(sha256.to_ascii_lowercase()));
    }
    if let Some(task_id) = attrs.get("task_id").and_then(Value::as_str)
        && is_safe_public_task_id(task_id)
    {
        out.insert("task_id".to_string(), json!(task_id));
    }
    if attrs.get("path").is_some() {
        out.insert("has_path".to_string(), json!(true));
    }
    if attrs.get("ref").is_some() {
        out.insert("has_ref".to_string(), json!(true));
    }

    Value::Object(out)
}

pub(crate) fn public_safe_file_node(node: Value) -> Value {
    let source_event_id = node
        .get("source_event_id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let raw_id = node
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or(source_event_id);
    json!({
        "id": public_safe_file_id(raw_id),
        "kind": "file",
        "label": "file",
        "visibility": node
            .get("visibility")
            .cloned()
            .unwrap_or_else(|| json!("public")),
        "source_event_id": source_event_id,
        "has_path": true,
    })
}

pub(crate) fn public_safe_file_id(raw_id: &str) -> String {
    let digest = sha256_hex(raw_id.as_bytes());
    format!("file:{}", &digest[..16])
}

fn is_valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn is_safe_public_task_id(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ':'))
}

pub(crate) fn public_safe_graph_endpoint_id(value: Option<&Value>) -> Value {
    let Some(id) = value.and_then(Value::as_str) else {
        return Value::Null;
    };
    if id.starts_with("file:") {
        json!(public_safe_file_id(id))
    } else {
        json!(id)
    }
}

pub(crate) fn value_is_public(value: &Value) -> bool {
    value
        .get("visibility")
        .or_else(|| value.get("attrs").and_then(|attrs| attrs.get("visibility")))
        .and_then(Value::as_str)
        == Some("public")
}
