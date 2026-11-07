//! Moving a virtual graft to a new base (`graft rebase`, ADR 0017).

use graft_core::{CommitId, GraftId, Timestamp};
use rusqlite::{TransactionBehavior, params};

use crate::delta::{Change, Op};
use crate::journal::insert_op;
use crate::{Journaled, Result, Store, StoreError};

impl Store {
    /// Moves graft `id` to `base` in one transaction: appends an
    /// [`Op::Rebase`], empties the delta, journals `ops` (each with the
    /// delta rows it changed, computed over `base`'s tree) and records the
    /// new base. Returns the rebase op's sequence number.
    ///
    /// # Errors
    ///
    /// [`StoreError::StillWriting`] if files are staged, and SQLite errors.
    /// Nothing is written on error.
    pub fn rebase(
        &mut self,
        id: GraftId,
        base: &CommitId,
        at: Timestamp,
        ops: &[(Op, Vec<Change>)],
    ) -> Result<u64> {
        let staged = self.staged(id)?.len();
        if staged > 0 {
            let graft = self
                .graft_by_id(id)?
                .ok_or(StoreError::NoSuchGraft { id })?;
            return Err(StoreError::StillWriting {
                graft: graft.name,
                files: staged as u64,
            });
        }
        let gid = id.to_string();
        let path = self.path.clone();
        let sql = |source| StoreError::Sqlite {
            path: path.clone(),
            source,
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        let mut seq: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM op_log WHERE graft_id = ?1",
                [&gid],
                |r| r.get(0),
            )
            .map_err(sql)?;
        let first = seq;
        let marker = Op::Rebase {
            base: base.to_string(),
        };
        insert_op(
            &tx,
            &gid,
            seq,
            &Journaled {
                at,
                op: &marker,
                actor: Some("graft rebase"),
                changes: &[],
                staged: &[],
                sync: false,
            },
        )
        .map_err(sql)?;
        tx.execute("DELETE FROM delta WHERE graft_id = ?1", [&gid])
            .map_err(sql)?;
        for (op, changes) in ops {
            seq += 1;
            insert_op(
                &tx,
                &gid,
                seq,
                &Journaled {
                    at,
                    op,
                    actor: Some("graft rebase"),
                    changes,
                    staged: &[],
                    sync: false,
                },
            )
            .map_err(sql)?;
        }
        tx.execute(
            "UPDATE grafts SET base_commit = ?2, updated_at = ?3 WHERE graft_id = ?1",
            params![gid, base.as_str(), at.as_millis()],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(u64::try_from(first).unwrap_or(0))
    }

    /// The sequence number of graft `id`'s last rebase, 0 if none.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn last_rebase(&self, id: GraftId) -> Result<u64> {
        let seq: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) FROM op_log WHERE graft_id = ?1 AND kind = 'rebase'",
                [id.to_string()],
                |r| r.get(0),
            )
            .map_err(|e| self.sql(e))?;
        Ok(u64::try_from(seq).unwrap_or(0))
    }
}
