use super::*;

fn require_public_doctor_ok(doctor: &Value) -> Result<()> {
    let ok = doctor.get("ok").and_then(Value::as_bool) == Some(true);
    if !ok {
        bail!("public export blocked: public_doctor ok=false");
    }
    Ok(())
}

struct OkfRenderStats {
    contexts: usize,
    tasks: usize,
    messages: usize,
    skills: usize,
    evidence: usize,
    graph: usize,
    events: usize,
    public_doctor: Option<Value>,
}

struct PublicSafeExportViews {
    public_report: Value,
    events: Vec<Value>,
    tasks: Vec<Value>,
    messages: Bounded<Value>,
    timelines: Vec<Value>,
    nodes: Vec<Value>,
    edges: Vec<Value>,
}

impl OkfRenderStats {
    fn document_total(&self) -> usize {
        self.contexts + self.tasks + self.messages + self.skills + self.evidence
    }

    fn document_counts(&self) -> Value {
        json!({
            "index": 1,
            "log": 1,
            "contexts": self.contexts,
            "tasks": self.tasks,
            "messages": self.messages,
            "skills": self.skills,
            "evidence": self.evidence,
            "graph": self.graph,
        })
    }
}

impl Meshlet {
    pub fn public_export(&self, limit: u32) -> Result<Value> {
        let limit = clamp_limit(limit);
        let views = self.public_safe_export_views(limit)?;
        Ok(json!({
            "format": "meshlet-public-export-v1",
            "profile": "public-safe",
            "limit": limit,
            "events": views.events,
            "tasks": views.tasks,
            "mailbox": {
                "messages": {
                    "items": views.messages.items,
                    "truncated": views.messages.truncated,
                }
            },
            "timelines": views.timelines,
            "graph": {
                "nodes": views.nodes,
                "edges": views.edges,
            },
            "redaction_report": views.public_report["redaction_report"].clone(),
        }))
    }

    fn public_safe_export_views(&self, limit: u32) -> Result<PublicSafeExportViews> {
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
        let messages = Bounded {
            items: messages.items.into_iter().map(compact_message).collect(),
            truncated: messages.truncated,
        };
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
        Ok(PublicSafeExportViews {
            public_report,
            events,
            tasks,
            messages,
            timelines,
            nodes,
            edges,
        })
    }

    pub fn public_export_okf(&self, out_dir: impl AsRef<Path>, limit: u32) -> Result<Value> {
        let out_dir = out_dir.as_ref();
        let limit = clamp_limit(limit);
        let public_report = self.public_doctor()?;
        require_public_doctor_ok(&public_report)?;
        prepare_okf_output_dir(out_dir)?;
        let stats = self.render_okf_bundle(out_dir, limit, SafetyProfile::PublicSafe)?;

        Ok(json!({
            "format": "meshlet-okf-public-export-v1",
            "profile": "public-safe",
            "path": out_dir.display().to_string(),
            "limit": limit,
            "documents": stats.document_total(),
            "events": stats.events,
            "messages": stats.messages,
            "public_doctor": stats.public_doctor.unwrap_or_else(|| json!({})),
        }))
    }

    pub fn okf_sync(&self, out_dir: impl AsRef<Path>, profile: SafetyProfile) -> Result<Value> {
        let out_dir = out_dir.as_ref();
        let limit = DEFAULT_LIMIT;
        let temp_dir = okf_sync_temp_dir(out_dir)?;
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir)?;
        }
        fs::create_dir_all(&temp_dir)?;

        let sync_result = (|| {
            let stats = self.render_okf_bundle(&temp_dir, limit, profile)?;
            let doctor = replace_okf_output_after_doctor(out_dir, &temp_dir)?;
            Ok(json!({
                "status": "synced",
                "format": "okf",
                "profile": profile.as_str(),
                "path": out_dir.display().to_string(),
                "documents": stats.document_counts(),
                "doctor": {
                    "ok": doctor.get("ok").and_then(Value::as_bool).unwrap_or(false),
                    "errors": doctor
                        .get("errors")
                        .and_then(Value::as_array)
                        .map(Vec::len)
                        .unwrap_or(0),
                    "warnings": doctor
                        .get("warnings")
                        .and_then(Value::as_array)
                        .map(Vec::len)
                        .unwrap_or(0),
                },
            }))
        })();

        if sync_result.is_err() && temp_dir.exists() {
            let _ = fs::remove_dir_all(&temp_dir);
        }
        sync_result
    }

    fn render_okf_bundle(
        &self,
        out_dir: &Path,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<OkfRenderStats> {
        let public_report = if profile == SafetyProfile::PublicSafe {
            let report = self.public_doctor()?;
            require_public_doctor_ok(&report)?;
            Some(report)
        } else {
            None
        };
        let events = self.list_events_bounded_scoped(limit, profile)?.items;
        let contexts = self.list_contexts_limited(limit, profile)?;
        let tasks = self.list_tasks_scoped(limit, profile)?;
        let skills = self.list_skills_scoped(profile)?;
        let evidence = self.list_evidence_scoped(limit, profile)?;
        let messages = self.okf_messages_bounded(limit, profile)?;
        let nodes = self.graph_nodes_bounded(None, limit, profile)?.items;
        let edges = self.graph_edges_bounded(None, limit, profile)?.items;
        let graph_count = nodes.len() + edges.len();
        let context_count = contexts.len();
        let task_count = tasks.len();
        let message_count = messages.items.len();
        let skill_count = skills.len();
        let evidence_count = evidence.len();
        let event_count = events.len();
        let mut documents = Vec::new();

        for context in contexts {
            let id = string_value(&context, "id").unwrap_or("context:unknown");
            let title = string_value(&context, "title")
                .or_else(|| string_value(&context, "summary"))
                .unwrap_or(id);
            let description = string_value(&context, "summary").unwrap_or(title);
            let mut body = String::new();
            body.push_str(&okf_safe_text(description));
            body.push_str("\n\n## Context\n");
            body.push_str(&okf_metadata_line("Kind", string_value(&context, "kind")));
            body.push_str(&okf_metadata_line(
                "Namespace",
                string_value(&context, "namespace"),
            ));
            documents.push(OkfDocument {
                id: id.to_string(),
                item_type: "Meshlet Context".to_string(),
                title: okf_safe_text(title),
                description: okf_safe_text(description),
                resource: format!("meshlet://contexts/{id}"),
                tags: vec!["meshlet".to_string(), "context".to_string()],
                timestamp: string_value(&context, "updated_at")
                    .or_else(|| string_value(&context, "created_at"))
                    .unwrap_or("")
                    .to_string(),
                source_event_id: string_value(&context, "source_event_id")
                    .unwrap_or("")
                    .to_string(),
                visibility: string_value(&context, "visibility")
                    .unwrap_or("public")
                    .to_string(),
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
            body.push_str(&okf_safe_text(description));
            body.push_str("\n\n## Task\n");
            body.push_str(&okf_metadata_line("Status", string_value(&task, "status")));
            body.push_str(&okf_metadata_line(
                "Assignee",
                string_value(&task, "assignee"),
            ));
            let timeline = self.task_timeline(id, limit, profile)?;
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
                title: okf_safe_text(title),
                description: okf_safe_text(description),
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
                visibility: string_value(&task, "visibility")
                    .unwrap_or("public")
                    .to_string(),
                relative_path: format!("tasks/{}.md", okf_slug(id)),
                body,
            });
        }

        for message in &messages.items {
            let id = string_value(message, "id").unwrap_or("message:unknown");
            let summary = string_value(message, "summary").unwrap_or(id);
            let mut body = String::new();
            body.push_str(&okf_safe_text(summary));
            body.push_str("\n\n## Message\n");
            body.push_str(&okf_metadata_line("From", string_value(message, "from")));
            body.push_str(&okf_metadata_line("To", string_value(message, "to")));
            body.push_str(&okf_metadata_line("Task", string_value(message, "task_id")));
            body.push_str(&okf_metadata_line(
                "Reply To",
                string_value(message, "reply_to"),
            ));
            documents.push(OkfDocument {
                id: id.to_string(),
                item_type: "Meshlet Message".to_string(),
                title: okf_safe_text(summary),
                description: okf_safe_text(summary),
                resource: format!("meshlet://messages/{id}"),
                tags: vec!["meshlet".to_string(), "message".to_string()],
                timestamp: string_value(message, "created_at")
                    .unwrap_or("")
                    .to_string(),
                source_event_id: string_value(message, "source_event_id")
                    .unwrap_or("")
                    .to_string(),
                visibility: string_value(message, "visibility")
                    .unwrap_or("public")
                    .to_string(),
                relative_path: format!("messages/{}.md", okf_slug(id)),
                body,
            });
        }

        for skill in skills {
            let name = string_value(&skill, "name").unwrap_or("skill");
            let description = string_value(&skill, "description").unwrap_or(name);
            let mut body = String::new();
            body.push_str(&okf_safe_text(description));
            body.push_str("\n\n## Skill\n");
            body.push_str(&okf_metadata_line(
                "Version",
                string_value(&skill, "version"),
            ));
            body.push_str(&okf_metadata_line("Entry", string_value(&skill, "entry")));
            body.push_str(&okf_metadata_line(
                "Permissions",
                skill
                    .get("permissions")
                    .map(|value| value.to_string())
                    .as_deref(),
            ));
            documents.push(OkfDocument {
                id: format!("skill:{name}"),
                item_type: "Meshlet Skill".to_string(),
                title: okf_safe_text(name),
                description: okf_safe_text(description),
                resource: format!("meshlet://skills/{name}"),
                tags: vec!["meshlet".to_string(), "skill".to_string()],
                timestamp: String::new(),
                source_event_id: string_value(&skill, "source_event_id")
                    .unwrap_or("")
                    .to_string(),
                visibility: string_value(&skill, "visibility")
                    .unwrap_or("public")
                    .to_string(),
                relative_path: format!("skills/{}.md", okf_slug(name)),
                body,
            });
        }

        for item in evidence {
            let id = string_value(&item, "id").unwrap_or("evidence:unknown");
            let attrs = item.get("attrs").unwrap_or(&Value::Null);
            let sha256 = string_value(&item, "sha256")
                .or_else(|| string_value(attrs, "sha256"))
                .unwrap_or("");
            let evidence_description = if profile == SafetyProfile::PublicSafe {
                "Public-safe evidence reference"
            } else {
                "Meshlet evidence reference"
            };
            let mut body = String::new();
            body.push_str(evidence_description);
            body.push('.');
            body.push_str("\n\n# Citations\n");
            body.push_str(&format!("- `meshlet://evidence/{id}`"));
            if !sha256.is_empty() {
                body.push_str(&format!(" sha256 `{sha256}`"));
            }
            body.push('\n');
            if item.get("has_path").and_then(Value::as_bool) == Some(true)
                || attrs.get("path").is_some()
            {
                body.push_str("- Local path: redacted\n");
            }
            if item.get("has_ref").and_then(Value::as_bool) == Some(true)
                || attrs.get("ref").is_some()
            {
                body.push_str("- Reference: redacted\n");
            }
            body.push_str(&okf_metadata_line("Task", string_value(attrs, "task_id")));
            documents.push(OkfDocument {
                id: id.to_string(),
                item_type: "Meshlet Evidence".to_string(),
                title: id.to_string(),
                description: evidence_description.to_string(),
                resource: format!("meshlet://evidence/{id}"),
                tags: vec!["meshlet".to_string(), "evidence".to_string()],
                timestamp: String::new(),
                source_event_id: string_value(&item, "source_event_id")
                    .unwrap_or("")
                    .to_string(),
                visibility: string_value(&item, "visibility")
                    .unwrap_or("public")
                    .to_string(),
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

        Ok(OkfRenderStats {
            contexts: context_count,
            tasks: task_count,
            messages: message_count,
            skills: skill_count,
            evidence: evidence_count,
            graph: graph_count,
            events: event_count,
            public_doctor: public_report,
        })
    }

    fn okf_messages_bounded(&self, limit: u32, profile: SafetyProfile) -> Result<Bounded<Value>> {
        if profile == SafetyProfile::PublicSafe {
            return self.public_messages_bounded(limit);
        }
        let limit = clamp_limit(limit);
        let visibility = visibility_clause(profile);
        let sql = format!(
            "SELECT id, from_agent, to_agent, task_id, summary, body, reply_to, visibility, source_event_id, created_at, schema_version, attrs_json
             FROM mailbox_messages
             WHERE {visibility}
             ORDER BY created_at DESC, id
             LIMIT ?1",
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut messages = stmt
            .query_map([limit + 1], message_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = messages.len() > limit as usize;
        messages.truncate(limit as usize);
        Ok(Bounded {
            items: messages,
            truncated,
        })
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

fn okf_sync_temp_dir(out_dir: &Path) -> Result<PathBuf> {
    let parent = okf_parent_dir(out_dir);
    fs::create_dir_all(&parent)?;
    let name = out_dir
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("okf");
    Ok(parent.join(format!(".{name}.meshlet-sync-tmp-{}", Uuid::new_v4())))
}

pub(crate) fn replace_okf_output_after_doctor(out_dir: &Path, temp_dir: &Path) -> Result<Value> {
    if !temp_dir.is_dir() {
        bail!("OKF temp path must be a directory");
    }
    if out_dir.exists() && !out_dir.is_dir() {
        bail!("OKF output path must be a directory");
    }

    let doctor = Meshlet::okf_doctor(temp_dir)?;
    if doctor.get("ok").and_then(Value::as_bool) != Some(true) {
        bail!("OKF doctor failed; target output was not replaced");
    }

    let backup_dir = okf_sync_backup_dir(out_dir)?;
    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir)?;
    }
    if out_dir.exists() {
        fs::rename(out_dir, &backup_dir)
            .with_context(|| format!("move old OKF output {}", out_dir.display()))?;
    }

    if let Err(error) = fs::rename(temp_dir, out_dir) {
        if backup_dir.exists() {
            let _ = fs::rename(&backup_dir, out_dir);
        }
        bail!("replace OKF output {}: {error}", out_dir.display());
    }

    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir)
            .with_context(|| format!("remove old OKF output {}", backup_dir.display()))?;
    }
    Ok(doctor)
}

fn okf_sync_backup_dir(out_dir: &Path) -> Result<PathBuf> {
    let parent = okf_parent_dir(out_dir);
    fs::create_dir_all(&parent)?;
    let name = out_dir
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("okf");
    Ok(parent.join(format!(".{name}.meshlet-sync-old-{}", Uuid::new_v4())))
}

fn okf_parent_dir(path: &Path) -> PathBuf {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn okf_safe_text(value: &str) -> String {
    let report = scan_payload_safety(&json!(value));
    if report.blocked_keys.is_empty() && report.suspicious_values.is_empty() {
        value.to_string()
    } else {
        "[redacted]".to_string()
    }
}

fn okf_metadata_line(label: &str, value: Option<&str>) -> String {
    value
        .map(okf_safe_text)
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("- {label}: `{value}`\n"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
    fn public_okf_export_filters_events_before_limit() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:public",
            json!({"title": "Older Public", "summary": "public note"}),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:private",
            json!({"title": "Newer Private", "summary": "private note"}),
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        let out = dir.path().join("okf");

        let export = meshlet.public_export_okf(&out, 1)?;
        let log = fs::read_to_string(out.join("log.md"))?;

        assert_eq!(export["events"], 1);
        assert!(log.contains("agent:public"));
        assert!(!log.contains("agent:private"));
        Ok(())
    }

    #[test]
    fn okf_sync_creates_index_and_log() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let out = dir.path().join(".meshlet-okf");

        let report = meshlet.okf_sync(&out, SafetyProfile::LocalTrusted)?;

        assert_eq!(report["status"], "synced");
        assert_eq!(report["profile"], "local-trusted");
        assert_eq!(report["doctor"]["ok"], true);
        assert!(out.join("index.md").exists());
        assert!(out.join("log.md").exists());
        Ok(())
    }

    #[test]
    fn okf_sync_replaces_existing_output() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let out = dir.path().join(".meshlet-okf");

        meshlet.okf_sync(&out, SafetyProfile::LocalTrusted)?;
        fs::write(out.join("stale.md"), "stale")?;
        meshlet.okf_sync(&out, SafetyProfile::LocalTrusted)?;

        assert!(!out.join("stale.md").exists());
        assert!(out.join("index.md").exists());
        Ok(())
    }

    #[test]
    fn okf_sync_failed_doctor_preserves_previous_output() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let out = dir.path().join(".meshlet-okf");
        meshlet.okf_sync(&out, SafetyProfile::LocalTrusted)?;
        let before = fs::read_to_string(out.join("index.md"))?;

        let bad_temp = dir.path().join(".bad-okf-sync");
        fs::create_dir_all(&bad_temp)?;
        fs::write(bad_temp.join("broken.md"), "# Missing frontmatter\n")?;

        assert!(replace_okf_output_after_doctor(&out, &bad_temp).is_err());
        assert_eq!(fs::read_to_string(out.join("index.md"))?, before);
        assert!(out.join("log.md").exists());
        Ok(())
    }

    #[test]
    fn okf_sync_public_safe_excludes_local_private_data() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"title": "Private Sync Context", "summary": "private sync summary"}),
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"title": "Local Sync Context", "summary": "local sync summary"}),
            EventVisibility::Local,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"title": "Public Sync Context", "summary": "public sync summary"}),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        meshlet.create_task(
            Some("private-sync-task"),
            "Private Sync Task",
            None,
            None,
            Some("private sync task note"),
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.create_task(
            Some("public-sync-task"),
            "Public Sync Task",
            None,
            None,
            Some("public sync task note"),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        meshlet.send_agent_message(
            "agent:a",
            "agent:b",
            "Private Sync Message",
            None,
            Some("private sync body"),
            None,
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.send_agent_message(
            "agent:a",
            "agent:b",
            "Public Sync Message",
            None,
            Some("public sync body"),
            None,
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;
        let out = dir.path().join("okf-public");

        let report = meshlet.okf_sync(&out, SafetyProfile::PublicSafe)?;
        let bundle = read_dir_text(&out)?;

        assert_eq!(report["profile"], "public-safe");
        assert!(bundle.contains("Public Sync Context"));
        assert!(bundle.contains("public sync task note"));
        assert!(bundle.contains("Public Sync Message"));
        assert!(!bundle.contains("Private Sync Context"));
        assert!(!bundle.contains("private sync summary"));
        assert!(!bundle.contains("Local Sync Context"));
        assert!(!bundle.contains("local sync summary"));
        assert!(!bundle.contains("Private Sync Task"));
        assert!(!bundle.contains("private sync task note"));
        assert!(!bundle.contains("Private Sync Message"));
        assert!(!bundle.contains("private sync body"));
        assert!(!bundle.contains("public sync body"));
        Ok(())
    }

    #[test]
    fn okf_sync_local_trusted_includes_local_task_context_summaries() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"title": "Local Trusted Context", "summary": "local trusted summary"}),
            EventVisibility::Local,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.create_task(
            Some("local-trusted-task"),
            "Local Trusted Task",
            None,
            Some("agent:codex"),
            Some("local trusted task note"),
            EventVisibility::Local,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.send_agent_message(
            "agent:a",
            "agent:b",
            "Local Trusted Message",
            Some("local-trusted-task"),
            Some("local trusted raw body"),
            None,
            EventVisibility::Local,
            SafetyProfile::LocalTrusted,
        )?;
        let out = dir.path().join(".meshlet-okf");

        let report = meshlet.okf_sync(&out, SafetyProfile::LocalTrusted)?;
        let bundle = read_dir_text(&out)?;

        assert_eq!(report["profile"], "local-trusted");
        assert!(bundle.contains("Local Trusted Context"));
        assert!(bundle.contains("local trusted summary"));
        assert!(bundle.contains("Local Trusted Task"));
        assert!(bundle.contains("local trusted task note"));
        assert!(bundle.contains("Local Trusted Message"));
        assert!(!bundle.contains("local trusted raw body"));
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
}
