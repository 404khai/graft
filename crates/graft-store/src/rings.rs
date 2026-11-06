//! Bud, Ring and rewind (ADR 0013).
//!
//! All three are metadata operations: each runs in one SQLite transaction
//! and copies rows, never blob contents. Blobs stay alive while any `delta`
//! or `ring_delta` row references them ([`Store::referenced_blobs`]).

use graft_core::{GraftId, GraftName, GraftState, RingLabel, Timestamp};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use crate::delta::Op;
use crate::{GraftRecord, Result, Store, StoreError};

/// A named checkpoint of a graft's delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ring {
    /// Its label, unique per graft.
    pub label: RingLabel,
    /// The sequence number of the graft's last op when it was taken (0 if
    /// the graft had none).
    pub seq: u64,
    /// When it was taken.
    pub created_at: Timestamp,
    /// How many delta rows it holds.
    pub paths: u64,
}

/// What [`Store::rewind`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewound {
    /// The ring holding the state from before the rewind; rewinding to it
    /// re-applies everything the rewind undid.
    pub undo: RingLabel,
    /// The sequence number of the `rewind` op.
    pub seq: u64,
}

/// Columns shared by `delta` and `ring_delta`.
const ENTRY_COLUMNS: &str = "path, kind, blob, lower, target, mode, mtime, opaque";

fn last_seq(tx: &Transaction<'_>, gid: &str) -> rusqlite::Result<u64> {
    let seq: i64 = tx.query_row(
        "SELECT COALESCE(MAX(seq), 0) FROM op_log WHERE graft_id = ?1",
        [gid],
        |r| r.get(0),
    )?;
    Ok(u64::try_from(seq).unwrap_or(0))
}

fn i64_of(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// Inserts ring `label` of graft `gid` at `seq` with a copy of the delta.
/// `false` if the graft already has a ring with that label.
fn insert_ring(
    tx: &Transaction<'_>,
    gid: &str,
    label: &RingLabel,
    seq: u64,
) -> rusqlite::Result<bool> {
    let inserted = tx.execute(
        "INSERT OR IGNORE INTO rings (graft_id, label, seq, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            gid,
            label.as_str(),
            i64_of(seq),
            Timestamp::now().as_millis()
        ],
    )?;
    if inserted == 0 {
        return Ok(false);
    }
    tx.execute(
        &format!(
            "INSERT INTO ring_delta (graft_id, label, {ENTRY_COLUMNS})
             SELECT graft_id, ?2, {ENTRY_COLUMNS} FROM delta WHERE graft_id = ?1"
        ),
        params![gid, label.as_str()],
    )?;
    Ok(true)
}

impl Store {
    fn begin(&mut self) -> Result<Transaction<'_>> {
        let path = self.path.clone();
        self.conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| StoreError::Sqlite { path, source })
    }

    fn live(&self, id: GraftId) -> Result<GraftRecord> {
        let g = self
            .graft_by_id(id)?
            .ok_or(StoreError::NoSuchGraft { id })?;
        if !matches!(g.state, GraftState::Created | GraftState::Live) {
            return Err(StoreError::InvalidTransition {
                name: g.name,
                from: g.state,
                to: GraftState::Live,
            });
        }
        Ok(g)
    }

    /// Buds graft `src` into a new graft `name`: same base, backend and
    /// layers, with a copy of its delta and op log. Blobs are shared. The
    /// bud starts in [`GraftState::Created`].
    ///
    /// # Errors
    ///
    /// [`StoreError::NameTaken`], [`StoreError::NoSuchGraft`],
    /// [`StoreError::InvalidTransition`] if `src` is not live, and SQLite
    /// errors. Nothing is written on error.
    pub fn bud(&mut self, src: GraftId, name: &GraftName) -> Result<GraftRecord> {
        let source = self.live(src)?;
        let id = GraftId::random()?;
        let (sid, gid) = (src.to_string(), id.to_string());
        let now = Timestamp::now().as_millis();
        let tx = self.begin()?;
        let inserted = tx.execute(
            "INSERT INTO grafts (graft_id, name, base_commit, state, created_at, updated_at, backend, budded_from)
             SELECT ?2, ?3, base_commit, 'created', ?4, ?4, backend, graft_id
             FROM grafts WHERE graft_id = ?1",
            params![sid, gid, name.as_str(), now],
        );
        match inserted {
            Ok(_) => {}
            Err(e) if e.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) => {
                drop(tx);
                return Err(if self.graft(name)?.is_some() {
                    StoreError::NameTaken { name: name.clone() }
                } else {
                    self.sql(e)
                });
            }
            Err(e) => {
                drop(tx);
                return Err(self.sql(e));
            }
        }
        let copied = (|| {
            tx.execute(
                &format!(
                    "INSERT INTO delta (graft_id, {ENTRY_COLUMNS})
                     SELECT ?2, {ENTRY_COLUMNS} FROM delta WHERE graft_id = ?1"
                ),
                params![sid, gid],
            )?;
            tx.execute(
                "INSERT INTO op_log (graft_id, seq, at, kind, path, path_to, blob, actor, mode, mtime, target, to_seq)
                 SELECT ?2, seq, at, kind, path, path_to, blob, actor, mode, mtime, target, to_seq
                 FROM op_log WHERE graft_id = ?1",
                params![sid, gid],
            )?;
            tx.execute(
                "INSERT INTO layer_refs (graft_id, layer_key)
                 SELECT ?2, layer_key FROM layer_refs WHERE graft_id = ?1",
                params![sid, gid],
            )?;
            tx.commit()
        })();
        copied.map_err(|e| self.sql(e))?;
        tracing::debug!(src = %source.name, bud = %name, "budded");
        self.graft_by_id(id)?.ok_or(StoreError::NoSuchGraft { id })
    }

    /// Rings graft `id`: records its delta as checkpoint `label` (default
    /// `ring-<seq>`), at the sequence number of its last op.
    ///
    /// # Errors
    ///
    /// [`StoreError::RingTaken`], [`StoreError::NoSuchGraft`],
    /// [`StoreError::InvalidTransition`] if the graft is not live, and
    /// SQLite errors.
    pub fn ring(&mut self, id: GraftId, label: Option<RingLabel>) -> Result<Ring> {
        let graft = self.live(id)?;
        let gid = id.to_string();
        let tx = self.begin()?;
        let done = (|| {
            let seq = last_seq(&tx, &gid)?;
            let label = label.unwrap_or_else(|| RingLabel::at_seq(seq));
            let inserted = insert_ring(&tx, &gid, &label, seq)?;
            if inserted {
                tx.commit()?;
            }
            Ok((label, inserted))
        })();
        let (label, inserted) = done.map_err(|e| self.sql(e))?;
        if !inserted {
            return Err(StoreError::RingTaken {
                graft: graft.name,
                label,
            });
        }
        self.ring_named(id, &label)?.ok_or(StoreError::NoSuchRing {
            graft: graft.name,
            label,
        })
    }

    /// Graft `id`'s rings, oldest first.
    ///
    /// # Errors
    ///
    /// SQLite errors and [`StoreError::Corrupt`].
    pub fn rings(&self, id: GraftId) -> Result<Vec<Ring>> {
        self.query_rings("", id, None)
    }

    /// Graft `id`'s ring `label`, if it has one.
    ///
    /// # Errors
    ///
    /// SQLite errors and [`StoreError::Corrupt`].
    pub fn ring_named(&self, id: GraftId, label: &RingLabel) -> Result<Option<Ring>> {
        Ok(self.query_rings("AND r.label = ?2", id, Some(label))?.pop())
    }

    fn query_rings(
        &self,
        filter: &str,
        id: GraftId,
        label: Option<&RingLabel>,
    ) -> Result<Vec<Ring>> {
        let sql = format!(
            "SELECT r.label, r.seq, r.created_at,
                    (SELECT count(*) FROM ring_delta d WHERE d.graft_id = r.graft_id AND d.label = r.label)
             FROM rings r WHERE r.graft_id = ?1 {filter} ORDER BY r.seq, r.created_at, r.label"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(|e| self.sql(e))?;
        let map = |r: &rusqlite::Row<'_>| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        };
        let gid = id.to_string();
        let rows = match label {
            Some(l) => stmt.query_map(params![gid, l.as_str()], map),
            None => stmt.query_map(params![gid], map),
        }
        .map_err(|e| self.sql(e))?;
        let mut out = Vec::new();
        for row in rows {
            let (label, seq, at, paths) = row.map_err(|e| self.sql(e))?;
            out.push(Ring {
                label: label
                    .parse()
                    .map_err(|e| self.corrupt(format!("rings.label: {e}")))?,
                seq: u64::try_from(seq).unwrap_or(0),
                created_at: Timestamp::from_millis(at),
                paths: u64::try_from(paths).unwrap_or(0),
            });
        }
        Ok(out)
    }

    /// Rewinds graft `id` to ring `label`: rings the current state as
    /// `undo-<seq>` (unless that ring exists), appends a `rewind` op and
    /// replaces the delta with the ring's copy, in one transaction. The
    /// graft must not be mounted (its view would be stale).
    ///
    /// # Errors
    ///
    /// [`StoreError::NoSuchRing`], [`StoreError::StillWriting`] if files
    /// are staged, [`StoreError::NoSuchGraft`],
    /// [`StoreError::InvalidTransition`] if it is not live, and SQLite
    /// errors. Nothing is written on error.
    pub fn rewind(&mut self, id: GraftId, label: &RingLabel, at: Timestamp) -> Result<Rewound> {
        let graft = self.live(id)?;
        let staged = self.staged(id)?.len();
        if staged > 0 {
            return Err(StoreError::StillWriting {
                graft: graft.name,
                files: staged as u64,
            });
        }
        let gid = id.to_string();
        let tx = self.begin()?;
        let done = (|| {
            let to: Option<i64> = tx
                .query_row(
                    "SELECT seq FROM rings WHERE graft_id = ?1 AND label = ?2",
                    params![gid, label.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(to) = to else {
                return Ok(None);
            };
            let last = last_seq(&tx, &gid)?;
            let undo = RingLabel::undo(last);
            insert_ring(&tx, &gid, &undo, last)?;
            let seq = last + 1;
            let op = Op::Rewind {
                to: u64::try_from(to).unwrap_or(0),
            };
            tx.execute(
                "INSERT INTO op_log (graft_id, seq, at, kind, path, to_seq) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![gid, i64_of(seq), at.as_millis(), op.kind(), op.path(), to],
            )?;
            tx.execute("DELETE FROM delta WHERE graft_id = ?1", [&gid])?;
            tx.execute(
                &format!(
                    "INSERT INTO delta (graft_id, {ENTRY_COLUMNS})
                     SELECT graft_id, {ENTRY_COLUMNS} FROM ring_delta
                     WHERE graft_id = ?1 AND label = ?2"
                ),
                params![gid, label.as_str()],
            )?;
            tx.commit()?;
            Ok(Some(Rewound { undo, seq }))
        })();
        done.map_err(|e| self.sql(e))?
            .ok_or_else(|| StoreError::NoSuchRing {
                graft: graft.name,
                label: label.clone(),
            })
    }
}
