use super::*;

impl Meshlet {
    pub fn add_skill_manifest(&self, manifest_path: impl AsRef<Path>) -> Result<Event> {
        let manifest_path = manifest_path.as_ref();
        let text = fs::read_to_string(manifest_path)
            .with_context(|| format!("read skill manifest {}", manifest_path.display()))?;
        let manifest: SkillManifest = toml::from_str(&text).context("parse skill manifest TOML")?;
        validate_skill_manifest(&manifest)?;
        let payload = json!({
            "name": manifest.name,
            "version": manifest.version,
            "kind": manifest.kind,
            "manifest_path": manifest_path.display().to_string(),
            "entry": manifest.entry,
            "permissions": manifest.permissions,
            "description": manifest.description,
        });
        self.append_event("skill.added", "cli", payload)
    }

    pub fn list_skills(&self) -> Result<Vec<Value>> {
        self.list_skills_scoped(SafetyProfile::LocalTrusted)
    }

    pub fn list_skills_scoped(&self, profile: SafetyProfile) -> Result<Vec<Value>> {
        Ok(self.skills_bounded(None, profile)?.items)
    }

    pub fn list_skills_limited(&self, limit: u32, profile: SafetyProfile) -> Result<Vec<Value>> {
        Ok(self.skills_bounded(Some(limit), profile)?.items)
    }

    pub fn search_skills(&self, q: &str, limit: u32, profile: SafetyProfile) -> Result<Value> {
        if q.trim().is_empty() {
            bail!("query must not be empty");
        }
        let query = fts_query(q)?;
        let bounded = self.query_skills(&query, limit, profile)?;
        Ok(json!({
            "q": q,
            "limit": clamp_limit(limit),
            "skills": {
                "items": bounded.items,
                "truncated": bounded.truncated,
            },
        }))
    }

    pub(crate) fn skills_bounded(
        &self,
        limit: Option<u32>,
        profile: SafetyProfile,
    ) -> Result<Bounded<Value>> {
        let visibility = visibility_clause(profile);
        let limit = limit.map(clamp_limit);
        let sql = if limit.is_some() {
            format!(
                "SELECT name, version, manifest_path, entry, permissions_json, description, source_event_id, visibility
                 FROM skills
                 WHERE {visibility}
                 ORDER BY name
                 LIMIT ?1"
            )
        } else {
            format!(
                "SELECT name, version, manifest_path, entry, permissions_json, description, source_event_id, visibility
                 FROM skills
                 WHERE {visibility}
                 ORDER BY name"
            )
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut skills = if let Some(limit) = limit {
            stmt.query_map([limit + 1], skill_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map([], skill_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let truncated = limit.is_some_and(|limit| skills.len() > limit as usize);
        if let Some(limit) = limit {
            skills.truncate(limit as usize);
        }
        Ok(Bounded {
            items: skills,
            truncated,
        })
    }

    pub(crate) fn query_skills(
        &self,
        query: &str,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Bounded<Value>> {
        let limit = clamp_limit(limit);
        let visibility = visibility_clause(profile);
        let sql = format!(
            "SELECT skills.name, skills.version, skills.manifest_path, skills.entry, skills.permissions_json, skills.description, skills.source_event_id, skills.visibility
             FROM skills
             JOIN skills_fts ON skills.name = skills_fts.name
             WHERE skills_fts MATCH ?1
               AND skills.{visibility}
             ORDER BY bm25(skills_fts), skills.name
             LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut skills = stmt
            .query_map(params![query, limit + 1], skill_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = skills.len() > limit as usize;
        skills.truncate(limit as usize);
        Ok(Bounded {
            items: skills,
            truncated,
        })
    }

    pub fn show_skill(&self, name: &str) -> Result<Value> {
        self.conn
            .query_row(
                "SELECT name, version, manifest_path, entry, permissions_json, description, source_event_id, visibility
                 FROM skills WHERE name = ?1",
                [name],
                skill_from_row,
            )
            .optional()?
            .ok_or_else(|| anyhow!("skill not found: {name}"))
    }

    pub(crate) fn apply_skill_added(&self, event: &Event) -> Result<()> {
        let name = str_field(&event.payload, "name")?;
        let version = str_field(&event.payload, "version")?;
        let manifest_path = str_field(&event.payload, "manifest_path")?;
        let entry = str_field(&event.payload, "entry")?;
        let permissions = event
            .payload
            .get("permissions")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let description = event.payload.get("description").and_then(Value::as_str);
        self.conn.execute(
            "INSERT OR REPLACE INTO skills(name, version, manifest_path, entry, permissions_json, description, source_event_id, visibility)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                name,
                version,
                manifest_path,
                entry,
                canonical_json(&permissions)?,
                description,
                event.id,
                event.visibility.as_str(),
            ],
        )?;
        self.upsert_skill_fts(name, description)?;
        let skill_id = format!("skill:{name}");
        self.upsert_node(
            &skill_id,
            "skill",
            Some(name),
            json!({
                "name": name,
                "version": version,
                "permissions": permissions,
                "description": description,
            }),
            &event.id,
            event.visibility,
        )?;
        let file_id = format!("file:{entry}");
        self.upsert_node(
            &file_id,
            "file",
            Some(entry),
            json!({ "path": entry }),
            &event.id,
            event.visibility,
        )?;
        self.upsert_edge(
            &format!("edge:{}:skill-entry", event.id),
            &skill_id,
            &file_id,
            "references",
            json!({ "field": "entry" }),
            &event.id,
            event.visibility,
        )?;
        Ok(())
    }

    pub(crate) fn upsert_skill_fts(&self, name: &str, summary: Option<&str>) -> Result<()> {
        self.conn
            .execute("DELETE FROM skills_fts WHERE name = ?1", [name])?;
        self.conn.execute(
            "INSERT INTO skills_fts(name, searchable_name, kind, summary)
             VALUES(?1, ?2, 'skill', ?3)",
            params![name, name, summary],
        )?;
        Ok(())
    }
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

    #[test]
    fn skills_visibility_column_materializes_from_event_visibility() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event_with_options(
            "skill.added",
            "agent:test",
            skill_payload("public-skill"),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        )?;

        let skill = meshlet.show_skill("public-skill")?;

        assert_eq!(skill["visibility"], "public");
        assert_eq!(skill["source_event_id"].as_str().expect("source").len(), 36);
        Ok(())
    }

    #[test]
    fn skill_manifest_roundtrip_materializes_skill_and_graph() -> Result<()> {
        let dir = tempdir()?;
        let manifest_path = dir.path().join("skill.toml");
        fs::write(
            &manifest_path,
            r#"
name = "rust-review"
version = "0.1.0"
kind = "skill"
entry = "./SKILL.md"
permissions = ["read_repo", "run_check"]
description = "Review Rust code."
"#,
        )?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.add_skill_manifest(&manifest_path)?;
        let skill = meshlet.show_skill("rust-review")?;
        assert_eq!(skill["name"], "rust-review");
        let skill_nodes = meshlet.graph_nodes(Some("skill"))?;
        assert_eq!(skill_nodes.len(), 1);
        Ok(())
    }

    #[test]
    fn skill_manifest_rejects_unknown_permission_and_unsafe_entry() -> Result<()> {
        let dir = tempdir()?;
        let bad_permission = dir.path().join("bad-permission.toml");
        fs::write(
            &bad_permission,
            r#"
name = "bad-permission"
version = "0.1.0"
kind = "skill"
entry = "./SKILL.md"
permissions = ["network"]
"#,
        )?;
        let absolute_entry = dir.path().join("absolute-entry.toml");
        fs::write(
            &absolute_entry,
            r#"
name = "absolute-entry"
version = "0.1.0"
kind = "skill"
entry = "/tmp/SKILL.md"
permissions = ["read_repo"]
"#,
        )?;
        let parent_entry = dir.path().join("parent-entry.toml");
        fs::write(
            &parent_entry,
            r#"
name = "parent-entry"
version = "0.1.0"
kind = "skill"
entry = "../SKILL.md"
permissions = ["read_repo"]
"#,
        )?;
        let meshlet = Meshlet::init(dir.path())?;

        assert!(meshlet.add_skill_manifest(&bad_permission).is_err());
        assert!(meshlet.add_skill_manifest(&absolute_entry).is_err());
        assert!(meshlet.add_skill_manifest(&parent_entry).is_err());
        assert_eq!(meshlet.event_count()?, 1);
        Ok(())
    }
}
