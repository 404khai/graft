//! Admission: may a graft change these symbols of a file? (ADR 0018)

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A live graft's claim.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Claim {
    /// The graft holding it.
    pub graft: String,
    /// The file.
    pub path: PathBuf,
    /// The symbol, or `None` for the whole file.
    pub symbol: Option<String>,
    /// The plan and position that created it, if a plan did.
    pub plan: Option<(String, u32)>,
}

/// The answer to a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// No other live graft claims what the write changes.
    Allowed,
    /// Claimed by a graft earlier in the same plan: refused until it lands.
    Queued {
        /// The holder.
        holder: String,
        /// What it claims.
        claim: String,
    },
    /// Claimed by another live graft.
    Rejected {
        /// The holder.
        holder: String,
        /// What it claims.
        claim: String,
    },
}

impl Verdict {
    /// The verdict as one line, naming the holder.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Allowed => "allowed".to_owned(),
            Self::Queued { holder, claim } => {
                format!("queued: {claim} is claimed by graft {holder}, which lands first")
            }
            Self::Rejected { holder, claim } => {
                format!("rejected: {claim} is claimed by graft {holder}")
            }
        }
    }
}

/// Whether changed symbol `changed` falls under claimed symbol `claimed`
/// (equal, or a member of it: `K.m` under `K`, `S::m` under `S`).
fn covers(claimed: &str, changed: &str) -> bool {
    changed == claimed
        || changed
            .strip_prefix(claimed)
            .is_some_and(|rest| rest.starts_with("::") || rest.starts_with('.'))
}

/// Admits a write by `graft` (at `plan` position, if any) that changes
/// `changed` symbols of `path` (`None`: unknown, so the whole file), against
/// `claims` of other live grafts. A refusal names the first conflicting
/// holder; `Rejected` wins over `Queued`.
#[must_use]
pub fn admit(
    graft: &str,
    plan: Option<&(String, u32)>,
    path: &Path,
    changed: Option<&BTreeSet<String>>,
    claims: &[Claim],
) -> Verdict {
    let mut conflicts: Vec<&Claim> = claims
        .iter()
        .filter(|c| c.graft != graft && c.path == path)
        .filter(|c| match (&c.symbol, changed) {
            (None, _) | (_, None) => true,
            (Some(s), Some(changed)) => changed.iter().any(|x| covers(s, x)),
        })
        .collect();
    conflicts.sort();
    let describe = |c: &Claim| match &c.symbol {
        Some(s) => format!("{}::{s}", c.path.display()),
        None => c.path.display().to_string(),
    };
    let ahead = |c: &Claim| match (plan, &c.plan) {
        (Some((p, mine)), Some((q, theirs))) => p == q && theirs < mine,
        _ => false,
    };
    if let Some(c) = conflicts.iter().find(|c| !ahead(c)) {
        return Verdict::Rejected {
            holder: c.graft.clone(),
            claim: describe(c),
        };
    }
    conflicts
        .first()
        .map_or(Verdict::Allowed, |c| Verdict::Queued {
            holder: c.graft.clone(),
            claim: describe(c),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(graft: &str, path: &str, symbol: Option<&str>, plan: Option<u32>) -> Claim {
        Claim {
            graft: graft.into(),
            path: path.into(),
            symbol: symbol.map(Into::into),
            plan: plan.map(|o| ("p".to_owned(), o)),
        }
    }

    fn set(xs: &[&str]) -> BTreeSet<String> {
        xs.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn verdicts() {
        let claims = [
            claim("a", "m.py", Some("total"), Some(0)),
            claim("b", "m.py", Some("K"), None),
            claim("c", "whole.py", None, None),
        ];
        let p = Path::new("m.py");
        assert_eq!(
            admit("x", None, p, Some(&set(&["other"])), &claims),
            Verdict::Allowed
        );
        assert_eq!(
            admit("x", None, p, Some(&set(&["total"])), &claims).describe(),
            "rejected: m.py::total is claimed by graft a"
        );
        assert_eq!(
            admit(
                "x",
                Some(&("p".into(), 2)),
                p,
                Some(&set(&["total"])),
                &claims
            ),
            Verdict::Queued {
                holder: "a".into(),
                claim: "m.py::total".into()
            }
        );
        assert!(matches!(
            admit("x", None, p, Some(&set(&["K.m"])), &claims),
            Verdict::Rejected { holder, .. } if holder == "b"
        ));
        assert!(matches!(
            admit("x", None, Path::new("whole.py"), Some(&set(&[])), &claims),
            Verdict::Rejected { .. }
        ));
        assert_eq!(
            admit("a", None, p, Some(&set(&["total"])), &claims[..1]),
            Verdict::Allowed,
            "own claim"
        );
        assert!(
            matches!(admit("x", None, p, None, &claims), Verdict::Rejected { .. }),
            "unknown symbols"
        );
    }
}
