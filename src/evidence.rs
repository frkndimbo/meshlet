use super::*;

impl Meshlet {
    pub fn list_evidence(&self, limit: u32) -> Result<Vec<Value>> {
        self.list_evidence_scoped(limit, SafetyProfile::LocalTrusted)
    }

    pub fn list_evidence_scoped(&self, limit: u32, profile: SafetyProfile) -> Result<Vec<Value>> {
        match profile {
            SafetyProfile::LocalTrusted => Ok(self
                .graph_nodes_bounded(Some("evidence"), limit, profile)?
                .items),
            SafetyProfile::PublicSafe => self.list_public_safe_evidence(limit),
        }
    }

    pub fn show_evidence(&self, id: &str) -> Result<Value> {
        let node_id = evidence_node_id(id);
        self.conn
            .query_row(
                "SELECT id, kind, label, attrs_json, source_event_id, visibility
                 FROM graph_nodes WHERE id = ?1 AND kind = 'evidence'",
                [node_id.as_str()],
                node_from_row,
            )
            .optional()?
            .ok_or_else(|| anyhow!("evidence not found: {id}"))
    }

    pub fn show_evidence_scoped(&self, id: &str, profile: SafetyProfile) -> Result<Value> {
        match profile {
            SafetyProfile::LocalTrusted => self.show_evidence(id),
            SafetyProfile::PublicSafe => {
                let node_id = evidence_node_id(id);
                self.conn
                    .query_row(
                        "SELECT id, kind, label, attrs_json, source_event_id, visibility
                         FROM graph_nodes
                         WHERE id = ?1 AND kind = 'evidence' AND visibility = 'public'",
                        [node_id.as_str()],
                        |row| {
                            let node = node_from_row(row)?;
                            Ok(public_safe_evidence_node(node))
                        },
                    )
                    .optional()?
                    .ok_or_else(|| anyhow!("evidence not found: {id}"))
            }
        }
    }

    fn list_public_safe_evidence(&self, limit: u32) -> Result<Vec<Value>> {
        let limit = clamp_limit(limit);
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, label, attrs_json, source_event_id, visibility
             FROM graph_nodes
             WHERE kind = 'evidence' AND visibility = 'public'
             ORDER BY id
             LIMIT ?1",
        )?;
        let mut evidence = stmt
            .query_map([i64::from(limit + 1)], |row| {
                let node = node_from_row(row)?;
                Ok(public_safe_evidence_node(node))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        evidence.truncate(limit as usize);
        Ok(evidence)
    }

    pub fn attach_evidence_file(
        &self,
        path: impl AsRef<Path>,
        task_id: Option<&str>,
        sha256: Option<&str>,
    ) -> Result<Value> {
        let path = path.as_ref();
        let bytes = fs::read(path).with_context(|| format!("read evidence {}", path.display()))?;
        let digest = match sha256 {
            Some("auto") | None => sha256_hex(&bytes),
            Some(value) if value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit()) => {
                value.to_ascii_lowercase()
            }
            Some(_) => bail!("sha256 must be `auto` or a 64-character hex digest"),
        };
        let mut payload = json!({
            "path": path.display().to_string(),
            "sha256": digest,
        });
        if let Some(task_id) = task_id {
            payload["task_id"] = json!(task_id);
        }
        let event = self.append_event("evidence.attached", "cli", payload)?;
        Ok(json!({
            "event_id": event.id,
            "evidence_id": format!("evidence:{}", event.id),
            "path": path.display().to_string(),
            "sha256": digest,
        }))
    }

    pub fn verify_evidence(&self, id: &str) -> Result<Value> {
        let evidence = self.show_evidence(id)?;
        let attrs = evidence
            .get("attrs")
            .and_then(Value::as_object)
            .ok_or_else(|| anyhow!("evidence attrs missing"))?;
        let path = attrs
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("evidence path missing"))?;
        let expected = attrs
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("evidence sha256 missing"))?;
        let bytes = fs::read(path).with_context(|| format!("read evidence {path}"))?;
        let actual = sha256_hex(&bytes);
        Ok(json!({
            "evidence_id": evidence["id"],
            "path": path,
            "ok": actual == expected,
            "expected_sha256": expected,
            "actual_sha256": actual,
        }))
    }

    pub fn retrieve_evidence_by_hash(&self, sha256: &str) -> Result<Value> {
        let expected = normalize_sha256_arg(sha256)?;
        let mut stmt = self.conn.prepare(
            "SELECT id, attrs_json, source_event_id, visibility
             FROM graph_nodes
             WHERE kind = 'evidence'
             ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let (id, attrs_json, source_event_id, visibility) = row?;
            let attrs: Value = serde_json::from_str(&attrs_json)
                .with_context(|| format!("parse evidence attrs for {id}"))?;
            let Some(stored) = attrs.get("sha256").and_then(Value::as_str) else {
                continue;
            };
            if normalize_sha256_arg(stored)? != expected {
                continue;
            }
            items.push(evidence_retrieve_item(
                &id,
                &expected,
                &source_event_id,
                &visibility,
                &attrs,
            ));
        }
        Ok(json!({
            "sha256": expected,
            "found": !items.is_empty(),
            "items": items,
        }))
    }

    pub(crate) fn apply_evidence(&self, event: &Event) -> Result<()> {
        let path = event
            .payload
            .get("path")
            .and_then(Value::as_str)
            .or_else(|| event.payload.get("ref").and_then(Value::as_str))
            .ok_or_else(|| anyhow!("evidence.attached requires path or ref"))?;
        let evidence_id = format!("evidence:{}", event.id);
        let file_id = format!("file:{path}");
        self.upsert_node(
            &evidence_id,
            "evidence",
            event.payload.get("note").and_then(Value::as_str),
            attrs_with_visibility(event.payload.clone(), event.visibility),
            &event.id,
            event.visibility,
        )?;
        self.upsert_node(
            &file_id,
            "file",
            Some(path),
            json!({ "path": path }),
            &event.id,
            event.visibility,
        )?;
        self.upsert_edge(
            &format!("edge:{}:evidence-file", event.id),
            &evidence_id,
            &file_id,
            "references",
            json!({}),
            &event.id,
            event.visibility,
        )?;
        if let Some(task_id) = event.payload.get("task_id").and_then(Value::as_str) {
            self.upsert_edge(
                &format!("edge:{}:evidence-task", event.id),
                &evidence_id,
                &format!("task:{task_id}"),
                "supports",
                json!({}),
                &event.id,
                event.visibility,
            )?;
        }
        Ok(())
    }
}

fn evidence_node_id(id: &str) -> String {
    if id.starts_with("evidence:") {
        id.to_string()
    } else {
        format!("evidence:{id}")
    }
}

fn normalize_sha256_arg(value: &str) -> Result<String> {
    if value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        bail!("sha256 must be a 64-character hex digest")
    }
}

fn evidence_retrieve_item(
    id: &str,
    expected: &str,
    source_event_id: &str,
    visibility: &str,
    attrs: &Value,
) -> Value {
    let has_path = attrs.get("path").is_some();
    let has_ref = attrs.get("ref").is_some();
    let (available, verified, reason) = evidence_availability(attrs, expected);
    let mut item = serde_json::Map::new();
    item.insert("id".to_string(), json!(id));
    item.insert("kind".to_string(), json!("evidence"));
    item.insert("source_event_id".to_string(), json!(source_event_id));
    item.insert("visibility".to_string(), json!(visibility));
    item.insert("sha256".to_string(), json!(expected));
    item.insert("has_path".to_string(), json!(has_path));
    item.insert("has_ref".to_string(), json!(has_ref));
    item.insert("available".to_string(), json!(available));
    item.insert("verified".to_string(), json!(verified));
    if let Some(task_id) = attrs.get("task_id").and_then(Value::as_str) {
        item.insert("task_id".to_string(), json!(task_id));
    }
    if let Some(reason) = reason {
        item.insert("reason".to_string(), json!(reason));
    }
    Value::Object(item)
}

fn evidence_availability(attrs: &Value, expected: &str) -> (bool, bool, Option<&'static str>) {
    let Some(path) = attrs.get("path").and_then(Value::as_str) else {
        return (false, false, Some("no local path"));
    };
    let Ok(bytes) = fs::read(path) else {
        return (false, false, Some("path unavailable"));
    };
    let actual = sha256_hex(&bytes);
    if actual == expected {
        (true, true, None)
    } else {
        (false, false, Some("digest mismatch"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn retrieve_evidence_by_hash_reports_available_metadata_without_path() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let path = dir.path().join("evidence.txt");
        fs::write(&path, "known evidence")?;
        let digest = sha256_hex(&fs::read(&path)?);

        meshlet.attach_evidence_file(&path, Some("task-1"), Some("auto"))?;

        let result = meshlet.retrieve_evidence_by_hash(&digest)?;
        let output = result.to_string();

        assert_eq!(result["found"], true);
        assert_eq!(result["items"][0]["sha256"], digest);
        assert_eq!(result["items"][0]["available"], true);
        assert_eq!(result["items"][0]["verified"], true);
        assert_eq!(result["items"][0]["has_path"], true);
        assert_eq!(result["items"][0]["task_id"], "task-1");
        assert!(!output.contains(path.to_str().unwrap()));
        Ok(())
    }

    #[test]
    fn retrieve_evidence_by_hash_reports_unknown_digest_as_empty() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;

        let result = meshlet.retrieve_evidence_by_hash(&"0".repeat(64))?;

        assert_eq!(result["found"], false);
        assert_eq!(result["items"].as_array().expect("items").len(), 0);
        Ok(())
    }

    #[test]
    fn retrieve_evidence_by_hash_reports_digest_mismatch_unavailable() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let path = dir.path().join("evidence.txt");
        fs::write(&path, "actual evidence")?;
        let stored = "1".repeat(64);

        meshlet.attach_evidence_file(&path, None, Some(&stored))?;

        let result = meshlet.retrieve_evidence_by_hash(&stored)?;

        assert_eq!(result["found"], true);
        assert_eq!(result["items"][0]["available"], false);
        assert_eq!(result["items"][0]["verified"], false);
        assert_eq!(result["items"][0]["reason"], "digest mismatch");
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
}
