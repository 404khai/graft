//! The Integrate queue (ADR 0017).

use graft_core::{GraftId, GraftName, Timestamp};
use rusqlite::{OptionalExtension, params};

use crate::{Result, Store};

/// Where an integration request is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationState {
    /// Waiting its turn.
    Queued,
    /// Being worked on.
    Running,
    /// Stopped on files that would not merge.
    Conflict,
    /// Stopped by the test gate or an error.
    Failed,
    /// Its commit is in Root.
    Landed,
    /// A dry run that passed every step but landing.
    Checked,
}

impl IntegrationState {
    /// The `state` column's value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Conflict => "conflict",
            Self::Failed => "failed",
            Self::Landed => "landed",
            Self::Checked => "checked",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "conflict" => Self::Conflict,
            "failed" => Self::Failed,
            "landed" => Self::Landed,
            "checked" => Self::Checked,
            _ => return None,
        })
    }
}

/// One integration request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Integration {
    /// Queue position (increasing).
    pub id: u64,
    /// The graft.
    pub graft_id: GraftId,
    /// Its name when it was queued.
    pub graft_name: String,
    /// The branch it lands on.
    pub target: String,
    /// The commit message given, if any.
    pub message: Option<String>,
    /// Whether it stops before landing.
    pub dry_run: bool,
    /// Where it is.
    pub state: IntegrationState,
    /// The commit it created.
    pub commit: Option<String>,
    /// Output of the step that stopped it (or of the gate).
    pub output: String,
    /// When it was queued.
    pub enqueued_at: Timestamp,
    /// When it last changed.
    pub updated_at: Timestamp,
}

const COLUMNS: &str = "id, graft_id, graft_name, target, message, dry_run, state, commit_id, output, enqueued_at, updated_at";

impl Store {
    /// Queues graft `id` (named `name`) for integration into `target`.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn enqueue(
        &mut self,
        id: GraftId,
        name: &GraftName,
        target: &str,
        message: Option<&str>,
        dry_run: bool,
    ) -> Result<u64> {
        let now = Timestamp::now().as_millis();
        self.conn
            .execute(
                "INSERT INTO integrations (graft_id, graft_name, target, message, dry_run, state, enqueued_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?6)",
                params![id.to_string(), name.as_str(), target, message, dry_run, now],
            )
            .map_err(|e| self.sql(e))?;
        Ok(u64::try_from(self.conn.last_insert_rowid()).unwrap_or(0))
    }

    fn decode_integration(&self, r: &rusqlite::Row<'_>) -> rusqlite::Result<Result<Integration>> {
        let gid: String = r.get(1)?;
        let state: String = r.get(6)?;
        let id: i64 = r.get(0)?;
        let row = (
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, bool>(5)?,
            r.get::<_, Option<String>>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, i64>(9)?,
            r.get::<_, i64>(10)?,
        );
        Ok((|| {
            let (graft_name, target, message, dry_run, commit, output, enq, upd) = row;
            Ok(Integration {
                id: u64::try_from(id).unwrap_or(0),
                graft_id: gid
                    .parse()
                    .map_err(|e| self.corrupt(format!("integrations.graft_id: {e}")))?,
                graft_name,
                target,
                message,
                dry_run,
                state: IntegrationState::parse(&state)
                    .ok_or_else(|| self.corrupt(format!("integrations.state {state:?}")))?,
                commit,
                output,
                enqueued_at: Timestamp::from_millis(enq),
                updated_at: Timestamp::from_millis(upd),
            })
        })())
    }

    fn query_integrations(&self, filter: &str, limit: i64) -> Result<Vec<Integration>> {
        let sql = format!("SELECT {COLUMNS} FROM integrations {filter} ORDER BY id LIMIT ?1");
        let mut stmt = self.conn.prepare(&sql).map_err(|e| self.sql(e))?;
        let rows = stmt
            .query_map([limit], |r| self.decode_integration(r))
            .map_err(|e| self.sql(e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| self.sql(e))??);
        }
        Ok(out)
    }

    /// The oldest queued request, if any.
    ///
    /// # Errors
    ///
    /// SQLite errors and [`crate::StoreError::Corrupt`].
    pub fn next_queued(&self) -> Result<Option<Integration>> {
        Ok(self.query_integrations("WHERE state = 'queued'", 1)?.pop())
    }

    /// The most recent `limit` requests, oldest first.
    ///
    /// # Errors
    ///
    /// SQLite errors and [`crate::StoreError::Corrupt`].
    pub fn integrations(&self, limit: u32) -> Result<Vec<Integration>> {
        let filter =
            format!("WHERE id > (SELECT COALESCE(MAX(id), 0) FROM integrations) - {limit}");
        self.query_integrations(&filter, i64::from(limit))
    }

    /// The request with `id`.
    ///
    /// # Errors
    ///
    /// SQLite errors and [`crate::StoreError::Corrupt`].
    pub fn integration(&self, id: u64) -> Result<Option<Integration>> {
        let sql = format!("SELECT {COLUMNS} FROM integrations WHERE id = ?1");
        self.conn
            .query_row(&sql, [i64::try_from(id).unwrap_or(i64::MAX)], |r| {
                self.decode_integration(r)
            })
            .optional()
            .map_err(|e| self.sql(e))?
            .transpose()
    }

    /// Moves request `id` to `state`, with its output and commit.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn set_integration(
        &mut self,
        id: u64,
        state: IntegrationState,
        output: &str,
        commit: Option<&str>,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE integrations SET state = ?2, output = ?3, commit_id = COALESCE(?4, commit_id), updated_at = ?5
                 WHERE id = ?1",
                params![
                    i64::try_from(id).unwrap_or(i64::MAX),
                    state.as_str(),
                    output,
                    commit,
                    Timestamp::now().as_millis()
                ],
            )
            .map_err(|e| self.sql(e))?;
        Ok(())
    }

    /// Puts requests left `running` by an interrupted `graft integrate`
    /// back in the queue. Call it while holding the queue lock.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn requeue_running(&mut self) -> Result<u64> {
        let n = self
            .conn
            .execute(
                "UPDATE integrations SET state = 'queued' WHERE state = 'running'",
                [],
            )
            .map_err(|e| self.sql(e))?;
        Ok(n as u64)
    }
}
