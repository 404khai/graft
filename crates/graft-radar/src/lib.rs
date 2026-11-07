//! Radar: overlapping work across live grafts (ADR 0016).
//!
//! [`overlaps`] takes each live graft's changes against its base, as `graft
//! diff` computes them for any backend, and reports every pair of grafts
//! that changed the same file: at [`Level::Symbol`] when they changed the
//! same function, type or export ([`symbols`], with tree-sitter), otherwise
//! at [`Level::Path`]. The behaviour level (speculative merges with a test
//! command) is run by the CLI and reported with [`Level::Conflict`] and
//! [`Level::Behaviour`].
//!
//! Nothing here runs on the write path.

#![forbid(unsafe_code)]

pub mod symbols;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// How serious an overlap is, least first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Both grafts changed the file, but no symbol in common.
    Path,
    /// Both grafts changed the same symbol.
    Symbol,
    /// A three-way merge of the file fails.
    Conflict,
    /// Merged, the test command fails, though each graft passes alone.
    Behaviour,
}

impl Level {
    /// The name shown in tables and JSON.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Symbol => "symbol",
            Self::Conflict => "conflict",
            Self::Behaviour => "behaviour",
        }
    }
}

/// One changed file of a graft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// Path relative to the graft's top level.
    pub path: PathBuf,
    /// The base version (`None` if added).
    pub old: Option<Vec<u8>>,
    /// The current version (`None` if deleted).
    pub new: Option<Vec<u8>>,
}

/// One live graft's changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraftChanges {
    /// The graft's name.
    pub graft: String,
    /// Its base commit.
    pub base: String,
    /// What it changed.
    pub files: Vec<FileChange>,
}

/// Two grafts changed the same file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Overlap {
    /// How serious it is.
    pub level: Level,
    /// The grafts, in name order.
    pub grafts: [String; 2],
    /// The file.
    pub path: PathBuf,
    /// The symbols both changed (empty at path level).
    pub symbols: Vec<String>,
}

/// Each changed file's symbols, or `None` if Radar cannot name them.
fn symbol_sets(g: &GraftChanges) -> BTreeMap<&PathBuf, Option<BTreeSet<String>>> {
    g.files
        .iter()
        .map(|f| {
            (
                &f.path,
                symbols::changed_symbols(&f.path, f.old.as_deref(), f.new.as_deref()),
            )
        })
        .collect()
}

/// Every overlap between `grafts`, by file, highest level first.
#[must_use]
pub fn overlaps(grafts: &[GraftChanges]) -> Vec<Overlap> {
    let mut sorted: Vec<&GraftChanges> = grafts.iter().collect();
    sorted.sort_by(|a, b| a.graft.cmp(&b.graft));
    // Symbols are only computed for files another graft also changed.
    let mut by_path: BTreeMap<&PathBuf, usize> = BTreeMap::new();
    for g in &sorted {
        for f in &g.files {
            *by_path.entry(&f.path).or_default() += 1;
        }
    }
    let shared = |g: &GraftChanges| GraftChanges {
        files: g
            .files
            .iter()
            .filter(|f| by_path.get(&f.path).copied().unwrap_or(0) > 1)
            .cloned()
            .collect(),
        ..g.clone()
    };
    let trimmed: Vec<GraftChanges> = sorted.iter().map(|g| shared(g)).collect();
    let sets: Vec<_> = trimmed.iter().map(symbol_sets).collect();
    let mut out = Vec::new();
    for i in 0..trimmed.len() {
        for j in i + 1..trimmed.len() {
            for (path, si) in &sets[i] {
                let Some(sj) = sets[j].get(path) else {
                    continue;
                };
                let common: Vec<String> = match (si, sj) {
                    (Some(a), Some(b)) => a.intersection(b).cloned().collect(),
                    _ => Vec::new(),
                };
                out.push(Overlap {
                    level: if common.is_empty() {
                        Level::Path
                    } else {
                        Level::Symbol
                    },
                    grafts: [trimmed[i].graft.clone(), trimmed[j].graft.clone()],
                    path: (*path).clone(),
                    symbols: common,
                });
            }
        }
    }
    out.sort_by(|a, b| {
        b.level
            .cmp(&a.level)
            .then_with(|| a.grafts.cmp(&b.grafts))
            .then_with(|| a.path.cmp(&b.path))
    });
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    const BASE: &str = "fn one() { 1 }\n\nfn two() { 2 }\n";

    fn graft(name: &str, files: &[(&str, &str)]) -> GraftChanges {
        GraftChanges {
            graft: name.into(),
            base: "b".into(),
            files: files
                .iter()
                .map(|(p, new)| FileChange {
                    path: (*p).into(),
                    old: Some(BASE.into()),
                    new: Some((*new).into()),
                })
                .collect(),
        }
    }

    #[test]
    fn same_function_is_symbol_level_different_functions_path_level() {
        let a = graft(
            "a",
            &[
                ("lib.rs", "fn one() { 10 }\n\nfn two() { 2 }\n"),
                ("only-a.rs", "x"),
            ],
        );
        let b = graft("b", &[("lib.rs", "fn one() { 11 }\n\nfn two() { 2 }\n")]);
        let c = graft("c", &[("lib.rs", "fn one() { 1 }\n\nfn two() { 20 }\n")]);
        let found = overlaps(&[c.clone(), b.clone(), a.clone()]);
        let summary: Vec<_> = found
            .iter()
            .map(|o| (o.level, o.grafts.join("+"), o.symbols.join(",")))
            .collect();
        assert_eq!(
            summary,
            [
                (Level::Symbol, "a+b".to_owned(), "one".to_owned()),
                (Level::Path, "a+c".to_owned(), String::new()),
                (Level::Path, "b+c".to_owned(), String::new()),
            ]
        );
        assert_eq!(overlaps(&[a, graft("d", &[("other.rs", "y")])]), []);
    }

    #[test]
    fn files_radar_cannot_parse_overlap_at_path_level() {
        let a = graft("a", &[("README.md", "a")]);
        let b = graft("b", &[("README.md", "b")]);
        let found = overlaps(&[a, b]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].level, Level::Path);
    }
}
