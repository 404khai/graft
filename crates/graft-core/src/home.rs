//! Resolution of `$GRAFT_HOME`.
//!
//! `$GRAFT_HOME` is the only directory Graft owns outside a repository. It
//! defaults to the platform data directory:
//!
//! | Platform | Default |
//! | --- | --- |
//! | Linux and other Unix | `$XDG_DATA_HOME/graft`, else `~/.local/share/graft` |
//! | macOS | `~/.graft`, or `~/Library/Application Support/graft` if that exists |
//!
//! Until ADR 0037, the macOS default was `~/Library/Application
//! Support/graft`, whose space every graft path inherited. A home already
//! there keeps being used, so no graft moves; new ones start in `~/.graft`.
//!
//! Resolution only computes a path; it never creates directories.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::config::CONFIG_FILE_NAME;
use crate::error::{GraftError, Result};
use crate::id::RootId;

/// Environment variable that overrides the default location.
pub const GRAFT_HOME_ENV: &str = "GRAFT_HOME";

/// Where a [`GraftHome`] path came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeSource {
    /// Set explicitly through `$GRAFT_HOME`.
    Env,
    /// Derived from the platform data directory.
    PlatformDefault,
    /// The macOS default before ADR 0037 (`~/Library/Application
    /// Support/graft`), used because it exists.
    LegacyDefault,
}

/// The resolved `$GRAFT_HOME` directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraftHome {
    path: PathBuf,
    source: HomeSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    MacOs,
    Xdg,
}

impl Platform {
    const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Xdg
        }
    }
}

impl GraftHome {
    /// Resolves `$GRAFT_HOME` from the process environment.
    ///
    /// # Errors
    ///
    /// Returns [`GraftError::RelativeHome`] if `$GRAFT_HOME` or `$HOME` is
    /// set to a relative path, and [`GraftError::HomeNotFound`] if neither is
    /// set.
    pub fn resolve() -> Result<Self> {
        Self::resolve_with(|key| std::env::var_os(key))
    }

    /// Resolves `$GRAFT_HOME` using `env` to look up environment variables.
    ///
    /// This is [`GraftHome::resolve`] with the environment injected, so
    /// callers and tests never need to mutate the process environment.
    ///
    /// # Errors
    ///
    /// As for [`GraftHome::resolve`].
    pub fn resolve_with(env: impl Fn(&str) -> Option<OsString>) -> Result<Self> {
        resolve_for(Platform::current(), env, Path::is_dir)
    }

    /// Uses `path` as `$GRAFT_HOME` directly.
    ///
    /// # Errors
    ///
    /// Returns [`GraftError::RelativeHome`] if `path` is not absolute.
    pub fn from_path(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if !path.is_absolute() {
            return Err(GraftError::RelativeHome {
                var: GRAFT_HOME_ENV,
                path,
            });
        }
        Ok(Self {
            path,
            source: HomeSource::Env,
        })
    }

    /// The `$GRAFT_HOME` directory. It may not exist yet.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where the path came from.
    #[must_use]
    pub fn source(&self) -> HomeSource {
        self.source
    }

    /// `$GRAFT_HOME/config.toml`.
    #[must_use]
    pub fn config_file(&self) -> PathBuf {
        self.path.join(CONFIG_FILE_NAME)
    }

    /// `$GRAFT_HOME/roots`, the parent of every per-Root directory.
    #[must_use]
    pub fn roots_dir(&self) -> PathBuf {
        self.path.join("roots")
    }

    /// `$GRAFT_HOME/roots/<root-id>`.
    #[must_use]
    pub fn root_dir(&self, root: &RootId) -> PathBuf {
        self.roots_dir().join(root.to_string())
    }
}

fn non_empty(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// `is_dir` says whether a directory exists, for the macOS legacy home.
fn resolve_for(
    platform: Platform,
    env: impl Fn(&str) -> Option<OsString>,
    is_dir: impl Fn(&Path) -> bool,
) -> Result<GraftHome> {
    if let Some(path) = non_empty(env(GRAFT_HOME_ENV)) {
        return GraftHome::from_path(path);
    }

    if platform == Platform::Xdg {
        // The XDG spec says relative values must be ignored, not rejected.
        if let Some(data) = non_empty(env("XDG_DATA_HOME")).filter(|p| p.is_absolute()) {
            return Ok(GraftHome {
                path: data.join("graft"),
                source: HomeSource::PlatformDefault,
            });
        }
    }

    let home = non_empty(env("HOME")).ok_or(GraftError::HomeNotFound)?;
    if !home.is_absolute() {
        return Err(GraftError::RelativeHome {
            var: "HOME",
            path: home,
        });
    }
    let path = match platform {
        Platform::MacOs => {
            let legacy = home.join("Library/Application Support/graft");
            if is_dir(&legacy) {
                return Ok(GraftHome {
                    path: legacy,
                    source: HomeSource::LegacyDefault,
                });
            }
            home.join(".graft")
        }
        Platform::Xdg => home.join(".local/share/graft"),
    };
    Ok(GraftHome {
        path,
        source: HomeSource::PlatformDefault,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |key| {
            vars.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn graft_home_env_wins() {
        let vars = [("GRAFT_HOME", "/srv/graft"), ("HOME", "/home/u")];
        for platform in [Platform::MacOs, Platform::Xdg] {
            let home = resolve_for(platform, env(&vars), |_| false).unwrap();
            assert_eq!(home.path(), Path::new("/srv/graft"));
            assert_eq!(home.source(), HomeSource::Env);
        }
    }

    #[test]
    fn relative_graft_home_is_rejected() {
        let vars = [("GRAFT_HOME", "graft"), ("HOME", "/home/u")];
        let err = resolve_for(Platform::Xdg, env(&vars), |_| false).unwrap_err();
        assert!(matches!(
            err,
            GraftError::RelativeHome {
                var: "GRAFT_HOME",
                ..
            }
        ));
    }

    #[test]
    fn empty_graft_home_is_treated_as_unset() {
        let vars = [("GRAFT_HOME", ""), ("HOME", "/home/u")];
        let home = resolve_for(Platform::Xdg, env(&vars), |_| false).unwrap();
        assert_eq!(home.path(), Path::new("/home/u/.local/share/graft"));
        assert_eq!(home.source(), HomeSource::PlatformDefault);
    }

    #[test]
    fn linux_default_uses_xdg_data_home() {
        let vars = [("XDG_DATA_HOME", "/data"), ("HOME", "/home/u")];
        let home = resolve_for(Platform::Xdg, env(&vars), |_| false).unwrap();
        assert_eq!(home.path(), Path::new("/data/graft"));
    }

    #[test]
    fn linux_ignores_relative_xdg_data_home() {
        let vars = [("XDG_DATA_HOME", "data"), ("HOME", "/home/u")];
        let home = resolve_for(Platform::Xdg, env(&vars), |_| false).unwrap();
        assert_eq!(home.path(), Path::new("/home/u/.local/share/graft"));
    }

    #[test]
    fn macos_default_ignores_xdg_and_has_no_space() {
        let vars = [("XDG_DATA_HOME", "/data"), ("HOME", "/Users/u")];
        let home = resolve_for(Platform::MacOs, env(&vars), |_| false).unwrap();
        assert_eq!(home.path(), Path::new("/Users/u/.graft"));
        assert_eq!(home.source(), HomeSource::PlatformDefault);
    }

    #[test]
    fn macos_keeps_an_existing_application_support_home() {
        let vars = [("HOME", "/Users/u")];
        let legacy = Path::new("/Users/u/Library/Application Support/graft");
        let home = resolve_for(Platform::MacOs, env(&vars), |p| p == legacy).unwrap();
        assert_eq!(home.path(), legacy);
        assert_eq!(home.source(), HomeSource::LegacyDefault);
        // $GRAFT_HOME still wins over it.
        let vars = [("HOME", "/Users/u"), ("GRAFT_HOME", "/g")];
        let home = resolve_for(Platform::MacOs, env(&vars), |p| p == legacy).unwrap();
        assert_eq!(home.path(), Path::new("/g"));
    }

    #[test]
    fn linux_ignores_an_application_support_dir() {
        let vars = [("HOME", "/home/u")];
        let home = resolve_for(Platform::Xdg, env(&vars), |_| true).unwrap();
        assert_eq!(home.path(), Path::new("/home/u/.local/share/graft"));
    }

    #[test]
    fn missing_home_is_an_error() {
        let err = resolve_for(Platform::Xdg, env(&[]), |_| false).unwrap_err();
        assert!(matches!(err, GraftError::HomeNotFound));
    }

    #[test]
    fn derived_paths() {
        let home = GraftHome::from_path("/g").unwrap();
        let root: RootId = "0123456789abcdef0123456789abcdef".parse().unwrap();
        assert_eq!(home.config_file(), Path::new("/g/config.toml"));
        assert_eq!(home.roots_dir(), Path::new("/g/roots"));
        assert_eq!(
            home.root_dir(&root),
            Path::new("/g/roots/0123456789abcdef0123456789abcdef")
        );
    }
}
