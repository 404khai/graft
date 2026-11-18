//! Request IDs: one per `graft` invocation, carried into graftd's log.
//!
//! The CLI takes the ID from [`REQUEST_ID_ENV`] when a caller (the MCP
//! server, a script) sets it, and otherwise makes a random one. It is
//! logged by the CLI, sent with every JSON-RPC request to graftd, and
//! logged there, so one grep ties a command to what the daemon did for it.
//! It is never persisted.

use std::fmt::{self, Write as _};
use std::str::FromStr;

/// Environment variable a caller sets to choose the request ID.
pub const REQUEST_ID_ENV: &str = "GRAFT_REQUEST_ID";

/// Longest accepted request ID.
pub const MAX_REQUEST_ID_LEN: usize = 64;

/// A request ID: 1 to [`MAX_REQUEST_ID_LEN`] ASCII letters, digits, `-`,
/// `_` or `.`, so it is safe in a log line or a file name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RequestId(String);

/// Why a string is not a [`RequestId`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid request ID {0:?}: use 1 to 64 ASCII letters, digits, '-', '_' or '.'")]
pub struct InvalidRequestId(pub String);

impl RequestId {
    /// A new random ID (16 hex characters).
    ///
    /// # Errors
    ///
    /// Returns [`GraftError::Entropy`] if the OS cannot provide randomness.
    ///
    /// [`GraftError::Entropy`]: crate::GraftError::Entropy
    pub fn random() -> crate::Result<Self> {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).map_err(|e| crate::GraftError::Entropy(e.to_string()))?;
        let hex = bytes.iter().fold(String::with_capacity(16), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        });
        Ok(Self(hex))
    }

    /// The ID used when the OS has no randomness to make one.
    #[must_use]
    pub fn unknown() -> Self {
        Self("unknown".to_owned())
    }

    /// The ID's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for RequestId {
    type Err = InvalidRequestId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let valid = !s.is_empty()
            && s.len() <= MAX_REQUEST_ID_LEN
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(InvalidRequestId(s.to_owned()))
        }
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_ids_are_hex_and_differ() {
        let a = RequestId::random().unwrap();
        let b = RequestId::random().unwrap();
        assert_eq!(a.as_str().len(), 16);
        assert!(a.as_str().bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert_eq!(a.as_str().parse::<RequestId>().unwrap(), a);
    }

    #[test]
    fn parsing_accepts_safe_ids_only() {
        for ok in ["a", "mcp-12.call_3", &"x".repeat(64)] {
            assert!(ok.parse::<RequestId>().is_ok(), "{ok}");
        }
        for bad in ["", "a b", "a\nb", "a/b", "é", &"x".repeat(65)] {
            assert!(bad.parse::<RequestId>().is_err(), "{bad:?}");
        }
    }
}
