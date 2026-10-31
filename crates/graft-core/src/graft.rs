//! Vocabulary for grafts themselves: names and lifecycle states.

use std::fmt;
use std::str::FromStr;

/// Maximum length of a [`GraftName`], in characters.
pub const MAX_GRAFT_NAME_LEN: usize = 40;

/// Error returned when a graft name is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid graft name {name:?}: {reason}")]
pub struct InvalidGraftName {
    /// The rejected name.
    pub name: String,
    /// Why it was rejected.
    pub reason: &'static str,
}

/// A graft's human-chosen name, unique among the grafts of one Root.
///
/// Names are 1 to 40 characters of lowercase ASCII letters, digits and
/// hyphens. They must start with a letter or digit, so a name is never
/// mistaken for a command-line flag, and must not end with a hyphen. Names
/// become directory names (`mounts/<name>/`), and this charset is safe on
/// every filesystem Graft supports.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GraftName(String);

impl GraftName {
    /// The name as a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for GraftName {
    type Err = InvalidGraftName;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = |reason| InvalidGraftName {
            name: s.to_owned(),
            reason,
        };
        let allowed = |b: u8| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-');
        match s.as_bytes() {
            [] => Err(err("must not be empty")),
            b if b.len() > MAX_GRAFT_NAME_LEN => Err(err("must be at most 40 characters")),
            b if !b.iter().copied().all(allowed) => Err(err(
                "may only contain lowercase letters, digits and hyphens",
            )),
            [b'-', ..] => Err(err("must start with a letter or digit")),
            [.., b'-'] => Err(err("must not end with a hyphen")),
            _ => Ok(Self(s.to_owned())),
        }
    }
}

impl TryFrom<String> for GraftName {
    type Error = InvalidGraftName;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<GraftName> for String {
    fn from(name: GraftName) -> Self {
        name.0
    }
}

impl fmt::Display for GraftName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for GraftName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GraftName({})", self.0)
    }
}

/// Where a graft is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GraftState {
    /// Recorded, but not yet mounted.
    Created,
    /// Mounted, with an open delta.
    Live,
    /// Its delta has landed in Root.
    Integrated,
    /// Removed; its references are released. Terminal.
    Pruned,
}

impl GraftState {
    /// Every state, in lifecycle order.
    pub const ALL: [Self; 4] = [Self::Created, Self::Live, Self::Integrated, Self::Pruned];

    /// The persisted, lowercase form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Live => "live",
            Self::Integrated => "integrated",
            Self::Pruned => "pruned",
        }
    }

    /// Whether a graft may move from `self` to `next`. Every move is allowed
    /// except out of [`GraftState::Pruned`] and to the same state.
    /// (Integrate may return a graft to `Live` when its test gate fails.)
    #[must_use]
    pub fn can_become(self, next: Self) -> bool {
        self != Self::Pruned && self != next
    }
}

impl fmt::Display for GraftState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when parsing an unknown [`GraftState`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown graft state {0:?}")]
pub struct UnknownGraftState(pub String);

impl FromStr for GraftState {
    type Err = UnknownGraftState;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|state| state.as_str() == s)
            .ok_or_else(|| UnknownGraftState(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(name: &str) -> &'static str {
        name.parse::<GraftName>().unwrap_err().reason
    }

    #[test]
    fn accepts_valid_names() {
        for name in ["a", "auth", "fix-login-2", "0day", &"a".repeat(40)] {
            assert_eq!(name.parse::<GraftName>().unwrap().as_str(), name);
        }
    }

    #[test]
    fn rejects_invalid_names() {
        assert_eq!(reason(""), "must not be empty");
        assert_eq!(reason(&"a".repeat(41)), "must be at most 40 characters");
        for bad in ["Auth", "a_b", "a b", "a.b", "a/b", "..", "é"] {
            assert!(reason(bad).starts_with("may only contain"), "{bad}");
        }
        assert_eq!(reason("-a"), "must start with a letter or digit");
        assert_eq!(reason("a-"), "must not end with a hyphen");
        assert_eq!(reason("-"), "must start with a letter or digit");
    }

    #[test]
    fn length_limit_counts_bytes_of_ascii_only_names() {
        // Non-ASCII is rejected by the charset rule, so bytes == characters.
        assert!(reason(&"é".repeat(20)).starts_with("may only contain"));
    }

    #[test]
    fn state_round_trips() {
        for state in GraftState::ALL {
            assert_eq!(state.as_str().parse::<GraftState>().unwrap(), state);
        }
        assert!("deleted".parse::<GraftState>().is_err());
    }

    #[test]
    fn pruned_is_terminal() {
        for next in GraftState::ALL {
            assert!(!GraftState::Pruned.can_become(next));
        }
        assert!(GraftState::Created.can_become(GraftState::Live));
        assert!(GraftState::Integrated.can_become(GraftState::Live));
        assert!(!GraftState::Live.can_become(GraftState::Live));
    }
}
