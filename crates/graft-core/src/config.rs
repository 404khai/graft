//! User configuration in `$GRAFT_HOME/config.toml`.
//!
//! The file is optional; a missing file means every setting takes its
//! default. Unknown keys are rejected so a typo never silently does nothing.
//!
//! ```toml
//! [log]
//! # A tracing filter directive. `$GRAFT_LOG` overrides it.
//! filter = "graft=debug"
//! ```

use std::io::ErrorKind;
use std::path::Path;

use serde::Deserialize;

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
