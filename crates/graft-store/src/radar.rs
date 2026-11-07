//! Cached results of Radar's speculative test runs (ADR 0016).

use graft_core::Timestamp;
use rusqlite::{OptionalExtension, params};

use crate::{Result, Store};

/// How a speculative test run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckOutcome {
    /// The command exited 0.
    Pass,
    /// It exited non-zero.
    Fail,
    /// A three-way merge failed, so nothing ran.
    Conflict,
    /// It ran past its timeout and was killed.
    Timeout,
    /// The check could not be set up.
    Error,
}

impl CheckOutcome {
    /// The `outcome` column's value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Conflict => "conflict",
            Self::Timeout => "timeout",
            Self::Error => "error",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pass" => Self::Pass,
            "fail" => Self::Fail,
            "conflict" => Self::Conflict,
            "timeout" => Self::Timeout,
            "error" => Self::Error,
            _ => return None,
        })
    }
}

/// A recorded check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadarCheck {
    /// How it ended.
    pub outcome: CheckOutcome,
    /// The end of its output (or why it could not run).
    pub output: String,
    /// When it ran.
    pub checked_at: Timestamp,
}

/// Checks older than this are forgotten when a new one is recorded.
const KEEP_MILLIS: i64 = 7 * 24 * 60 * 60 * 1000;

impl Store {
    /// The recorded check with `fingerprint` (64 hex digits), if any.
    ///
    /// # Errors
    ///
    /// SQLite errors and [`crate::StoreError::Corrupt`].
    pub fn radar_check(&self, fingerprint: &str) -> Result<Option<RadarCheck>> {
        let row: Option<(String, String, i64)> = self
            .conn
            .query_row(
                "SELECT outcome, output, checked_at FROM radar_checks WHERE fingerprint = ?1",
                [fingerprint],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| self.sql(e))?;
        row.map(|(o, output, at)| {
            Ok(RadarCheck {
                outcome: CheckOutcome::parse(&o)
                    .ok_or_else(|| self.corrupt(format!("radar_checks.outcome {o:?}")))?,
                output,
                checked_at: Timestamp::from_millis(at),
            })
        })
        .transpose()
    }

    /// Records a check, replacing any with the same fingerprint, and
    /// forgets checks older than a week.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn record_radar_check(
        &mut self,
        fingerprint: &str,
        outcome: CheckOutcome,
        output: &str,
    ) -> Result<()> {
        let now = Timestamp::now().as_millis();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO radar_checks (fingerprint, outcome, output, checked_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![fingerprint, outcome.as_str(), output, now],
            )
            .map_err(|e| self.sql(e))?;
        self.conn
            .execute(
                "DELETE FROM radar_checks WHERE checked_at < ?1",
                [now - KEEP_MILLIS],
            )
            .map_err(|e| self.sql(e))?;
        Ok(())
    }
}
