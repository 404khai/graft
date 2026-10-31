//! The shared error type.

use std::path::PathBuf;

use crate::id::ParseIdError;

/// Result alias using [`GraftError`].
pub type Result<T, E = GraftError> = std::result::Result<T, E>;

/// Errors produced by `graft-core`, and the base error type other library
/// crates wrap or extend.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GraftError {
    /// An I/O operation on a specific path failed.
    #[error("{action} {}: {source}", path.display())]
    Io {
        /// What was being attempted, e.g. `"reading"`.
        action: &'static str,
        /// The path involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },

    /// Neither `$GRAFT_HOME` nor a home directory is available, so there is
    /// nowhere to put Graft's data.
    #[error("cannot locate Graft's data directory: set $GRAFT_HOME or $HOME")]
    HomeNotFound,

    /// `$GRAFT_HOME` (or the home directory it is derived from) is not an
    /// absolute path.
    #[error("{var} must be an absolute path, got {}", path.display())]
    RelativeHome {
        /// The environment variable that held the path.
        var: &'static str,
        /// The rejected value.
        path: PathBuf,
    },

    /// `config.toml` exists but is not valid.
    #[error("invalid config file {}: {source}", path.display())]
    Config {
        /// The config file.
        path: PathBuf,
        /// The parse error, including line and column.
        #[source]
        source: Box<toml::de::Error>,
    },

    /// An identifier could not be parsed.
    #[error(transparent)]
    InvalidId(#[from] ParseIdError),
}

impl GraftError {
    /// Builds an [`GraftError::Io`] for `path`.
    pub fn io(action: &'static str, path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            action,
            path: path.into(),
            source,
        }
    }
}
