//! User configuration in `$GRAFT_HOME/config.toml`.
//!
//! The file is optional; a missing file means every setting takes its
//! default. Unknown keys are rejected so a typo never silently does nothing.
//!
//! ```toml
//! [log]
//! # A tracing filter directive. `$GRAFT_LOG` overrides it.
//! filter = "graft=debug"
//!
//! [fs]
//! # Backend for new grafts: "reflink", "fuse" (Linux) or "nfs" (macOS). `$GRAFT_FS` and
//! # `graft new --backend` override it.
//! backend = "reflink"
//!
//! [git]
//! # Register new grafts as git worktrees of Root, so `git worktree list`,
//! # editors and git clients list them (ADR 0036). Off by default.
//! worktrees = true
//!
//! [telemetry]
//! # Where opted-in usage events are sent (ADR 0034). There is no default:
//! # without it, nothing is ever sent, even after `graft telemetry on`.
//! endpoint = "https://telemetry.example.com/v1/events"
//! ```

use std::io::ErrorKind;
use std::path::Path;

use serde::Deserialize;

use crate::backend::BackendKind;
use crate::error::{GraftError, Result};
use crate::home::GraftHome;

/// File name of the config file inside `$GRAFT_HOME`.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Parsed contents of `config.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Logging settings.
    pub log: LogConfig,
    /// Filesystem backend settings.
    pub fs: FsConfig,
    /// How grafts appear to git.
    pub git: GitConfig,
    /// Opt-in telemetry settings.
    pub telemetry: TelemetryConfig,
}

/// The `[git]` table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GitConfig {
    /// Register new grafts as linked worktrees of Root (ADR 0036). Grafts
    /// that exist already keep their layout.
    pub worktrees: bool,
}

/// The `[telemetry]` table. Sending also needs the user's opt-in
/// (`graft telemetry on`); this only says where to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TelemetryConfig {
    /// URL that receives batches of events. `https`, or `http` to a
    /// loopback address. `None`: nothing is sent.
    pub endpoint: Option<String>,
}

/// The `[fs]` table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FsConfig {
    /// Backend for new grafts. `None` means [`BackendKind::default`].
    pub backend: Option<BackendKind>,
}

/// The `[log]` table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LogConfig {
    /// A `tracing` filter directive such as `"info"` or `"graft_fs=debug"`.
    /// `None` uses the binary's default.
    pub filter: Option<String>,
}

impl Config {
    /// Loads `$GRAFT_HOME/config.toml`, or the defaults if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`GraftError::Io`] if the file exists but cannot be read and
    /// [`GraftError::Config`] if it is not valid.
    pub fn load(home: &GraftHome) -> Result<Self> {
        Ok(Self::load_file(&home.config_file())?.unwrap_or_default())
    }

    /// Loads the config file at `path`, returning `None` if it does not exist.
    ///
    /// # Errors
    ///
    /// As for [`Config::load`].
    pub fn load_file(path: &Path) -> Result<Option<Self>> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(GraftError::io("reading", path, e)),
        };
        Self::parse(&text)
            .map(Some)
            .map_err(|source| GraftError::Config {
                path: path.to_owned(),
                source: Box::new(source),
            })
    }

    /// Parses config text.
    ///
    /// # Errors
    ///
    /// Returns the TOML error, which carries line and column.
    pub fn parse(text: &str) -> std::result::Result<Self, toml::de::Error> {
        toml::from_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let home = GraftHome::from_path(dir.path()).unwrap();
        assert_eq!(Config::load(&home).unwrap(), Config::default());
        assert_eq!(Config::load_file(&home.config_file()).unwrap(), None);
    }

    #[test]
    fn reads_fs_backend() {
        let config = Config::parse("[fs]\nbackend = \"fuse\"\n").unwrap();
        assert_eq!(config.fs.backend, Some(BackendKind::Fuse));
        assert!(Config::parse("[fs]\nbackend = \"zfs\"\n").is_err());
    }

    #[test]
    fn reads_git_worktrees() {
        assert!(!Config::default().git.worktrees);
        let config = Config::parse("[git]\nworktrees = true\n").unwrap();
        assert!(config.git.worktrees);
        assert!(Config::parse("[git]\nworktree = true\n").is_err());
        assert!(Config::parse("[git]\nworktrees = \"yes\"\n").is_err());
    }

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn reads_log_filter() {
        let dir = tempfile::tempdir().unwrap();
        let home = GraftHome::from_path(dir.path()).unwrap();
        std::fs::write(home.config_file(), "[log]\nfilter = \"debug\"\n").unwrap();
        let config = Config::load(&home).unwrap();
        assert_eq!(config.log.filter.as_deref(), Some("debug"));
    }

    #[test]
    fn unknown_keys_are_rejected_with_path_and_line() {
        let dir = tempfile::tempdir().unwrap();
        let home = GraftHome::from_path(dir.path()).unwrap();
        std::fs::write(
            home.config_file(),
            "[log]\nfilter = \"x\"\nfliter = \"y\"\n",
        )
        .unwrap();
        let err = Config::load(&home).unwrap_err();
        let GraftError::Config { path, source } = &err else {
            panic!("expected a config error, got {err:?}");
        };
        assert_eq!(path, &home.config_file());
        assert!(source.to_string().contains("fliter"), "{source}");
        assert!(err.to_string().contains("line 3"), "{err}");
    }

    #[test]
    fn unreadable_path_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should be cannot be read as a string.
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::create_dir(&path).unwrap();
        let err = Config::load_file(&path).unwrap_err();
        assert!(matches!(err, GraftError::Io { .. }), "{err:?}");
    }
}
