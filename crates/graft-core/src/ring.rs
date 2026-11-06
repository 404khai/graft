//! Ring labels: the names of a graft's checkpoints.

use std::fmt;
use std::str::FromStr;

use crate::GraftName;

/// Error returned when a ring label is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid ring label {label:?}: {reason}")]
pub struct InvalidRingLabel {
    /// The rejected label.
    pub label: String,
    /// Why it was rejected.
    pub reason: &'static str,
}

/// The label of a ring, unique among one graft's rings. Labels follow the
/// same rules as [`GraftName`]: 1 to 40 lowercase letters, digits and
/// hyphens, starting with a letter or digit and not ending with a hyphen.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RingLabel(String);

impl RingLabel {
    /// The label as a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The default label of a ring taken at op log sequence number `seq`.
    #[must_use]
    pub fn at_seq(seq: u64) -> Self {
        Self(format!("ring-{seq}"))
    }

    /// The label of the ring taken automatically before a rewind, when the
    /// last op was `seq`.
    #[must_use]
    pub fn undo(seq: u64) -> Self {
        Self(format!("undo-{seq}"))
    }
}

impl FromStr for RingLabel {
    type Err = InvalidRingLabel;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<GraftName>()
            .map(|n| Self(n.as_str().to_owned()))
            .map_err(|e| InvalidRingLabel {
                label: e.name,
                reason: e.reason,
            })
    }
}

impl TryFrom<String> for RingLabel {
    type Error = InvalidRingLabel;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<RingLabel> for String {
    fn from(label: RingLabel) -> Self {
        label.0
    }
}

impl fmt::Display for RingLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for RingLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_graft_name_rules() {
        assert_eq!(
            "before-refactor".parse::<RingLabel>().unwrap().as_str(),
            "before-refactor"
        );
        for bad in ["", "-x", "x-", "Upper", "a b", &"x".repeat(41)] {
            assert!(bad.parse::<RingLabel>().is_err(), "{bad:?}");
        }
        assert_eq!(RingLabel::at_seq(7).as_str(), "ring-7");
        assert_eq!(RingLabel::undo(u64::MAX).as_str().len(), 25);
        assert!(
            RingLabel::undo(u64::MAX)
                .as_str()
                .parse::<RingLabel>()
                .is_ok()
        );
    }
}
