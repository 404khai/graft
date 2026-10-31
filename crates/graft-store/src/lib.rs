//! Durable state for Graft.
//!
//! Each Root has one SQLite database, `$GRAFT_HOME/roots/<root-id>/graft.db`,
//! opened in WAL mode so readers never block the writer and concurrent
//! `graft` processes are safe. It holds:
//!
//! - `roots`: the single Root this database belongs to;
//! - `grafts`: one record per graft (name, ID, base commit, state, times);
//! - `op_log`: the append-only journal of graft writes (created in schema
//!   v1, written from the delta store phase on).
//!
//! The schema is versioned with `PRAGMA user_version` and migrated on open;
//! see [`SCHEMA_VERSION`] and `docs/on-disk-format.md`.
//!
//! [`registry`] finds and registers Roots for a repository.

#![forbid(unsafe_code)]

mod error;
pub mod registry;
mod schema;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use graft_core::{CommitId, GraftId, GraftName, GraftState, RootId, Timestamp};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior};

pub use error::{Result, StoreError};
pub use schema::SCHEMA_VERSION;

/// File name of the metadata database inside a Root's directory.
pub const DB_FILE_NAME: &str = "graft.db";

/// How long a connection waits for another process's write lock.
const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

/// The Root a database belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootRecord {
    /// The Root's ID; also its directory name under `$GRAFT_HOME/roots/`.
    pub id: RootId,
    /// Canonical path of the working tree's top level.
    pub path: PathBuf,
    /// The first commit, as defined by `graft_root::Repo::first_commit`.
    pub first_commit: CommitId,
    /// When the Root was registered.
    pub registered_at: Timestamp,
}

/// Whether registering a Root created it or found it already there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    /// This call registered the Root.
    Created,
    /// The Root was already registered; nothing changed.
    Existing,
}

/// One graft's metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraftRecord {
    /// Stable ID.
    pub id: GraftId,
    /// Name, unique among the Root's grafts that are not pruned.
    pub name: GraftName,
    /// The commit the graft was created from.
    pub base_commit: CommitId,
    /// Lifecycle state.
    pub state: GraftState,
    /// When the graft was created.
    pub created_at: Timestamp,
    /// When its record last changed.
    pub updated_at: Timestamp,
}

/// An open metadata database for one Root.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
    path: PathBuf,
}

/// Creates `dir` and missing parents, readable only by the current user.
fn create_private_dir(dir: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir).map_err(|source| StoreError::Io {
        action: "creating",
        path: dir.to_owned(),
        source,
    })
}

fn is_busy(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    )
}

impl Store {
    /// Path of the database inside a Root directory.
    #[must_use]
    pub fn db_path(root_dir: &Path) -> PathBuf {
        root_dir.join(DB_FILE_NAME)
    }

    /// Creates (or reuses) the database in `root_dir` and records `root` in
    /// it. Safe to call from many processes at once: exactly one of them
    /// gets [`Registration::Created`].
    ///
    /// # Errors
    ///
    /// [`StoreError::RootMismatch`] if the database already belongs to a
    /// different Root, plus I/O and SQLite errors.
    pub fn create(root_dir: &Path, root: &RootRecord) -> Result<(Self, Registration)> {
        create_private_dir(root_dir)?;
        let path = Self::db_path(root_dir);
        let conn = Connection::open(&path).map_err(|source| StoreError::Sqlite {
            path: path.clone(),
            source,
        })?;
        let store = Self::configure(conn, path)?;
        let inserted = store
            .conn
            .execute(
                "INSERT INTO roots (singleton, root_id, path, first_commit, registered_at)
                 VALUES (1, ?1, ?2, ?3, ?4)
                 ON CONFLICT (singleton) DO NOTHING",
                (
                    root.id.to_string(),
                    graft_core::path_to_bytes(&root.path),
                    root.first_commit.as_str(),
                    root.registered_at.as_millis(),
                ),
            )
            .map_err(|e| store.sql(e))?;
        let existing = store.root()?;
        if existing.id != root.id {
            return Err(StoreError::RootMismatch {
                path: store.path.clone(),
                expected: root.id,
                found: existing.id,
            });
        }
        let registration = if inserted == 1 {
            Registration::Created
        } else {
            Registration::Existing
        };
        Ok((store, registration))
    }

    /// Opens the existing database in `root_dir`, migrating it if needed.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] with `NotFound` if there is no database,
    /// [`StoreError::SchemaTooNew`] if a newer Graft wrote it, plus SQLite
    /// errors.
    pub fn open(root_dir: &Path) -> Result<Self> {
        let path = Self::db_path(root_dir);
        if !path.is_file() {
            return Err(StoreError::Io {
                action: "opening",
                path,
                source: std::io::ErrorKind::NotFound.into(),
            });
        }
        let flags = OpenFlags::default().difference(OpenFlags::SQLITE_OPEN_CREATE);
        let conn =
            Connection::open_with_flags(&path, flags).map_err(|source| StoreError::Sqlite {
                path: path.clone(),
                source,
            })?;
        Self::configure(conn, path)
    }

    fn configure(conn: Connection, path: PathBuf) -> Result<Self> {
        let mut store = Self { conn, path };
        store
            .conn
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(|e| store.sql(e))?;
        store.enable_wal()?;
        store
            .conn
            .pragma_update(None, "foreign_keys", true)
            .map_err(|e| store.sql(e))?;
        // Safe with WAL: a crash may lose the last commits but never
        // corrupts the database.
        store
            .conn
            .pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| store.sql(e))?;
        match schema::migrate(&mut store.conn) {
            Ok(schema::Migrated::UpToDate) => {}
            Ok(schema::Migrated::Upgraded { from }) => {
                tracing::debug!(path = %store.path.display(), from, to = SCHEMA_VERSION, "migrated schema");
            }
            Err(schema::MigrateError::TooNew(found)) => {
                return Err(StoreError::SchemaTooNew {
                    path: store.path.clone(),
                    found,
                    supported: SCHEMA_VERSION,
                });
            }
            Err(schema::MigrateError::Sqlite(e)) => return Err(store.sql(e)),
        }
        Ok(store)
    }

    /// Switches to WAL. Changing the journal mode needs an exclusive lock and
    /// SQLite does not apply the busy timeout to it, so retry while another
    /// process holds the database.
    fn enable_wal(&self) -> Result<()> {
        let deadline = Instant::now() + BUSY_TIMEOUT;
        loop {
            let result: rusqlite::Result<String> =
                self.conn
                    .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0));
            match result {
                Ok(mode) if mode.eq_ignore_ascii_case("wal") => return Ok(()),
                Ok(mode) => {
                    return Err(StoreError::Corrupt {
                        path: self.path.clone(),
                        detail: format!("could not enable WAL; journal mode is {mode}"),
                    });
                }
                Err(e) if is_busy(&e) && Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => return Err(self.sql(e)),
            }
        }
    }

    fn sql(&self, source: rusqlite::Error) -> StoreError {
        StoreError::Sqlite {
            path: self.path.clone(),
            source,
        }
    }

    fn corrupt(&self, detail: impl Into<String>) -> StoreError {
        StoreError::Corrupt {
            path: self.path.clone(),
            detail: detail.into(),
        }
    }

    /// Path of the database file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The schema version of the open database.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn schema_version(&self) -> Result<u32> {
        schema::user_version(&self.conn).map_err(|e| self.sql(e))
    }

    /// Runs SQLite's `integrity_check` and returns the problems found; an
    /// empty list means the database is sound.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn integrity_check(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("PRAGMA integrity_check")
            .map_err(|e| self.sql(e))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| self.sql(e))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| self.sql(e))?;
        Ok(rows.into_iter().filter(|r| r != "ok").collect())
    }

    /// The Root this database belongs to.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotInitialised`] if no Root has been recorded yet.
    pub fn root(&self) -> Result<RootRecord> {
        let row = self
            .conn
            .query_row(
                "SELECT root_id, path, first_commit, registered_at FROM roots",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| self.sql(e))?;
        let Some((id, path, first_commit, registered_at)) = row else {
            return Err(StoreError::NotInitialised {
                path: self.path.clone(),
            });
        };
        Ok(RootRecord {
            id: id
                .parse()
                .map_err(|e| self.corrupt(format!("roots.root_id: {e}")))?,
            path: graft_core::path_from_bytes(path),
            first_commit: first_commit
                .parse()
                .map_err(|e| self.corrupt(format!("roots.first_commit: {e}")))?,
            registered_at: Timestamp::from_millis(registered_at),
        })
    }

    /// Records a new graft in state [`GraftState::Created`].
    ///
    /// # Errors
    ///
    /// [`StoreError::NameTaken`] if a graft that is not pruned already has
    /// `name`.
    pub fn create_graft(
        &mut self,
        name: &GraftName,
        base_commit: &CommitId,
    ) -> Result<GraftRecord> {
        let now = Timestamp::now();
        let record = GraftRecord {
            id: GraftId::random()?,
            name: name.clone(),
            base_commit: base_commit.clone(),
            state: GraftState::Created,
            created_at: now,
            updated_at: now,
        };
        let result = self.conn.execute(
            "INSERT INTO grafts (graft_id, name, base_commit, state, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                record.id.to_string(),
                record.name.as_str(),
                record.base_commit.as_str(),
                record.state.as_str(),
                now.as_millis(),
                now.as_millis(),
            ),
        );
        match result {
            Ok(_) => Ok(record),
            Err(e)
                if e.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation)
                    && self.graft(name)?.is_some() =>
            {
                Err(StoreError::NameTaken { name: name.clone() })
            }
            Err(e) => Err(self.sql(e)),
        }
    }

    const GRAFT_COLUMNS: &'static str =
        "graft_id, name, base_commit, state, created_at, updated_at";

    fn decode_graft(&self, row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<GraftRecord>> {
        let id: String = row.get(0)?;
        let name: String = row.get(1)?;
        let base: String = row.get(2)?;
        let state: String = row.get(3)?;
        let created_at: i64 = row.get(4)?;
        let updated_at: i64 = row.get(5)?;
        Ok((|| {
            Ok(GraftRecord {
                id: id
                    .parse()
                    .map_err(|e| self.corrupt(format!("grafts.graft_id: {e}")))?,
                name: name
                    .parse()
                    .map_err(|e| self.corrupt(format!("grafts.name: {e}")))?,
                base_commit: base
                    .parse()
                    .map_err(|e| self.corrupt(format!("grafts.base_commit: {e}")))?,
                state: state
                    .parse()
                    .map_err(|e| self.corrupt(format!("grafts.state: {e}")))?,
                created_at: Timestamp::from_millis(created_at),
                updated_at: Timestamp::from_millis(updated_at),
            })
        })())
    }

    fn query_grafts(
        &self,
        filter: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<GraftRecord>> {
        let sql = format!(
            "SELECT {} FROM grafts {filter} ORDER BY created_at, name",
            Self::GRAFT_COLUMNS
        );
        let mut stmt = self.conn.prepare(&sql).map_err(|e| self.sql(e))?;
        let rows = stmt
            .query_map(params, |row| self.decode_graft(row))
            .map_err(|e| self.sql(e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| self.sql(e))??);
        }
        Ok(out)
    }

    /// The graft named `name` that is not pruned, if any.
    ///
    /// # Errors
    ///
    /// SQLite errors and [`StoreError::Corrupt`].
    pub fn graft(&self, name: &GraftName) -> Result<Option<GraftRecord>> {
        Ok(self
            .query_grafts("WHERE name = ?1 AND state <> 'pruned'", [name.as_str()])?
            .pop())
    }

    /// The graft with `id`, in any state.
    ///
    /// # Errors
    ///
    /// SQLite errors and [`StoreError::Corrupt`].
    pub fn graft_by_id(&self, id: GraftId) -> Result<Option<GraftRecord>> {
        Ok(self
            .query_grafts("WHERE graft_id = ?1", [id.to_string()])?
            .pop())
    }

    /// Every graft, pruned ones included, oldest first.
    ///
    /// # Errors
    ///
    /// SQLite errors and [`StoreError::Corrupt`].
    pub fn grafts(&self) -> Result<Vec<GraftRecord>> {
        self.query_grafts("", [])
    }

    /// Moves a graft to `next`, enforcing [`GraftState::can_become`].
    ///
    /// # Errors
    ///
    /// [`StoreError::NoSuchGraft`], [`StoreError::InvalidTransition`], and
    /// [`StoreError::NameTaken`] when un-pruning is attempted.
    pub fn set_state(&mut self, id: GraftId, next: GraftState) -> Result<GraftRecord> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| StoreError::Sqlite {
                path: self.path.clone(),
                source,
            })?;
        let path = self.path.clone();
        let sql = |source| StoreError::Sqlite {
            path: path.clone(),
            source,
        };
        let current: Option<(String, String)> = tx
            .query_row(
                "SELECT name, state FROM grafts WHERE graft_id = ?1",
                [id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let Some((name, state)) = current else {
            return Err(StoreError::NoSuchGraft { id });
        };
        let corrupt = |detail: String| StoreError::Corrupt {
            path: path.clone(),
            detail,
        };
        let name: GraftName = name
            .parse()
            .map_err(|e| corrupt(format!("grafts.name: {e}")))?;
        let from: GraftState = state
            .parse()
            .map_err(|e| corrupt(format!("grafts.state: {e}")))?;
        if !from.can_become(next) {
            return Err(StoreError::InvalidTransition {
                name,
                from,
                to: next,
            });
        }
        tx.execute(
            "UPDATE grafts SET state = ?1, updated_at = ?2 WHERE graft_id = ?3",
            (next.as_str(), Timestamp::now().as_millis(), id.to_string()),
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        self.graft_by_id(id)?.ok_or(StoreError::NoSuchGraft { id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root_record(id_byte: u8) -> RootRecord {
        RootRecord {
            id: RootId::from_bytes([id_byte; 16]),
            path: PathBuf::from("/src/app"),
            first_commit: "a".repeat(40).parse().unwrap(),
            registered_at: Timestamp::from_millis(1_000),
        }
    }

    fn fresh() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let (store, reg) = Store::create(dir.path(), &root_record(1)).unwrap();
        assert_eq!(reg, Registration::Created);
        (dir, store)
    }

    fn name(s: &str) -> GraftName {
        s.parse().unwrap()
    }

    fn commit() -> CommitId {
        "b".repeat(40).parse().unwrap()
    }

    #[test]
    fn create_is_idempotent_and_returns_the_root() {
        let (dir, store) = fresh();
        assert_eq!(store.root().unwrap(), root_record(1));
        drop(store);
        let (again, reg) = Store::create(dir.path(), &root_record(1)).unwrap();
        assert_eq!(reg, Registration::Existing);
        assert_eq!(again.root().unwrap(), root_record(1));
    }

    #[test]
    fn create_refuses_a_different_root_in_the_same_directory() {
        let (dir, _store) = fresh();
        let err = Store::create(dir.path(), &root_record(2)).unwrap_err();
        assert!(matches!(err, StoreError::RootMismatch { .. }), "{err:?}");
    }

    #[test]
    fn open_requires_an_existing_database() {
        let dir = tempfile::tempdir().unwrap();
        let err = Store::open(dir.path()).unwrap_err();
        assert!(matches!(err, StoreError::Io { .. }), "{err:?}");
        assert!(!Store::db_path(dir.path()).exists(), "open must not create");
    }

    #[test]
    fn uses_wal_and_foreign_keys() {
        let (_dir, store) = fresh();
        let mode: String = store
            .conn
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        let fk: bool = store
            .conn
            .pragma_query_value(None, "foreign_keys", |r| r.get(0))
            .unwrap();
        assert!(fk);
    }

    #[cfg(unix)]
    #[test]
    fn root_directory_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root_dir = dir.path().join("roots/abc");
        Store::create(&root_dir, &root_record(1)).unwrap();
        for d in [dir.path().join("roots"), root_dir] {
            let mode = std::fs::metadata(&d).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{}", d.display());
        }
    }

    #[test]
    fn graft_lifecycle() {
        let (_dir, mut store) = fresh();
        let g = store.create_graft(&name("auth"), &commit()).unwrap();
        assert_eq!(g.state, GraftState::Created);
        assert_eq!(store.graft(&name("auth")).unwrap(), Some(g.clone()));
        assert_eq!(store.graft_by_id(g.id).unwrap(), Some(g.clone()));

        let live = store.set_state(g.id, GraftState::Live).unwrap();
        assert_eq!(live.state, GraftState::Live);
        assert!(live.updated_at >= g.updated_at);
        assert_eq!(live.created_at, g.created_at);

        let err = store.set_state(g.id, GraftState::Live).unwrap_err();
        assert!(
            matches!(err, StoreError::InvalidTransition { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn names_are_unique_among_unpruned_grafts() {
        let (_dir, mut store) = fresh();
        let first = store.create_graft(&name("auth"), &commit()).unwrap();
        let err = store.create_graft(&name("auth"), &commit()).unwrap_err();
        assert!(matches!(err, StoreError::NameTaken { .. }), "{err:?}");
        assert_eq!(err.to_string(), "a graft named auth already exists");

        // Once pruned, the name is free again; the pruned record remains.
        store.set_state(first.id, GraftState::Pruned).unwrap();
        assert_eq!(store.graft(&name("auth")).unwrap(), None);
        let second = store.create_graft(&name("auth"), &commit()).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(store.grafts().unwrap().len(), 2);

        let err = store.set_state(first.id, GraftState::Live).unwrap_err();
        assert!(
            matches!(err, StoreError::InvalidTransition { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn set_state_on_a_missing_graft() {
        let (_dir, mut store) = fresh();
        let id = GraftId::from_bytes([9; 16]);
        let err = store.set_state(id, GraftState::Live).unwrap_err();
        assert!(matches!(err, StoreError::NoSuchGraft { .. }), "{err:?}");
    }

    #[test]
    fn grafts_are_listed_oldest_first() {
        let (_dir, mut store) = fresh();
        for n in ["c", "a", "b"] {
            store.create_graft(&name(n), &commit()).unwrap();
        }
        let grafts = store.grafts().unwrap();
        let mut expected = grafts.clone();
        expected.sort_by(|a, b| (a.created_at, &a.name).cmp(&(b.created_at, &b.name)));
        assert_eq!(grafts, expected);
        assert_eq!(grafts.len(), 3);
    }

    #[test]
    fn database_constraints_back_up_the_types() {
        let (_dir, store) = fresh();
        let bad_state = store.conn.execute(
            "INSERT INTO grafts VALUES (?1, 'x', ?2, 'deleted', 0, 0)",
            ("0".repeat(32), "b".repeat(40)),
        );
        assert!(bad_state.is_err());
        let second_root = store.conn.execute(
            "INSERT INTO roots VALUES (2, ?1, x'00', ?2, 0)",
            ("0".repeat(32), "b".repeat(40)),
        );
        assert!(second_root.is_err());
    }

    #[test]
    fn integrity_check_passes() {
        let (_dir, store) = fresh();
        assert_eq!(store.integrity_check().unwrap(), Vec::<String>::new());
    }
}
