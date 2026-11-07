//! Schema migrations.
//!
//! The schema version is SQLite's `PRAGMA user_version`. Migration `n`
//! (1-based) takes the database from version `n - 1` to `n`. Migrations run
//! inside one `BEGIN IMMEDIATE` transaction, so concurrent processes opening
//! a fresh database serialise on it and the loser sees the finished schema.
//! They run with foreign keys off, so a migration may rebuild a table that
//! others reference (SQLite's documented procedure); `foreign_key_check`
//! must pass before the transaction commits.

use rusqlite::{Connection, TransactionBehavior};

/// Every migration, in order. Never edit or reorder a released entry.
pub(crate) const MIGRATIONS: &[&str] = &[
    include_str!("schema/v1.sql"),
    include_str!("schema/v2.sql"),
    include_str!("schema/v3.sql"),
    include_str!("schema/v4.sql"),
    include_str!("schema/v5.sql"),
    include_str!("schema/v6.sql"),
    include_str!("schema/v7.sql"),
];

/// The schema version this build creates and understands.
pub const SCHEMA_VERSION: u32 = {
    assert!(MIGRATIONS.len() < u32::MAX as usize);
    #[allow(clippy::cast_possible_truncation)] // checked just above
    let v = MIGRATIONS.len() as u32;
    v
};

/// What [`migrate`] found and did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Migrated {
    /// Already current.
    UpToDate,
    /// Applied migrations from `from` to [`SCHEMA_VERSION`].
    Upgraded { from: u32 },
}

/// Why [`migrate`] did nothing useful.
pub(crate) enum MigrateError {
    Sqlite(rusqlite::Error),
    TooNew(u32),
}

impl From<rusqlite::Error> for MigrateError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

pub(crate) fn user_version(conn: &Connection) -> rusqlite::Result<u32> {
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
}

fn migrate_tx(conn: &mut Connection) -> Result<u32, MigrateError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Another process may have migrated while we waited for the lock.
    let from = user_version(&tx)?;
    if from > SCHEMA_VERSION {
        return Err(MigrateError::TooNew(from));
    }
    for (version, sql) in (1..=SCHEMA_VERSION).zip(MIGRATIONS).skip(from as usize) {
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
    }
    let violations: i64 =
        tx.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })?;
    if violations > 0 {
        return Err(MigrateError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY),
            Some(format!(
                "migration left {violations} foreign key violation(s)"
            )),
        )));
    }
    tx.commit()?;
    Ok(from)
}

pub(crate) fn migrate(conn: &mut Connection) -> Result<Migrated, MigrateError> {
    // Fast path without taking the write lock.
    let version = user_version(conn)?;
    if version == SCHEMA_VERSION {
        return Ok(Migrated::UpToDate);
    }
    if version > SCHEMA_VERSION {
        return Err(MigrateError::TooNew(version));
    }

    // The pragma has no effect inside a transaction, so set it around one.
    let fk: bool = conn.pragma_query_value(None, "foreign_keys", |r| r.get(0))?;
    conn.pragma_update(None, "foreign_keys", false)?;
    let result = migrate_tx(conn);
    let restored = conn.pragma_update(None, "foreign_keys", fk);
    let from = result?;
    restored?;
    Ok(if from == SCHEMA_VERSION {
        Migrated::UpToDate
    } else {
        Migrated::Upgraded { from }
    })
}
