//! Which filesystem backend materialises a graft.

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;

/// How a graft's files are provided (ADR 0006). Recorded per graft, so
/// every later command treats the graft the way it was created.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    /// A real working tree whose unchanged files are reflinks of Root's
    /// (ADR 0003).
    #[default]
    Reflink,
    /// A virtual tree served by `graftd` over FUSE from Root's object store
    /// (Linux).
    Fuse,
}

impl BackendKind {
    /// Every backend.
    pub const ALL: [Self; 2] = [Self::Reflink, Self::Fuse];

    /// Environment variable selecting the backend for `graft new`.
    pub const ENV: &'static str = "GRAFT_FS";

    /// The persisted, lowercase form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reflink => "reflink",
            Self::Fuse => "fuse",
        }
    }

    /// Whether grafts on this backend are served by `graftd`.
    #[must_use]
    pub const fn is_virtual(self) -> bool {
        matches!(self, Self::Fuse)
    }
}

impl fmt::Display for BackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when parsing an unknown [`BackendKind`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown filesystem backend {0:?}; expected \"reflink\" or \"fuse\"")]
pub struct UnknownBackend(pub String);

impl FromStr for BackendKind {
    type Err = UnknownBackend;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|b| b.as_str() == s)
            .ok_or_else(|| UnknownBackend(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for b in BackendKind::ALL {
            assert_eq!(b.as_str().parse::<BackendKind>().unwrap(), b);
        }
        assert!("nfs".parse::<BackendKind>().is_err());
        assert_eq!(BackendKind::default(), BackendKind::Reflink);
    }
}
