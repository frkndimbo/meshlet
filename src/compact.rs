use serde_json::{Value, json};

use super::public_safe::{
    public_safe_evidence_node, public_safe_file_node, public_safe_graph_endpoint_id,
    value_is_public,
};
use super::{Event, EventVisibility, SafetyProfile, nonempty_string};

pub(crate) fn compact_timeline_event(seq: i64, event: &Event, profile: SafetyProfile) -> Value {
    let mut payload = serde_json::Map::new();
    for key in [
        "task_id", "title", "status", "assignee", "note", "from", "to", "summary", "reply_to",
    ] {
        if let Some(value) = event.payload.get(key) {
            payload.insert(key.to_string(), value.clone());
        }
    }
    if profile == SafetyProfile::PublicSafe {
        if event.payload.get("path").is_some() {
            payload.insert("has_path".to_string(), json!(true));
        }
        if event.payload.get("ref").is_some() {
            payload.insert("has_ref".to_string(), json!(true));
        }
        if let Some(sha256) = event.payload.get("sha256").and_then(Value::as_str)
            && sha256.len() == 64
            && sha256.chars().all(|ch| ch.is_ascii_hexdigit())
        {
            payload.insert("sha256".to_string(), json!(sha256.to_ascii_lowercase()));
        }
    } else {
        for key in ["path", "ref"] {
            if let Some(value) = event.payload.get(key) {
                payload.insert(key.to_string(), value.clone());
            }
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

pub(crate) fn attrs_with_visibility(attrs: Value, visibility: EventVisibility) -> Value {
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

pub(crate) fn compact_event_text(event: &Event) -> String {
    [
        "label", "title", "summary", "status", "name", "assignee", "from", "to",
    ]
    .into_iter()
    .filter_map(|key| nonempty_string(&event.payload, key))
    .collect::<Vec<_>>()
    .join(" ")
}

pub(crate) fn compact_event(event: &Event) -> Value {
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

pub(crate) fn compact_node(node: Value) -> Value {
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

pub(crate) fn compact_node_for_profile(node: Value, profile: SafetyProfile) -> Value {
    if profile == SafetyProfile::PublicSafe {
        match node.get("kind").and_then(Value::as_str) {
            Some("evidence") => return public_safe_evidence_node(node),
            Some("file") => return public_safe_file_node(node),
            _ => {}
        }
    }
    compact_node(node)
}

pub(crate) fn compact_edge(edge: Value) -> Value {
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

pub(crate) fn compact_edge_for_profile(edge: Value, profile: SafetyProfile) -> Value {
    if profile != SafetyProfile::PublicSafe {
        return compact_edge(edge);
    }
    json!({
        "id": edge.get("id").cloned().unwrap_or(Value::Null),
        "from_id": public_safe_graph_endpoint_id(edge.get("from_id")),
        "to_id": public_safe_graph_endpoint_id(edge.get("to_id")),
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

pub(crate) fn compact_message(message: Value) -> Value {
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

pub(crate) fn compact_query(mut value: Value, profile: SafetyProfile) -> Value {
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
                        Some(compact_node_for_profile(item, profile))
                    } else {
                        None
                    }
                }
                "edges" => {
                    if profile != SafetyProfile::PublicSafe || value_is_public(&item) {
                        Some(compact_edge_for_profile(item, profile))
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
