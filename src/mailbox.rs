use super::*;

impl Meshlet {
    pub fn send_agent_message(
        &self,
        from_agent: &str,
        to_agent: &str,
        summary: &str,
        task_id: Option<&str>,
        body: Option<&str>,
        reply_to: Option<&str>,
        visibility: EventVisibility,
        profile: SafetyProfile,
    ) -> Result<Event> {
        let mut payload = serde_json::Map::new();
        payload.insert("from".to_string(), json!(from_agent));
        payload.insert("to".to_string(), json!(to_agent));
        payload.insert("summary".to_string(), json!(summary));
        if let Some(task_id) = task_id {
            payload.insert("task_id".to_string(), json!(task_id));
        }
        if let Some(body) = body {
            payload.insert("body".to_string(), json!(body));
        }
        if let Some(reply_to) = reply_to {
            payload.insert("reply_to".to_string(), json!(reply_to));
        }
        self.append_event_with_options(
            "agent.message",
            from_agent,
            Value::Object(payload),
            visibility,
            profile,
        )
    }

    pub fn list_mailbox(
        &self,
        agent: &str,
        direction: &str,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Value> {
        if agent.trim().is_empty() {
            bail!("agent must not be empty");
        }
        let column = match direction {
            "inbox" => "to_agent",
            "outbox" => "from_agent",
            _ => bail!("mailbox direction must be inbox or outbox"),
        };
        let limit = clamp_limit(limit);
        let visibility = visibility_clause(profile);
        let sql = format!(
            "SELECT id, from_agent, to_agent, task_id, summary, body, reply_to, visibility, source_event_id, created_at, schema_version, attrs_json
             FROM mailbox_messages
             WHERE {column} = ?1 AND {visibility}
             ORDER BY created_at DESC, id
             LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut messages = stmt
            .query_map(params![agent, limit + 1], message_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = messages.len() > limit as usize;
        messages.truncate(limit as usize);
        if profile == SafetyProfile::PublicSafe {
            messages = messages
                .into_iter()
                .map(public_mailbox_message)
                .collect::<Vec<_>>();
        }
        Ok(json!({
            "agent": agent,
            "direction": direction,
            "limit": limit,
            "messages": {
                "items": messages,
                "truncated": truncated,
            },
        }))
    }

    pub(crate) fn apply_agent_message(&self, event: &Event) -> Result<()> {
        let from_agent = nonempty_string(&event.payload, "from")
            .or_else(|| nonempty_string(&event.payload, "agent"))
            .unwrap_or(&event.actor);
        let to_agent = nonempty_string(&event.payload, "to").unwrap_or("agent:unknown");
        let summary = nonempty_string(&event.payload, "summary")
            .or_else(|| nonempty_string(&event.payload, "label"))
            .or_else(|| nonempty_string(&event.payload, "note"))
            .unwrap_or("agent.message");
        let task_id = nonempty_string(&event.payload, "task_id");
        let body = event.payload.get("body").and_then(Value::as_str);
        let reply_to = nonempty_string(&event.payload, "reply_to");
        let agent_id = format!("agent:{from_agent}");
        let to_agent_id = format!("agent:{to_agent}");
        let message_id = format!("message:{}", event.id);
        self.conn.execute(
            "INSERT OR REPLACE INTO mailbox_messages(
                id, from_agent, to_agent, task_id, summary, body, reply_to, visibility,
                source_event_id, created_at, schema_version, attrs_json
             )
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                message_id,
                from_agent,
                to_agent,
                task_id,
                summary,
                body,
                reply_to,
                event.visibility.as_str(),
                event.id,
                event.created_at,
                MESSAGE_SCHEMA_VERSION,
                canonical_json(&attrs_with_visibility(
                    event.payload.clone(),
                    event.visibility
                ))?,
            ],
        )?;
        self.upsert_node(
            &agent_id,
            "agent",
            Some(from_agent),
            json!({ "name": from_agent }),
            &event.id,
            event.visibility,
        )?;
        self.upsert_node(
            &to_agent_id,
            "agent",
            Some(to_agent),
            json!({ "name": to_agent }),
            &event.id,
            event.visibility,
        )?;
        self.upsert_node(
            &message_id,
            "message",
            Some(summary),
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
        self.upsert_edge(
            &format!("edge:{}:message-to-agent", event.id),
            &message_id,
            &to_agent_id,
            "references",
            json!({ "field": "to" }),
            &event.id,
            event.visibility,
        )?;
        if let Some(task_id) = task_id {
            self.upsert_edge(
                &format!("edge:{}:message-task", event.id),
                &message_id,
                &format!("task:{task_id}"),
                "references",
                json!({ "field": "task_id" }),
                &event.id,
                event.visibility,
            )?;
        }
        Ok(())
    }
}

fn public_mailbox_message(message: Value) -> Value {
    let mut out = serde_json::Map::new();
    for key in [
        "id",
        "from",
        "to",
        "task_id",
        "summary",
        "created_at",
        "visibility",
        "source_event_id",
    ] {
        insert_public_mailbox_string(&mut out, &message, key);
    }
    Value::Object(out)
}

fn insert_public_mailbox_string(
    out: &mut serde_json::Map<String, Value>,
    message: &Value,
    key: &str,
) {
    let Some(value) = message.get(key).and_then(Value::as_str) else {
        return;
    };
    let report = scan_payload_safety(&json!(value));
    if report.blocked_keys.is_empty() && report.suspicious_values.is_empty() {
        out.insert(key.to_string(), json!(value));
    }
}
