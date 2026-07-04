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
