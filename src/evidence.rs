use super::*;

impl Meshlet {
    pub fn list_evidence(&self, limit: u32) -> Result<Vec<Value>> {
        self.list_evidence_scoped(limit, SafetyProfile::LocalTrusted)
    }

    pub fn list_evidence_scoped(&self, limit: u32, profile: SafetyProfile) -> Result<Vec<Value>> {
        Ok(self
            .graph_nodes_bounded(Some("evidence"), limit, profile)?
            .items)
    }

    pub fn show_evidence(&self, id: &str) -> Result<Value> {
        let node_id = if id.starts_with("evidence:") {
            id.to_string()
        } else {
            format!("evidence:{id}")
        };
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
