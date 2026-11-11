//! Scheduler claims and refused writes (ADR 0018).

use graft_core::{GraftId, Timestamp};
use rusqlite::params;

use crate::{Result, Store};

/// A claim, as stored.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClaimRow {
    /// The graft's name.
    pub graft: String,
    /// The graft's ID.
    pub graft_id: GraftId,
    /// The file.
    pub path: String,
    /// The symbol, or `None` for the whole file.
    pub symbol: Option<String>,
    /// The plan and position that created it.
    pub plan: Option<(String, u32)>,
}

/// A refused write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionRow {
    /// The graft that wrote.
    pub graft: String,
    /// The file.
    pub path: String,
    /// The verdict, naming the holder.
    pub verdict: String,
    /// When.
    pub at: Timestamp,
}

impl Store {
    /// Records claims for graft `id` (existing ones are kept).
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn add_claims(
        &mut self,
        id: GraftId,
        claims: &[(String, Option<String>)],
        plan: Option<(&str, u32)>,
    ) -> Result<()> {
        let gid = id.to_string();
        let path = self.path.clone();
        let sql = |source| crate::StoreError::Sqlite {
            path: path.clone(),
            source,
        };
        let tx = self.conn.transaction().map_err(sql)?;
        for (p, s) in claims {
            tx.execute(
                "INSERT OR IGNORE INTO claims (graft_id, path, symbol, plan, plan_order)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    gid,
                    p,
                    s.as_deref().unwrap_or(""),
                    plan.map(|(p, _)| p),
                    plan.map(|(_, o)| i64::from(o))
                ],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)
    }

    /// Claims held by live grafts, optionally only on `path`.
    ///
    /// # Errors
    ///
    /// SQLite errors and [`crate::StoreError::Corrupt`].
    pub fn live_claims(&self, path: Option<&str>) -> Result<Vec<ClaimRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT g.name, c.graft_id, c.path, c.symbol, c.plan, c.plan_order
                 FROM claims c JOIN grafts g ON g.graft_id = c.graft_id
                 WHERE g.state = 'live' AND (?1 IS NULL OR c.path = ?1)
                 ORDER BY g.name, c.path, c.symbol",
            )
            .map_err(|e| self.sql(e))?;
        let rows = stmt
            .query_map([path], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                ))
            })
            .map_err(|e| self.sql(e))?;
        let mut out = Vec::new();
        for row in rows {
            let (graft, gid, path, symbol, plan, order) = row.map_err(|e| self.sql(e))?;
            out.push(ClaimRow {
                graft,
                graft_id: gid
                    .parse()
                    .map_err(|e| self.corrupt(format!("claims.graft_id: {e}")))?,
                path,
                symbol: (!symbol.is_empty()).then_some(symbol),
                plan: plan.zip(order.and_then(|o| u32::try_from(o).ok())),
            });
        }
        Ok(out)
    }

    /// Records a refused write.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn record_admission(&mut self, graft: &str, path: &str, verdict: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO admissions (graft_name, path, verdict, at) VALUES (?1, ?2, ?3, ?4)",
                params![graft, path, verdict, Timestamp::now().as_millis()],
            )
            .map_err(|e| self.sql(e))?;
        Ok(())
    }

    /// The most recent `limit` refused writes, oldest first.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn admissions(&self, limit: u32) -> Result<Vec<AdmissionRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT graft_name, path, verdict, at FROM
                 (SELECT * FROM admissions ORDER BY id DESC LIMIT ?1) ORDER BY id",
            )
            .map_err(|e| self.sql(e))?;
        let rows = stmt
            .query_map([i64::from(limit)], |r| {
                Ok(AdmissionRow {
                    graft: r.get(0)?,
                    path: r.get(1)?,
                    verdict: r.get(2)?,
                    at: Timestamp::from_millis(r.get(3)?),
                })
            })
            .map_err(|e| self.sql(e))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| self.sql(e))
    }
}
