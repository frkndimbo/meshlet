use super::*;

fn require_public_doctor_ok(doctor: &Value) -> Result<()> {
    let ok = doctor.get("ok").and_then(Value::as_bool) == Some(true);
    if !ok {
        bail!("public export blocked: public_doctor ok=false");
    }
    Ok(())
}

impl Meshlet {
    pub fn public_export(&self, limit: u32) -> Result<Value> {
        let limit = clamp_limit(limit);
        let public_report = self.public_doctor()?;
        require_public_doctor_ok(&public_report)?;
        let events = self
            .list_events_bounded_scoped(limit, SafetyProfile::PublicSafe)?
            .items
            .into_iter()
            .map(|event| compact_event(&event))
            .collect::<Vec<_>>();
        let tasks = self.list_tasks_scoped(limit, SafetyProfile::PublicSafe)?;
        let messages = self.public_messages_bounded(limit)?;
        let timelines = tasks
            .iter()
            .filter_map(|task| string_value(task, "id"))
            .map(|task_id| self.task_timeline(task_id, limit, SafetyProfile::PublicSafe))
            .collect::<Result<Vec<_>>>()?;
        let nodes = self
            .graph_nodes_bounded(None, limit, SafetyProfile::PublicSafe)?
            .items
            .into_iter()
            .map(|node| compact_node_for_profile(node, SafetyProfile::PublicSafe))
            .collect::<Vec<_>>();
        let edges = self
            .graph_edges_bounded(None, limit, SafetyProfile::PublicSafe)?
            .items
            .into_iter()
            .map(|edge| compact_edge_for_profile(edge, SafetyProfile::PublicSafe))
            .collect::<Vec<_>>();
        Ok(json!({
            "format": "meshlet-public-export-v1",
            "profile": "public-safe",
            "limit": limit,
            "events": events,
            "tasks": tasks,
            "mailbox": {
                "messages": {
                    "items": messages.items.into_iter().map(compact_message).collect::<Vec<_>>(),
                    "truncated": messages.truncated,
                }
            },
            "timelines": timelines,
            "graph": {
                "nodes": nodes,
                "edges": edges,
            },
            "redaction_report": public_report["redaction_report"].clone(),
        }))
    }

    pub fn public_export_okf(&self, out_dir: impl AsRef<Path>, limit: u32) -> Result<Value> {
        let out_dir = out_dir.as_ref();
        let limit = clamp_limit(limit);
        let public_report = self.public_doctor()?;
        require_public_doctor_ok(&public_report)?;
        prepare_okf_output_dir(out_dir)?;
        let events = self
            .list_events(limit)?
            .into_iter()
            .filter(|event| event.visibility == EventVisibility::Public)
            .collect::<Vec<_>>();
        let contexts = self.list_contexts_limited(limit, SafetyProfile::PublicSafe)?;
        let tasks = self.list_tasks_scoped(limit, SafetyProfile::PublicSafe)?;
        let skills = self.list_skills_scoped(SafetyProfile::PublicSafe)?;
        let evidence = self.list_evidence_scoped(limit, SafetyProfile::PublicSafe)?;
        let messages = self.public_messages_bounded(limit)?;
        let edges = self
            .graph_edges_bounded(None, limit, SafetyProfile::PublicSafe)?
            .items;
        let mut documents = Vec::new();

        for context in contexts {
            let id = string_value(&context, "id").unwrap_or("context:unknown");
            let title = string_value(&context, "title")
                .or_else(|| string_value(&context, "summary"))
                .unwrap_or(id);
            let description = string_value(&context, "summary").unwrap_or(title);
            let mut body = String::new();
            body.push_str(description);
            body.push_str("\n\n## Context\n");
            body.push_str(&metadata_line("Kind", string_value(&context, "kind")));
            body.push_str(&metadata_line(
                "Namespace",
                string_value(&context, "namespace"),
            ));
            documents.push(OkfDocument {
                id: id.to_string(),
                item_type: "Meshlet Context".to_string(),
                title: title.to_string(),
                description: description.to_string(),
                resource: format!("meshlet://contexts/{id}"),
                tags: vec!["meshlet".to_string(), "context".to_string()],
                timestamp: string_value(&context, "updated_at")
                    .or_else(|| string_value(&context, "created_at"))
                    .unwrap_or("")
                    .to_string(),
                source_event_id: string_value(&context, "source_event_id")
                    .unwrap_or("")
                    .to_string(),
                visibility: "public".to_string(),
                relative_path: format!("contexts/{}.md", okf_slug(id)),
                body,
            });
        }

        for task in tasks {
            let id = string_value(&task, "id").unwrap_or("task:unknown");
            let title = string_value(&task, "title").unwrap_or(id);
            let description = string_value(&task, "note")
                .or_else(|| string_value(&task, "status"))
                .unwrap_or(title);
            let mut body = String::new();
            body.push_str(description);
            body.push_str("\n\n## Task\n");
            body.push_str(&metadata_line("Status", string_value(&task, "status")));
            body.push_str(&metadata_line("Assignee", string_value(&task, "assignee")));
            let timeline = self.task_timeline(id, limit, SafetyProfile::PublicSafe)?;
            if let Some(items) = timeline.get("items").and_then(Value::as_array)
                && !items.is_empty()
            {
                body.push_str("\n## Timeline\n");
                for item in items {
                    let event_type = string_value(item, "type").unwrap_or("event");
                    let created_at = string_value(item, "created_at").unwrap_or("");
                    let event_id = string_value(item, "id").unwrap_or("");
                    body.push_str(&format!("- {created_at} `{event_type}` `{event_id}`\n"));
                }
            }
            documents.push(OkfDocument {
                id: format!("task:{id}"),
                item_type: "Meshlet Task".to_string(),
                title: title.to_string(),
                description: description.to_string(),
                resource: format!("meshlet://tasks/{id}"),
                tags: vec!["meshlet".to_string(), "task".to_string()],
                timestamp: string_value(&task, "updated_at")
                    .or_else(|| string_value(&task, "created_at"))
                    .unwrap_or("")
                    .to_string(),
                source_event_id: string_value(&task, "updated_event_id")
                    .or_else(|| string_value(&task, "created_event_id"))
                    .unwrap_or("")
                    .to_string(),
                visibility: "public".to_string(),
                relative_path: format!("tasks/{}.md", okf_slug(id)),
                body,
            });
        }

        for message in &messages.items {
            let id = string_value(message, "id").unwrap_or("message:unknown");
            let summary = string_value(message, "summary").unwrap_or(id);
            let mut body = String::new();
            body.push_str(summary);
            body.push_str("\n\n## Message\n");
            body.push_str(&metadata_line("From", string_value(message, "from")));
            body.push_str(&metadata_line("To", string_value(message, "to")));
            body.push_str(&metadata_line("Task", string_value(message, "task_id")));
            body.push_str(&metadata_line(
                "Reply To",
                string_value(message, "reply_to"),
            ));
            documents.push(OkfDocument {
                id: id.to_string(),
                item_type: "Meshlet Message".to_string(),
                title: summary.to_string(),
                description: summary.to_string(),
                resource: format!("meshlet://messages/{id}"),
                tags: vec!["meshlet".to_string(), "message".to_string()],
                timestamp: string_value(message, "created_at")
                    .unwrap_or("")
                    .to_string(),
                source_event_id: string_value(message, "source_event_id")
                    .unwrap_or("")
                    .to_string(),
                visibility: "public".to_string(),
                relative_path: format!("messages/{}.md", okf_slug(id)),
                body,
            });
        }

        for skill in skills {
            let name = string_value(&skill, "name").unwrap_or("skill");
            let description = string_value(&skill, "description").unwrap_or(name);
            let mut body = String::new();
            body.push_str(description);
            body.push_str("\n\n## Skill\n");
            body.push_str(&metadata_line("Version", string_value(&skill, "version")));
            body.push_str(&metadata_line("Entry", string_value(&skill, "entry")));
            body.push_str(&metadata_line(
                "Permissions",
                skill
                    .get("permissions")
                    .map(|value| value.to_string())
                    .as_deref(),
            ));
            documents.push(OkfDocument {
                id: format!("skill:{name}"),
                item_type: "Meshlet Skill".to_string(),
                title: name.to_string(),
                description: description.to_string(),
                resource: format!("meshlet://skills/{name}"),
                tags: vec!["meshlet".to_string(), "skill".to_string()],
                timestamp: String::new(),
                source_event_id: string_value(&skill, "source_event_id")
                    .unwrap_or("")
                    .to_string(),
                visibility: "public".to_string(),
                relative_path: format!("skills/{}.md", okf_slug(name)),
                body,
            });
        }

        for item in evidence {
            let id = string_value(&item, "id").unwrap_or("evidence:unknown");
            let sha256 = string_value(&item, "sha256").unwrap_or("");
            let mut body = String::new();
            body.push_str("Public-safe evidence reference.");
            body.push_str("\n\n# Citations\n");
            body.push_str(&format!("- `meshlet://evidence/{id}`"));
            if !sha256.is_empty() {
                body.push_str(&format!(" sha256 `{sha256}`"));
            }
            body.push('\n');
            if item.get("has_path").and_then(Value::as_bool) == Some(true) {
                body.push_str("- Local path: redacted\n");
            }
            if item.get("has_ref").and_then(Value::as_bool) == Some(true) {
                body.push_str("- Reference: redacted\n");
            }
            documents.push(OkfDocument {
                id: id.to_string(),
                item_type: "Meshlet Evidence".to_string(),
                title: id.to_string(),
                description: "Public-safe evidence reference".to_string(),
                resource: format!("meshlet://evidence/{id}"),
                tags: vec!["meshlet".to_string(), "evidence".to_string()],
                timestamp: String::new(),
                source_event_id: string_value(&item, "source_event_id")
                    .unwrap_or("")
                    .to_string(),
                visibility: "public".to_string(),
                relative_path: format!("evidence/{}.md", okf_slug(id)),
                body,
            });
        }

        let path_by_id = documents
            .iter()
            .map(|doc| (doc.id.clone(), doc.relative_path.clone()))
            .collect::<BTreeMap<_, _>>();
        for dir in ["contexts", "tasks", "messages", "skills", "evidence"] {
            fs::create_dir_all(out_dir.join(dir))?;
        }
        fs::write(out_dir.join("index.md"), okf_index(&documents))?;
        fs::write(out_dir.join("log.md"), okf_log(&events))?;
        for doc in &documents {
            let relations = okf_relation_lines(&doc.id, &edges, &path_by_id, &doc.relative_path);
            fs::write(
                out_dir.join(&doc.relative_path),
                okf_document_text(doc, &relations),
            )?;
        }

        Ok(json!({
            "format": "meshlet-okf-public-export-v1",
            "profile": "public-safe",
            "path": out_dir.display().to_string(),
            "limit": limit,
            "documents": documents.len(),
            "events": events.len(),
            "messages": messages.items.len(),
            "public_doctor": public_report,
        }))
    }

    pub(crate) fn public_messages_bounded(&self, limit: u32) -> Result<Bounded<Value>> {
        let limit = clamp_limit(limit);
        let mut stmt = self.conn.prepare(
            "SELECT id, from_agent, to_agent, task_id, summary, body, reply_to, visibility, source_event_id, created_at, schema_version, attrs_json
             FROM mailbox_messages
             WHERE visibility = 'public'
             ORDER BY created_at DESC, id
             LIMIT ?1",
        )?;
        let mut messages = stmt
            .query_map(params![limit + 1], message_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = messages.len() > limit as usize;
        messages.truncate(limit as usize);
        Ok(Bounded {
            items: messages,
            truncated,
        })
    }

    pub fn public_doctor(&self) -> Result<Value> {
        let chain = self.verify_event_chain()?;
        let events = self.events_ascending()?;
        let mut report = RedactionReport::default();
        let mut visibility_counts: BTreeMap<String, u64> = BTreeMap::new();
        for event in &events {
            *visibility_counts
                .entry(event.visibility.as_str().to_string())
                .or_insert(0) += 1;
            report.merge(scan_payload_safety(&event.payload));
        }
        report.merge(self.scan_graph_attrs_safety()?);
        let ok = chain.ok && report.blocked_keys.is_empty() && report.suspicious_values.is_empty();
        Ok(json!({
            "ok": ok,
            "chain": chain,
            "events": events.len(),
            "visibility": visibility_counts,
            "redaction_report": report,
            "blockers": if ok { json!([]) } else { json!(["stored state is not public-safe"]) },
        }))
    }

    pub(crate) fn scan_graph_attrs_safety(&self) -> Result<RedactionReport> {
        let mut report = RedactionReport::default();
        for table in ["graph_nodes", "graph_edges"] {
            let mut stmt = self
                .conn
                .prepare(&format!("SELECT attrs_json FROM {table}"))?;
            let attrs = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for attrs_json in attrs {
                let attrs: Value = serde_json::from_str(&attrs_json)
                    .with_context(|| format!("parse {table} attrs_json"))?;
                report.merge(scan_payload_safety(&attrs));
            }
        }
        Ok(report)
    }

    pub fn okf_doctor(bundle_dir: impl AsRef<Path>) -> Result<Value> {
        let bundle_dir = bundle_dir.as_ref();
        if !bundle_dir.is_dir() {
            bail!("OKF bundle path must be a directory");
        }
        let mut files = Vec::new();
        collect_markdown_files(bundle_dir, &mut files)?;
        files.sort();
        let document_count = files.len();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        if files.is_empty() {
            errors.push("OKF bundle contains no markdown documents".to_string());
        }
        for path in files {
            let relative = relative_path(bundle_dir, &path);
            let text = fs::read_to_string(&path)
                .with_context(|| format!("read OKF document {}", path.display()))?;
            if relative != "index.md" && relative != "log.md" {
                match okf_frontmatter_type(&text) {
                    Some(value) if !value.trim().is_empty() => {}
                    Some(_) => {
                        errors.push(format!("{relative}: frontmatter type must not be empty"))
                    }
                    None => errors.push(format!("{relative}: missing frontmatter")),
                }
            }
            for link in markdown_links(&text) {
                if link_is_external_or_anchor(&link) {
                    continue;
                }
                let target = link.split('#').next().unwrap_or("");
                if target.is_empty() || !target.ends_with(".md") {
                    continue;
                }
                if !path.parent().unwrap_or(bundle_dir).join(target).exists() {
                    warnings.push(format!("{relative}: broken link {link}"));
                }
            }
        }
        Ok(json!({
            "ok": errors.is_empty(),
            "documents": document_count,
            "errors": errors,
            "warnings": warnings,
        }))
    }
}
