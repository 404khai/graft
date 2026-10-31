//! Store errors.

use std::path::PathBuf;

use graft_core::{GraftId, GraftName, GraftState, RootId};

/// Errors from the metadata store.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// SQLite reported an error for the database at `path`.
    #[error("database {}: {source}", path.display())]
    Sqlite {
        /// The database file.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: rusqlite::Error,
    },

    /// A filesystem operation failed.
    #[error("{action} {}: {source}", path.display())]
    Io {
        /// What was being attempted.
        action: &'static str,
        /// The path involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },

    /// The database was written by a newer Graft.
    #[error(
        "database {} has schema v{found}, but this graft supports up to v{supported}; upgrade graft",
        path.display()
    )]
    SchemaTooNew {
        /// The database file.
        path: PathBuf,
        /// The schema version found.
        found: u32,
        /// The newest version this build understands.
        supported: u32,
    },

    /// The database exists but has no Root registered in it.
    #[error("database {} has no Root registered", path.display())]
    NotInitialised {
        /// The database file.
        path: PathBuf,
    },

    /// The database directory belongs to a different Root.
    #[error("database {} belongs to Root {found}, expected {expected}", path.display())]
    RootMismatch {
        /// The database file.
        path: PathBuf,
        /// The Root that was being registered or opened.
        expected: RootId,
        /// The Root recorded in the database.
        found: RootId,
    },

    /// The repository has not been registered with `graft init`.
    #[error("{} is not registered as a Root; run `graft init` first", path.display())]
    NotRegistered {
        /// The repository's working tree.
        path: PathBuf,
    },

    /// A live graft already uses this name.
    #[error("a graft named {name} already exists")]
    NameTaken {
        /// The contested name.
        name: GraftName,
    },

    /// No graft has this ID.
    #[error("no graft with id {id}")]
    NoSuchGraft {
        /// The missing ID.
        id: GraftId,
    },

    /// The requested state change is not allowed.
    #[error("graft {name} cannot change from {from} to {to}")]
    InvalidTransition {
        /// The graft.
        name: GraftName,
        /// Its current state.
        from: GraftState,
        /// The requested state.
        to: GraftState,
    },

    /// A row contains a value this build cannot decode.
    #[error("database {} is corrupt: {detail}", path.display())]
    Corrupt {
        /// The database file.
        path: PathBuf,
        /// What was wrong.
        detail: String,
    },

    /// An error from `graft-core`, such as failing to generate an ID.
    #[error(transparent)]
    Core(#[from] graft_core::GraftError),

    /// An error reading the repository.
    #[error(transparent)]
    Root(#[from] graft_root::RootError),
}

/// Result alias using [`StoreError`].
pub type Result<T, E = StoreError> = std::result::Result<T, E>;
