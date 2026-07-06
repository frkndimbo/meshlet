use super::*;

impl Meshlet {
    pub(crate) fn task_exists(&self, id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM tasks WHERE id = ?1", [id], |_| Ok(()))
            .optional()?
            .is_some())
    }

    pub(crate) fn task_status(&self, id: &str) -> Result<Option<String>> {
        self.conn
            .query_row("SELECT status FROM tasks WHERE id = ?1", [id], |row| {
                row.get::<_, String>(0)
            })
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn task_row(&self, id: &str) -> Result<Option<Value>> {
        self.conn
            .query_row(
                "SELECT id, title, status, assignee, note, visibility, created_event_id, updated_event_id, created_at, updated_at, schema_version, attrs_json
                 FROM tasks WHERE id = ?1",
                [id],
                task_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_tasks(&self, limit: u32) -> Result<Vec<Value>> {
        self.list_tasks_scoped(limit, SafetyProfile::LocalTrusted)
    }

    pub fn show_task(&self, id: &str) -> Result<Value> {
        self.show_task_scoped(id, SafetyProfile::LocalTrusted)
    }

    pub fn list_tasks_scoped(&self, limit: u32, profile: SafetyProfile) -> Result<Vec<Value>> {
        if profile == SafetyProfile::PublicSafe {
            let limit = clamp_limit(limit) as usize;
            let mut tasks = self.task_views_from_events(profile)?;
            tasks.truncate(limit);
            return Ok(tasks);
        }
        Ok(self.tasks_bounded(limit, profile)?.items)
    }

    pub fn show_task_scoped(&self, id: &str, profile: SafetyProfile) -> Result<Value> {
        if profile == SafetyProfile::PublicSafe {
            return self
                .task_views_from_events(profile)?
                .into_iter()
                .find(|task| task["id"] == id)
                .ok_or_else(|| anyhow!("task not found: {id}"));
        }
        let visibility = visibility_clause(profile);
        let sql = format!(
            "SELECT id, title, status, assignee, note, visibility, created_event_id, updated_event_id, created_at, updated_at, schema_version, attrs_json
             FROM tasks WHERE id = ?1 AND {visibility}"
        );
        self.conn
            .query_row(&sql, [id], task_from_row)
            .optional()?
            .ok_or_else(|| anyhow!("task not found: {id}"))
    }

    pub(crate) fn tasks_bounded(
        &self,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Bounded<Value>> {
        let limit = clamp_limit(limit);
        let visibility = visibility_clause(profile);
        let sql = format!(
            "SELECT id, title, status, assignee, note, visibility, created_event_id, updated_event_id, created_at, updated_at, schema_version, attrs_json
             FROM tasks
             WHERE {visibility}
             ORDER BY updated_at DESC, id
             LIMIT ?1"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut tasks = stmt
            .query_map([limit + 1], task_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = tasks.len() > limit as usize;
        tasks.truncate(limit as usize);
        Ok(Bounded {
            items: tasks,
            truncated,
        })
    }

    // ponytail: public CLI/MCP facade mirrors user-facing fields.
    #[allow(clippy::too_many_arguments)]
    pub fn create_task(
        &self,
        task_id: Option<&str>,
        title: &str,
        status: Option<&str>,
        assignee: Option<&str>,
        note: Option<&str>,
        visibility: EventVisibility,
        profile: SafetyProfile,
    ) -> Result<Event> {
        let mut payload = serde_json::Map::new();
        if let Some(task_id) = task_id {
            payload.insert("task_id".to_string(), json!(task_id));
        }
        payload.insert("title".to_string(), json!(title));
        if let Some(status) = status {
            payload.insert("status".to_string(), json!(status));
        }
        if let Some(assignee) = assignee {
            payload.insert("assignee".to_string(), json!(assignee));
        }
        if let Some(note) = note {
            payload.insert("note".to_string(), json!(note));
        }
        self.append_event_with_options(
            "task.created",
            "cli",
            Value::Object(payload),
            visibility,
            profile,
        )
    }

    pub fn update_task(
        &self,
        task_id: &str,
        status: Option<&str>,
        assignee: Option<&str>,
        note: Option<&str>,
        visibility: EventVisibility,
        profile: SafetyProfile,
    ) -> Result<Event> {
        let mut payload = serde_json::Map::new();
        payload.insert("task_id".to_string(), json!(task_id));
        if let Some(status) = status {
            payload.insert("status".to_string(), json!(status));
        }
        if let Some(assignee) = assignee {
            payload.insert("assignee".to_string(), json!(assignee));
        }
        if let Some(note) = note {
            payload.insert("note".to_string(), json!(note));
        }
        self.append_event_with_options(
            "task.updated",
            "cli",
            Value::Object(payload),
            visibility,
            profile,
        )
    }

    pub fn task_timeline(
        &self,
        task_id: &str,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Value> {
        if task_id.trim().is_empty() {
            bail!("task_id must not be empty");
        }
        let limit = clamp_limit(limit) as usize;
        let mut items = Vec::new();
        for record in self.event_records_ascending()? {
            if profile == SafetyProfile::PublicSafe
                && record.event.visibility != EventVisibility::Public
            {
                continue;
            }
            if event_task_id(&record.event).as_deref() == Some(task_id) {
                items.push(compact_timeline_event(record.seq, &record.event));
            }
        }
        let truncated = items.len() > limit;
        items.truncate(limit);
        Ok(json!({
            "task_id": task_id,
            "profile": profile.as_str(),
            "limit": limit,
            "items": items,
            "truncated": truncated,
        }))
    }

    pub(crate) fn task_views_from_events(&self, profile: SafetyProfile) -> Result<Vec<Value>> {
        let mut tasks: BTreeMap<String, serde_json::Map<String, Value>> = BTreeMap::new();
        for event in self.events_ascending()? {
            if profile == SafetyProfile::PublicSafe && event.visibility != EventVisibility::Public {
                continue;
            }
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
                    merge_task_payload(task, &event, true);
                }
                "task.updated" => {
                    let Some(id) = event.payload.get("task_id").and_then(Value::as_str) else {
                        continue;
                    };
                    let task = tasks.entry(id.to_string()).or_default();
                    task.entry("id").or_insert_with(|| json!(id));
                    merge_task_payload(task, &event, false);
                }
                _ => {}
            }
        }
        let mut tasks = tasks.into_values().map(Value::Object).collect::<Vec<_>>();
        tasks.sort_by(|a, b| {
            b.get("updated_at")
                .and_then(Value::as_str)
                .cmp(&a.get("updated_at").and_then(Value::as_str))
                .then_with(|| {
                    a.get("id")
                        .and_then(Value::as_str)
                        .cmp(&b.get("id").and_then(Value::as_str))
                })
        });
        Ok(tasks)
    }

    pub(crate) fn apply_task(&self, event: &Event) -> Result<()> {
        let task_id = event
            .payload
            .get("task_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| event.id.clone());
        let existing = self.task_row(&task_id)?;
        let title = match event.event_type.as_str() {
            "task.created" => required_nonempty_string(&event.payload, "title")?.to_string(),
            _ => existing
                .as_ref()
                .and_then(|task| task.get("title").and_then(Value::as_str))
                .or_else(|| nonempty_string(&event.payload, "title"))
                .unwrap_or(&task_id)
                .to_string(),
        };
        let status = nonempty_string(&event.payload, "status")
            .map(ToOwned::to_owned)
            .or_else(|| {
                existing
                    .as_ref()
                    .and_then(|task| task.get("status").and_then(Value::as_str))
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| "open".to_string());
        let assignee = nonempty_string(&event.payload, "assignee")
            .map(ToOwned::to_owned)
            .or_else(|| {
                existing
                    .as_ref()
                    .and_then(|task| task.get("assignee").and_then(Value::as_str))
                    .map(ToOwned::to_owned)
            });
        let note = event
            .payload
            .get("note")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| {
                existing
                    .as_ref()
                    .and_then(|task| task.get("note").and_then(Value::as_str))
                    .map(ToOwned::to_owned)
            });
        self.conn.execute(
            "INSERT INTO tasks(
                id, title, status, assignee, note, visibility, created_event_id,
                updated_event_id, created_at, updated_at, schema_version, attrs_json
             )
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                status = excluded.status,
                assignee = excluded.assignee,
                note = excluded.note,
                visibility = excluded.visibility,
                updated_event_id = excluded.updated_event_id,
                updated_at = excluded.updated_at,
                schema_version = excluded.schema_version,
                attrs_json = excluded.attrs_json",
            params![
                task_id,
                title,
                status,
                assignee,
                note,
                event.visibility.as_str(),
                existing
                    .as_ref()
                    .and_then(|task| task.get("created_event_id").and_then(Value::as_str))
                    .unwrap_or(&event.id),
                event.id,
                existing
                    .as_ref()
                    .and_then(|task| task.get("created_at").and_then(Value::as_str))
                    .unwrap_or(&event.created_at),
                event.created_at,
                TASK_SCHEMA_VERSION,
                canonical_json(&attrs_with_visibility(
                    event.payload.clone(),
                    event.visibility
                ))?,
            ],
        )?;
        self.upsert_node(
            &format!("task:{task_id}"),
            "task",
            Some(&title),
            attrs_with_visibility(event.payload.clone(), event.visibility),
            &event.id,
            event.visibility,
        )?;
        Ok(())
    }
}
