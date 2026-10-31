//! Git commit identifiers.

use std::fmt;
use std::str::FromStr;

use crate::id::ParseIdError;

/// A git commit object ID in lowercase hex: 40 characters for SHA-1
/// repositories, 64 for SHA-256.
///
/// `graft-core` does not depend on `gix`; this type is how commit IDs cross
/// crate boundaries and get persisted.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CommitId(String);

impl CommitId {
    /// The hex form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The first `len` hex characters, for display. Never longer than the ID.
    #[must_use]
    pub fn short(&self, len: usize) -> &str {
        &self.0[..len.min(self.0.len())]
    }
}

impl FromStr for CommitId {
    type Err = ParseIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = |reason: &str| ParseIdError {
            kind: "commit id",
            value: s.to_owned(),
            reason: reason.to_owned(),
        };
        if s.len() != 40 && s.len() != 64 {
            return Err(err(
                "expected 40 (SHA-1) or 64 (SHA-256) lowercase hex characters",
            ));
        }
        if !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return Err(err("contains a non-lowercase-hex character"));
        }
        Ok(Self(s.to_owned()))
    }
}

impl TryFrom<String> for CommitId {
    type Error = ParseIdError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<CommitId> for String {
    fn from(id: CommitId) -> Self {
        id.0
    }
}

impl fmt::Display for CommitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for CommitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CommitId({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_sha1_and_sha256() {
        let sha1 = "a".repeat(40);
        let sha256 = "b".repeat(64);
        assert_eq!(sha1.parse::<CommitId>().unwrap().as_str(), sha1);
        assert_eq!(sha256.parse::<CommitId>().unwrap().to_string(), sha256);
    }

    #[test]
    fn rejects_other_lengths_and_uppercase() {
        assert!("a".repeat(39).parse::<CommitId>().is_err());
        assert!("a".repeat(41).parse::<CommitId>().is_err());
        assert!("A".repeat(40).parse::<CommitId>().is_err());
        assert!("g".repeat(40).parse::<CommitId>().is_err());
    }

    #[test]
    fn short_form() {
        let id: CommitId = "0123456789".repeat(4).parse().unwrap();
        assert_eq!(id.short(7), "0123456");
        assert_eq!(id.short(100).len(), 40);
    }
}
