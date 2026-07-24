use super::*;

impl Meshlet {
    pub fn event_count(&self) -> Result<u64> {
        self.event_count_scoped(SafetyProfile::LocalTrusted)
    }

    pub(crate) fn event_count_scoped(&self, profile: SafetyProfile) -> Result<u64> {
        let visibility = visibility_clause(profile);
        let sql = format!("SELECT COUNT(*) FROM events WHERE {visibility}");
        let count: u64 = self.conn.query_row(&sql, [], |row| row.get(0))?;
        Ok(count)
    }

    pub fn verify_event_chain(&self) -> Result<VerificationReport> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, id, type, created_at, actor, payload_json, visibility, hash, prev_hash
             FROM events ORDER BY seq ASC",
        )?;
        let rows = stmt
            .query_map([], event_record_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut expected_prev_hash: Option<String> = None;
        let mut checked_until_seq = 0;

        for record in &rows {
            checked_until_seq = record.seq;
            if record.event.prev_hash != expected_prev_hash {
                return Ok(VerificationReport {
                    ok: false,
                    events: rows.len() as u64,
                    checked_until_seq,
                    first_invalid_seq: Some(record.seq),
                    reason: Some("prev_hash_mismatch".to_string()),
                });
            }
            let expected_hash = event_hash(
                &record.event.id,
                &record.event.event_type,
                &record.event.created_at,
                &record.event.actor,
                &record.event.payload,
                record.event.visibility,
                record.event.prev_hash.as_deref(),
            )?;
            if record.event.hash != expected_hash {
                return Ok(VerificationReport {
                    ok: false,
                    events: rows.len() as u64,
                    checked_until_seq,
                    first_invalid_seq: Some(record.seq),
                    reason: Some("hash_mismatch".to_string()),
                });
            }
            expected_prev_hash = Some(record.event.hash.clone());
        }

        Ok(VerificationReport {
            ok: true,
            events: rows.len() as u64,
            checked_until_seq,
            first_invalid_seq: None,
            reason: None,
        })
    }

    pub fn append_event(&self, event_type: &str, actor: &str, payload: Value) -> Result<Event> {
        self.append_event_with_options(
            event_type,
            actor,
            payload,
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )
    }

    pub fn append_event_with_options(
        &self,
        event_type: &str,
        actor: &str,
        payload: Value,
        visibility: EventVisibility,
        profile: SafetyProfile,
    ) -> Result<Event> {
        validate_event_type(event_type)?;
        if actor.trim().is_empty() {
            bail!("actor must not be empty");
        }
        validate_payload_safety(&payload, profile)?;
        validate_event_payload(event_type, &payload)?;
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .context("begin event append transaction")?;
        let result = (|| -> Result<Event> {
            self.validate_event_semantics(event_type, &payload)?;
            let prev_hash = self.latest_hash()?;
            let id = Uuid::new_v4().to_string();
            let created_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            let hash = event_hash(
                &id,
                event_type,
                &created_at,
                actor,
                &payload,
                visibility,
                prev_hash.as_deref(),
            )?;
            let next_seq = self.next_seq()?;
            self.conn.execute(
                "INSERT INTO events(id, seq, type, created_at, actor, payload_json, visibility, hash, prev_hash)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    id,
                    next_seq,
                    event_type,
                    created_at,
                    actor,
                    canonical_json(&payload)?,
                    visibility.as_str(),
                    hash,
                    prev_hash
                ],
            )?;
            let event = self
                .get_event_by_seq(next_seq)?
                .ok_or_else(|| anyhow!("inserted event not found"))?;
            self.upsert_event_fts(&event)?;
            self.apply_event(&event)?;
            Ok(event)
        })();
        match result {
            Ok(event) => {
                self.conn
                    .execute_batch("COMMIT")
                    .context("commit event append transaction")?;
                Ok(event)
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub fn list_events(&self, limit: u32) -> Result<Vec<Event>> {
        let limit = clamp_limit(limit);
        let mut stmt = self.conn.prepare(
            "SELECT id, type, created_at, actor, payload_json, visibility, hash, prev_hash
             FROM events ORDER BY seq DESC LIMIT ?1",
        )?;
        let events = stmt
            .query_map([limit], event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(events)
    }

    pub(crate) fn list_events_bounded(&self, limit: u32) -> Result<Bounded<Event>> {
        self.list_events_bounded_scoped(limit, SafetyProfile::LocalTrusted)
    }

    pub(crate) fn list_events_bounded_scoped(
        &self,
        limit: u32,
        profile: SafetyProfile,
    ) -> Result<Bounded<Event>> {
        let limit = clamp_limit(limit);
        let visibility = visibility_clause(profile);
        let sql = format!(
            "SELECT id, type, created_at, actor, payload_json, visibility, hash, prev_hash
             FROM events
             WHERE {visibility}
             ORDER BY seq DESC
             LIMIT ?1"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut events = stmt
            .query_map([limit + 1], event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = events.len() > limit as usize;
        events.truncate(limit as usize);
        Ok(Bounded {
            items: events,
            truncated,
        })
    }

    pub fn show_event(&self, id: &str) -> Result<Event> {
        self.conn
            .query_row(
                "SELECT id, type, created_at, actor, payload_json, visibility, hash, prev_hash
                 FROM events WHERE id = ?1",
                [id],
                event_from_row,
            )
            .optional()?
            .ok_or_else(|| anyhow!("event not found: {id}"))
    }

    pub(crate) fn validate_event_semantics(&self, event_type: &str, payload: &Value) -> Result<()> {
        match event_type {
            "task.created" => {
                if let Some(task_id) = nonempty_string(payload, "task_id")
                    && self.task_exists(task_id)?
                {
                    bail!("task already exists: {task_id}");
                }
            }
            "task.updated" => {
                let task_id = required_nonempty_string(payload, "task_id")?;
                let Some(current_status) = self.task_status(task_id)? else {
                    bail!("task not found: {task_id}");
                };
                if let Some(next_status) = nonempty_string(payload, "status") {
                    validate_task_transition(&current_status, next_status)?;
                }
            }
            "agent.message" => {
                if let Some(task_id) = nonempty_string(payload, "task_id")
                    && !self.task_exists(task_id)?
                {
                    bail!("task not found: {task_id}");
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn next_seq(&self) -> Result<i64> {
        let seq: i64 =
            self.conn
                .query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM events", [], |row| {
                    row.get(0)
                })?;
        Ok(seq)
    }

    pub(crate) fn latest_hash(&self) -> Result<Option<String>> {
        let hash = self
            .conn
            .query_row(
                "SELECT hash FROM events ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(hash)
    }

    pub(crate) fn get_event_by_seq(&self, seq: i64) -> Result<Option<Event>> {
        let event = self
            .conn
            .query_row(
                "SELECT id, type, created_at, actor, payload_json, visibility, hash, prev_hash
                 FROM events WHERE seq = ?1",
                [seq],
                event_from_row,
            )
            .optional()?;
        Ok(event)
    }

    pub(crate) fn events_ascending(&self) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, type, created_at, actor, payload_json, visibility, hash, prev_hash
             FROM events ORDER BY seq ASC",
        )?;
        let events = stmt
            .query_map([], event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(events)
    }

    pub(crate) fn event_records_ascending(&self) -> Result<Vec<EventRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, id, type, created_at, actor, payload_json, visibility, hash, prev_hash
             FROM events ORDER BY seq ASC",
        )?;
        let records = stmt
            .query_map([], event_record_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(records)
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestCaseError};
    use tempfile::tempdir;

    fn must<T, E: std::fmt::Display>(
        result: std::result::Result<T, E>,
    ) -> std::result::Result<T, TestCaseError> {
        result.map_err(|error| TestCaseError::fail(error.to_string()))
    }

    fn generated_events() -> impl Strategy<Value = Vec<(u16, u8)>> {
        prop::collection::vec((0u16..4096, 0u8..3), 1..16)
    }

    fn visibility_from_seed(seed: u8) -> EventVisibility {
        match seed % 3 {
            0 => EventVisibility::Private,
            1 => EventVisibility::Local,
            _ => EventVisibility::Public,
        }
    }

    fn append_generated_events(
        meshlet: &Meshlet,
        events: &[(u16, u8)],
    ) -> std::result::Result<(), TestCaseError> {
        for (index, (label_seed, visibility_seed)) in events.iter().enumerate() {
            must(meshlet.append_event_with_options(
                "context.added",
                &format!("agent:{index}"),
                json!({"label": format!("event-{index}-{label_seed}")}),
                visibility_from_seed(*visibility_seed),
                SafetyProfile::LocalTrusted,
            ))?;
        }
        Ok(())
    }

    fn stored_event_count(meshlet: &Meshlet) -> std::result::Result<i64, TestCaseError> {
        must(
            meshlet
                .conn
                .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0)),
        )
    }

    fn mutate_event_field(
        meshlet: &Meshlet,
        seq: i64,
        field: u8,
    ) -> std::result::Result<(), TestCaseError> {
        match field % 5 {
            0 => {
                let payload = json!({"label": "tampered", "seq": seq}).to_string();
                must(meshlet.conn.execute(
                    "UPDATE events SET payload_json = ?1 WHERE seq = ?2",
                    params![payload, seq],
                ))?;
            }
            1 => {
                let visibility: String = must(meshlet.conn.query_row(
                    "SELECT visibility FROM events WHERE seq = ?1",
                    [seq],
                    |row| row.get(0),
                ))?;
                let replacement = if visibility == "public" {
                    "private"
                } else {
                    "public"
                };
                must(meshlet.conn.execute(
                    "UPDATE events SET visibility = ?1 WHERE seq = ?2",
                    params![replacement, seq],
                ))?;
            }
            2 => {
                must(meshlet.conn.execute(
                    "UPDATE events SET actor = ?1 WHERE seq = ?2",
                    params!["agent:tampered", seq],
                ))?;
            }
            3 => {
                must(meshlet.conn.execute(
                    "UPDATE events SET type = ?1 WHERE seq = ?2",
                    params!["tampered.event", seq],
                ))?;
            }
            _ => {
                must(meshlet.conn.execute(
                    "UPDATE events SET prev_hash = ?1 WHERE seq = ?2",
                    params!["tampered-prev-hash", seq],
                ))?;
            }
        }
        Ok(())
    }

    fn swap_adjacent_events(
        meshlet: &Meshlet,
        first_seq: i64,
    ) -> std::result::Result<(), TestCaseError> {
        let second_seq = first_seq + 1;
        must(
            meshlet
                .conn
                .execute("UPDATE events SET seq = -1 WHERE seq = ?1", [first_seq]),
        )?;
        must(meshlet.conn.execute(
            "UPDATE events SET seq = ?1 WHERE seq = ?2",
            params![first_seq, second_seq],
        ))?;
        must(
            meshlet
                .conn
                .execute("UPDATE events SET seq = ?1 WHERE seq = -1", [second_seq]),
        )?;
        Ok(())
    }

    proptest! {
        #![proptest_config(Config::with_cases(64))]

        #[test]
        fn randomized_appends_keep_hash_chain_valid(events in generated_events()) {
            let dir = must(tempdir())?;
            let meshlet = must(Meshlet::init(dir.path()))?;
            append_generated_events(&meshlet, &events)?;

            let report = must(meshlet.verify_event_chain())?;

            prop_assert!(report.ok);
            prop_assert_eq!(report.events, events.len() as u64 + 1);
        }

        #[test]
        fn mutating_any_single_event_field_breaks_hash_chain(
            events in generated_events(),
            seq_selector in 0usize..128,
            field in 0u8..5,
        ) {
            let dir = must(tempdir())?;
            let meshlet = must(Meshlet::init(dir.path()))?;
            append_generated_events(&meshlet, &events)?;
            let count = stored_event_count(&meshlet)?;
            let seq = (seq_selector % count as usize) as i64 + 1;

            mutate_event_field(&meshlet, seq, field)?;
            let report = must(meshlet.verify_event_chain())?;

            prop_assert!(
                !report.ok,
                "field mutation {field} at seq {seq} unexpectedly verified"
            );
        }

        #[test]
        fn swapping_adjacent_events_breaks_hash_chain(
            events in generated_events(),
            seq_selector in 0usize..128,
        ) {
            let dir = must(tempdir())?;
            let meshlet = must(Meshlet::init(dir.path()))?;
            append_generated_events(&meshlet, &events)?;
            let count = stored_event_count(&meshlet)?;
            let first_seq = (seq_selector % (count as usize - 1)) as i64 + 1;

            swap_adjacent_events(&meshlet, first_seq)?;
            let report = must(meshlet.verify_event_chain())?;

            prop_assert!(
                !report.ok,
                "adjacent event swap at seq {first_seq} unexpectedly verified"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn init_creates_repo_event() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        assert_eq!(meshlet.event_count()?, 1);
        assert!(dir.path().join(DB_DIR).join(DB_FILE).exists());
        Ok(())
    }

    #[test]
    fn append_event_chains_hashes() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let first = meshlet.append_event("context.added", "agent:test", json!({"label": "a"}))?;
        let second = meshlet.append_event("context.added", "agent:test", json!({"label": "b"}))?;
        assert_eq!(second.prev_hash.as_deref(), Some(first.hash.as_str()));
        Ok(())
    }

    #[test]
    fn append_event_rejects_secret_key_names() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;

        let direct = meshlet.append_event(
            "context.added",
            "agent:test",
            json!({"label": "bad", "api_key": "value"}),
        );
        let nested = meshlet.append_event(
            "context.added",
            "agent:test",
            json!({"label": "bad", "nested": {"access-token": "value"}}),
        );

        assert!(direct.is_err());
        assert!(nested.is_err());
        assert_eq!(meshlet.event_count()?, 1);
        Ok(())
    }

    #[test]
    fn append_event_accepts_safe_payload_keys() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;

        meshlet.append_event(
            "context.added",
            "agent:test",
            json!({"label": "safe", "note": "public context"}),
        )?;

        assert_eq!(meshlet.event_count()?, 2);
        Ok(())
    }

    #[test]
    fn public_safe_append_rejects_secret_looking_values() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;

        let result = meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"label": "bad", "note": "Bearer abc123"}),
            EventVisibility::Public,
            SafetyProfile::PublicSafe,
        );

        assert!(result.is_err());
        assert_eq!(meshlet.event_count()?, 1);
        Ok(())
    }

    #[test]
    fn verify_event_chain_accepts_clean_events() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.append_event("context.added", "agent:test", json!({"label": "clean"}))?;

        let report = meshlet.verify_event_chain()?;

        assert!(report.ok);
        assert_eq!(report.events, 2);
        assert_eq!(report.first_invalid_seq, None);
        assert_eq!(report.reason, None);
        Ok(())
    }

    #[test]
    fn verify_event_chain_detects_tampered_payload() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let event =
            meshlet.append_event("context.added", "agent:test", json!({"label": "safe"}))?;
        meshlet.conn.execute(
            "UPDATE events SET payload_json = ?1 WHERE id = ?2",
            params![r#"{"label":"tampered"}"#, event.id],
        )?;

        let report = meshlet.verify_event_chain()?;

        assert!(!report.ok);
        assert_eq!(report.first_invalid_seq, Some(2));
        assert_eq!(report.reason.as_deref(), Some("hash_mismatch"));
        Ok(())
    }

    #[test]
    fn verify_event_chain_detects_tampered_prev_hash() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let event =
            meshlet.append_event("context.added", "agent:test", json!({"label": "safe"}))?;
        meshlet.conn.execute(
            "UPDATE events SET prev_hash = ?1 WHERE id = ?2",
            params!["wrong", event.id],
        )?;

        let report = meshlet.verify_event_chain()?;

        assert!(!report.ok);
        assert_eq!(report.first_invalid_seq, Some(2));
        assert_eq!(report.reason.as_deref(), Some("prev_hash_mismatch"));
        Ok(())
    }

    #[test]
    fn verify_event_chain_detects_tampered_visibility() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        let event = meshlet.append_event_with_options(
            "context.added",
            "agent:test",
            json!({"label": "private"}),
            EventVisibility::Private,
            SafetyProfile::LocalTrusted,
        )?;
        meshlet.conn.execute(
            "UPDATE events SET visibility = ?1 WHERE id = ?2",
            params!["public", event.id],
        )?;

        let report = meshlet.verify_event_chain()?;

        assert!(!report.ok);
        assert_eq!(report.first_invalid_seq, Some(2));
        assert_eq!(report.reason.as_deref(), Some("hash_mismatch"));
        Ok(())
    }
}
