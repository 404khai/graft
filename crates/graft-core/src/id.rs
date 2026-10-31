//! Fixed-size identifiers with a canonical lowercase-hex text form.
//!
//! Every identifier that is persisted (in `graft.db`, in directory names, in
//! IPC messages) is one of these newtypes rather than a bare string. The text
//! form is always lowercase hex of exactly `2 * N` characters; parsing rejects
//! anything else, including uppercase, so one value has exactly one spelling.

use std::fmt;
use std::str::FromStr;

/// Error returned when parsing an identifier from text fails.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {kind} {value:?}: {reason}")]
pub struct ParseIdError {
    /// Which identifier was being parsed, e.g. `"graft id"`.
    pub kind: &'static str,
    /// The rejected input.
    pub value: String,
    /// Why it was rejected.
    pub reason: String,
}

fn decode_hex<const N: usize>(kind: &'static str, s: &str) -> Result<[u8; N], ParseIdError> {
    let err = |reason: String| ParseIdError {
        kind,
        value: s.to_owned(),
        reason,
    };
    let bytes = s.as_bytes();
    if bytes.len() != N * 2 {
        return Err(err(format!("expected {} lowercase hex characters", N * 2)));
    }
    let mut out = [0u8; N];
    let (pairs, _) = bytes.as_chunks::<2>();
    for (slot, &[a, b]) in out.iter_mut().zip(pairs) {
        let (Some(hi), Some(lo)) = (nibble(a), nibble(b)) else {
            return Err(err("contains a non-lowercase-hex character".to_owned()));
        };
        *slot = (hi << 4) | lo;
    }
    Ok(out)
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

fn write_hex(f: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    for b in bytes {
        write!(f, "{b:02x}")?;
    }
    Ok(())
}

macro_rules! hex_id {
    ($(#[$meta:meta])* $name:ident, $len:literal, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[derive(serde::Serialize, serde::Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name([u8; $len]);

        impl $name {
            /// Length of the identifier in bytes.
            pub const LEN: usize = $len;

            /// Wraps raw bytes.
            #[must_use]
            pub const fn from_bytes(bytes: [u8; $len]) -> Self {
                Self(bytes)
            }

            /// Returns the raw bytes.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; $len] {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write_hex(f, &self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self)
            }
        }

        impl FromStr for $name {
            type Err = ParseIdError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                decode_hex::<$len>($kind, s).map(Self)
            }
        }

        impl TryFrom<String> for $name {
            type Error = ParseIdError;

            fn try_from(s: String) -> Result<Self, Self::Error> {
                s.parse()
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> Self {
                id.to_string()
            }
        }
    };
}

hex_id!(
    /// Identifies one graft. Stable for the graft's lifetime, unlike its name.
    GraftId,
    16,
    "graft id"
);

hex_id!(
    /// Identifies one Root. Names the Root's directory under
    /// `$GRAFT_HOME/roots/`.
    RootId,
    16,
    "root id"
);

hex_id!(
    /// The `blake3` hash of a blob in the delta blob store or layer cache.
    BlobHash,
    32,
    "blob hash"
);

hex_id!(
    /// Identifies a layer in the layer cache: a hash of everything that
    /// determines its contents. Names `layers/<key>/`.
    LayerKey,
    32,
    "layer key"
);

hex_id!(
    /// Identifies the exact commands of a layer, for the build trust gate.
    CommandHash,
    32,
    "command hash"
);

impl GraftId {
    /// A new random ID from the operating system's secure random source.
    ///
    /// # Errors
    ///
    /// Returns [`GraftError::Entropy`] if the OS cannot provide randomness.
    ///
    /// [`GraftError::Entropy`]: crate::GraftError::Entropy
    pub fn random() -> crate::Result<Self> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| crate::GraftError::Entropy(e.to_string()))?;
        Ok(Self(bytes))
    }
}

impl BlobHash {
    /// Splits the hex form into the two-character shard directory and the
    /// remaining file name, as laid out under `blobs/`.
    #[must_use]
    pub fn shard(&self) -> (String, String) {
        let mut hex = self.to_string();
        let rest = hex.split_off(2);
        (hex, rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRAFT_HEX: &str = "00112233445566778899aabbccddeeff";

    #[test]
    fn graft_id_round_trips_through_text() {
        let id: GraftId = GRAFT_HEX.parse().unwrap();
        assert_eq!(id.as_bytes()[0], 0x00);
        assert_eq!(id.as_bytes()[15], 0xff);
        assert_eq!(id.to_string(), GRAFT_HEX);
        assert_eq!(format!("{id:?}"), format!("GraftId({GRAFT_HEX})"));
    }

    #[test]
    fn random_graft_ids_differ() {
        let a = GraftId::random().unwrap();
        let b = GraftId::random().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.to_string().parse::<GraftId>().unwrap(), a);
    }

    #[test]
    fn blob_hash_round_trips_and_shards() {
        let hex = "ab".repeat(32);
        let hash: BlobHash = hex.parse().unwrap();
        assert_eq!(hash.to_string(), hex);
        let (dir, file) = hash.shard();
        assert_eq!(dir, "ab");
        assert_eq!(file.len(), 62);
        assert_eq!(format!("{dir}{file}"), hex);
    }

    #[test]
    fn rejects_wrong_length() {
        let err = "abc".parse::<RootId>().unwrap_err();
        assert_eq!(err.kind, "root id");
        assert!(err.to_string().contains("32 lowercase hex"));
        assert!("ab".repeat(31).parse::<BlobHash>().is_err());
        assert!("ab".repeat(33).parse::<BlobHash>().is_err());
    }

    #[test]
    fn rejects_uppercase_and_non_hex() {
        assert!(GRAFT_HEX.to_uppercase().parse::<GraftId>().is_err());
        assert!(
            "zz112233445566778899aabbccddeeff"
                .parse::<GraftId>()
                .is_err()
        );
        // Multi-byte UTF-8 of the right byte length must not slip through.
        assert!("é".repeat(16).parse::<GraftId>().is_err());
    }

    #[test]
    fn serde_uses_the_hex_form() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Row {
            id: GraftId,
        }
        let row = Row {
            id: GRAFT_HEX.parse().unwrap(),
        };
        let text = toml::to_string(&row).unwrap();
        assert_eq!(text.trim(), format!("id = \"{GRAFT_HEX}\""));
        assert_eq!(toml::from_str::<Row>(&text).unwrap(), row);
        assert!(toml::from_str::<Row>("id = \"nope\"").is_err());
    }
}
