//! Persisting deltas and the op log (ADR 0008).
//!
//! [`Store::record`] writes one op log row together with the delta rows it
//! changed (and any staging bookkeeping) in a single transaction, so the op
//! log and the delta never disagree.

use std::collections::HashSet;

use graft_core::{BlobHash, GraftId, Timestamp};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

use crate::delta::{Change, Content, Delta, Entry, Op, Record, RelPath, WriteKind};
use crate::{Result, Store};

/// One operation to journal with [`Store::record`].
#[derive(Debug, Clone, Copy)]
pub struct Journaled<'a> {
    /// When it happened.
    pub at: Timestamp,
    /// What happened.
    pub op: &'a Op,
    /// Who did it, if known.
    pub actor: Option<&'a str>,
    /// The delta rows it changed (from [`Delta::apply`]).
    pub changes: &'a [Change],
    /// Staging bookkeeping that commits with it.
    pub staged: &'a [StagedChange],
    /// Wait for the commit to reach disk (for `fsync`).
    pub sync: bool,
}

/// The op-specific columns of an op log row.
#[derive(Default)]
struct OpColumns<'a> {
    path_to: Option<&'a [u8]>,
    blob: Option<String>,
    mode: Option<i64>,
    mtime: Option<i64>,
    target: Option<&'a [u8]>,
}

fn op_columns(op: &Op) -> OpColumns<'_> {
    match op {
        Op::Write { blob, mode, .. } => OpColumns {
            blob: Some(blob.to_string()),
            mode: Some(i64::from(*mode)),
            ..OpColumns::default()
        },
        Op::Mkdir { mode, .. } | Op::Chmod { mode, .. } => OpColumns {
            mode: Some(i64::from(*mode)),
            ..OpColumns::default()
        },
        Op::Symlink { target, .. } => OpColumns {
            target: Some(target),
            ..OpColumns::default()
        },
        Op::Touch { mtime, .. } => OpColumns {
            mtime: Some(*mtime),
            ..OpColumns::default()
        },
        Op::Rename { to, .. } => OpColumns {
            path_to: Some(to),
            ..OpColumns::default()
        },
        Op::Unlink { .. } | Op::Rmdir { .. } => OpColumns::default(),
    }
}

/// The columns of a delta row.
#[derive(Default)]
struct EntryColumns<'a> {
    kind: &'static str,
    blob: Option<String>,
    lower: Option<&'a [u8]>,
    target: Option<&'a [u8]>,
    mode: Option<i64>,
    mtime: Option<i64>,
    opaque: bool,
}

fn entry_columns(e: &Entry) -> EntryColumns<'_> {
    match e {
        Entry::File {
            content,
            mode,
            mtime,
        } => {
            let (blob, lower) = match content {
                Content::Blob(h) => (Some(h.to_string()), None),
                Content::Lower(l) => (None, Some(l.as_slice())),
            };
            EntryColumns {
                kind: "file",
                blob,
                lower,
                mode: Some(i64::from(*mode)),
                mtime: *mtime,
                ..EntryColumns::default()
            }
        }
        Entry::Symlink { target, mtime } => EntryColumns {
            kind: "symlink",
            target: Some(target),
            mtime: *mtime,
            ..EntryColumns::default()
        },
        Entry::Dir {
            mode,
            opaque,
            mtime,
        } => EntryColumns {
            kind: "dir",
            mode: Some(i64::from(*mode)),
            mtime: *mtime,
            opaque: *opaque,
            ..EntryColumns::default()
        },
        Entry::Tombstone => EntryColumns {
            kind: "tombstone",
            ..EntryColumns::default()
        },
    }
}

/// A file open for writing whose contents are not journaled yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    /// Identifies the staging file (`work/<id>`).
    pub id: u64,
    /// Where the file is in the graft.
    pub path: RelPath,
    /// Its permission bits.
    pub mode: u32,
}

/// Staging bookkeeping that must commit with an op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagedChange {
    /// The staged file moved (renamed, or a directory above it was).
    Moved {
        /// Its ID.
        id: u64,
        /// Its new path.
        path: RelPath,
    },
    /// The staged file is gone (journaled, or its path was deleted).
    Dropped {
        /// Its ID.
        id: u64,
    },
}

fn i64_of(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn u32_of(v: Option<i64>) -> u32 {
    v.and_then(|v| u32::try_from(v).ok()).unwrap_or(0)
}

impl Store {
    /// The delta of graft `id`.
    ///
    /// # Errors
    ///
    /// SQLite errors and [`crate::StoreError::Corrupt`].
    pub fn delta(&self, id: GraftId) -> Result<Delta> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT path, kind, blob, lower, target, mode, mtime, opaque
                 FROM delta WHERE graft_id = ?1",
            )
            .map_err(|e| self.sql(e))?;
        let rows = stmt
            .query_map([id.to_string()], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<Vec<u8>>>(3)?,
                    r.get::<_, Option<Vec<u8>>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, bool>(7)?,
                ))
            })
            .map_err(|e| self.sql(e))?;
        let mut entries = Vec::new();
        for row in rows {
            let (path, kind, blob, lower, target, mode, mtime, opaque) =
                row.map_err(|e| self.sql(e))?;
            let entry = match kind.as_str() {
                "file" => Entry::File {
                    content: match (blob, lower) {
                        (Some(b), None) => Content::Blob(
                            b.parse()
                                .map_err(|e| self.corrupt(format!("delta.blob: {e}")))?,
                        ),
                        (None, Some(l)) => Content::Lower(l),
                        _ => return Err(self.corrupt("delta file without exactly one source")),
                    },
                    mode: u32_of(mode),
                    mtime,
                },
                "symlink" => Entry::Symlink {
                    target: target.unwrap_or_default(),
                    mtime,
                },
                "dir" => Entry::Dir {
                    mode: u32_of(mode),
                    opaque,
                    mtime,
                },
                "tombstone" => Entry::Tombstone,
                other => return Err(self.corrupt(format!("delta.kind {other:?}"))),
            };
            entries.push((path, entry));
        }
        Ok(Delta::from_entries(entries))
    }

    /// Journals `j.op` for graft `id` with the delta rows it changed and
    /// its staging bookkeeping, in one transaction. Returns the op's
    /// sequence number.
    ///
    /// # Errors
    ///
    /// SQLite errors; nothing is written on error.
    pub fn record(&mut self, id: GraftId, j: &Journaled<'_>) -> Result<u64> {
        if j.sync {
            self.conn
                .pragma_update(None, "synchronous", "FULL")
                .map_err(|e| self.sql(e))?;
        }
        let result = self.record_tx(id, j);
        if j.sync {
            let _ = self.conn.pragma_update(None, "synchronous", "NORMAL");
        }
        result
    }

    fn record_tx(&mut self, id: GraftId, j: &Journaled<'_>) -> Result<u64> {
        let gid = id.to_string();
        let path = self.path.clone();
        let sql = |source| crate::StoreError::Sqlite {
            path: path.clone(),
            source,
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        let seq: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM op_log WHERE graft_id = ?1",
                [&gid],
                |r| r.get(0),
            )
            .map_err(sql)?;
        let c = op_columns(j.op);
        tx.execute(
            "INSERT INTO op_log (graft_id, seq, at, kind, path, path_to, blob, actor, mode, mtime, target)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                gid,
                seq,
                j.at.as_millis(),
                j.op.kind(),
                j.op.path(),
                c.path_to,
                c.blob,
                j.actor,
                c.mode,
                c.mtime,
                c.target
            ],
        )
        .map_err(sql)?;
        for (p, _, new) in j.changes {
            if let Some(e) = new {
                let c = entry_columns(e);
                tx.execute(
                    "INSERT OR REPLACE INTO delta (graft_id, path, kind, blob, lower, target, mode, mtime, opaque)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![gid, p, c.kind, c.blob, c.lower, c.target, c.mode, c.mtime, c.opaque],
                )
            } else {
                tx.execute(
                    "DELETE FROM delta WHERE graft_id = ?1 AND path = ?2",
                    params![gid, p],
                )
            }
            .map_err(sql)?;
        }
        for s in j.staged {
            match s {
                StagedChange::Moved { id: sid, path: p } => tx.execute(
                    "UPDATE staged SET path = ?3 WHERE graft_id = ?1 AND id = ?2",
                    params![gid, i64_of(*sid), p],
                ),
                StagedChange::Dropped { id: sid } => tx.execute(
                    "DELETE FROM staged WHERE graft_id = ?1 AND id = ?2",
                    params![gid, i64_of(*sid)],
                ),
            }
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)?;
        Ok(u64::try_from(seq).unwrap_or(0))
    }

    /// The op log of graft `id`, oldest first.
    ///
    /// # Errors
    ///
    /// SQLite errors and [`crate::StoreError::Corrupt`].
    pub fn op_log(&self, id: GraftId) -> Result<Vec<Record>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT seq, at, kind, path, path_to, blob, actor, mode, mtime, target
                 FROM op_log WHERE graft_id = ?1 ORDER BY seq",
            )
            .map_err(|e| self.sql(e))?;
        let rows = stmt
            .query_map([id.to_string()], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                    r.get::<_, Option<Vec<u8>>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                    r.get::<_, Option<i64>>(8)?,
                    r.get::<_, Option<Vec<u8>>>(9)?,
                ))
            })
            .map_err(|e| self.sql(e))?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, at, kind, path, path_to, blob, actor, mode, mtime, target) =
                row.map_err(|e| self.sql(e))?;
            let blob = || -> Result<BlobHash> {
                blob.as_deref()
                    .ok_or_else(|| self.corrupt("op_log write without a blob"))?
                    .parse()
                    .map_err(|e| self.corrupt(format!("op_log.blob: {e}")))
            };
            let write = |kind| -> Result<Op> {
                Ok(Op::Write {
                    kind,
                    path: path.clone(),
                    blob: blob()?,
                    mode: u32_of(mode),
                })
            };
            let op = match kind.as_str() {
                "create" => write(WriteKind::Create)?,
                "write" => write(WriteKind::Write)?,
                "truncate" => write(WriteKind::Truncate)?,
                "mkdir" => Op::Mkdir {
                    path,
                    mode: u32_of(mode),
                },
                "symlink" => Op::Symlink {
                    path,
                    target: target.unwrap_or_default(),
                },
                "unlink" => Op::Unlink { path },
                "rmdir" => Op::Rmdir { path },
                "rename" => Op::Rename {
                    from: path,
                    to: path_to.ok_or_else(|| self.corrupt("op_log rename without path_to"))?,
                },
                "chmod" => Op::Chmod {
                    path,
                    mode: u32_of(mode),
                },
                "touch" => Op::Touch {
                    path,
                    mtime: mtime.ok_or_else(|| self.corrupt("op_log touch without mtime"))?,
                },
                other => return Err(self.corrupt(format!("op_log.kind {other:?}"))),
            };
            out.push(Record {
                seq: u64::try_from(seq).unwrap_or(0),
                at: Timestamp::from_millis(at),
                op,
                actor,
            });
        }
        Ok(out)
    }

    /// Records that a staging file now backs `path` of graft `id`.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn stage(&mut self, id: GraftId, staged: &Staged) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO staged (graft_id, id, path, mode) VALUES (?1, ?2, ?3, ?4)",
                params![
                    id.to_string(),
                    i64_of(staged.id),
                    staged.path,
                    i64::from(staged.mode)
                ],
            )
            .map_err(|e| self.sql(e))?;
        Ok(())
    }

    /// Forgets staging file `staged` of graft `id`.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn unstage(&mut self, id: GraftId, staged: u64) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM staged WHERE graft_id = ?1 AND id = ?2",
                params![id.to_string(), i64_of(staged)],
            )
            .map_err(|e| self.sql(e))?;
        Ok(())
    }

    /// Staging files of graft `id` that were never journaled.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn staged(&self, id: GraftId) -> Result<Vec<Staged>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, mode FROM staged WHERE graft_id = ?1 ORDER BY id")
            .map_err(|e| self.sql(e))?;
        let rows = stmt
            .query_map([id.to_string()], |r| {
                Ok(Staged {
                    id: u64::try_from(r.get::<_, i64>(0)?).unwrap_or(0),
                    path: r.get(1)?,
                    mode: u32_of(r.get(2)?),
                })
            })
            .map_err(|e| self.sql(e))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| self.sql(e))
    }

    /// The highest staging ID graft `id` has used.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn max_staged_id(&self, id: GraftId) -> Result<u64> {
        let max: Option<i64> = self
            .conn
            .query_row(
                "SELECT MAX(id) FROM staged WHERE graft_id = ?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| self.sql(e))?
            .flatten();
        Ok(max.and_then(|m| u64::try_from(m).ok()).unwrap_or(0))
    }

    /// Discards graft `id`'s delta and staging rows (its op log stays until
    /// the graft's record is deleted).
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn clear_delta(&mut self, id: GraftId) -> Result<()> {
        for table in ["delta", "staged"] {
            self.conn
                .execute(
                    &format!("DELETE FROM {table} WHERE graft_id = ?1"),
                    [id.to_string()],
                )
                .map_err(|e| self.sql(e))?;
        }
        Ok(())
    }

    /// Every delta blob any graft of this Root references.
    ///
    /// # Errors
    ///
    /// SQLite errors and [`crate::StoreError::Corrupt`].
    pub fn referenced_blobs(&self) -> Result<HashSet<BlobHash>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT blob FROM delta WHERE blob IS NOT NULL")
            .map_err(|e| self.sql(e))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| self.sql(e))?;
        let mut out = HashSet::new();
        for row in rows {
            let hex = row.map_err(|e| self.sql(e))?;
            out.insert(
                hex.parse()
                    .map_err(|e| self.corrupt(format!("delta.blob: {e}")))?,
            );
        }
        Ok(out)
    }
}
