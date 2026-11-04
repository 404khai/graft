//! A graft's delta and the operations that change it (ADR 0008).
//!
//! The delta maps graft-relative paths (bytes, `/`-separated, no leading
//! slash) to [`Entry`] values that override the *lower* tree: the base
//! commit's tree plus the graft's layers, which never change while the graft
//! lives. [`Delta::apply`] is the only way a delta changes, so replaying the
//! op log through it reproduces the delta exactly.

use std::collections::{BTreeMap, BTreeSet};

use graft_core::{BlobHash, Timestamp};

/// A graft-relative path: bytes separated by `/`. The empty path is the
/// graft's top directory.
pub type RelPath = Vec<u8>;

/// Where a file's contents come from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Content {
    /// A blob in the delta blob store.
    Blob(BlobHash),
    /// Whatever the lower tree has at this path (the base tree or a layer).
    /// Used when a lower file is renamed or its mode changes, so its
    /// contents are never copied.
    Lower(RelPath),
}

/// What the delta says about one path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Entry {
    /// A regular file.
    File {
        /// Its contents.
        content: Content,
        /// Permission bits.
        mode: u32,
        /// Modification time in milliseconds, or `None` for the base time.
        mtime: Option<i64>,
    },
    /// A symbolic link.
    Symlink {
        /// Its target.
        target: Vec<u8>,
        /// Modification time in milliseconds, or `None` for the base time.
        mtime: Option<i64>,
    },
    /// A directory. An `opaque` directory hides everything the lower tree
    /// has below this path.
    Dir {
        /// Permission bits.
        mode: u32,
        /// Whether lower entries below are hidden.
        opaque: bool,
        /// Modification time in milliseconds, or `None` for the base time.
        mtime: Option<i64>,
    },
    /// The path, and everything the lower tree has below it, is deleted.
    Tombstone,
}

/// How a `write` op came about, for the op log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WriteKind {
    /// The file was created.
    Create,
    /// Its contents were written.
    Write,
    /// It was truncated.
    Truncate,
}

/// One operation in the op log.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Op {
    /// The file at `path` now has the contents of `blob` and `mode`.
    Write {
        /// What happened.
        kind: WriteKind,
        /// The file.
        path: RelPath,
        /// Its new contents.
        blob: BlobHash,
        /// Its permission bits.
        mode: u32,
    },
    /// A directory was created.
    Mkdir {
        /// The directory.
        path: RelPath,
        /// Its permission bits.
        mode: u32,
    },
    /// A symlink was created.
    Symlink {
        /// The link.
        path: RelPath,
        /// Its target.
        target: Vec<u8>,
    },
    /// A file or symlink was removed.
    Unlink {
        /// The path.
        path: RelPath,
    },
    /// An empty directory was removed.
    Rmdir {
        /// The path.
        path: RelPath,
    },
    /// `from` moved to `to`, replacing whatever was there.
    Rename {
        /// The source.
        from: RelPath,
        /// The destination.
        to: RelPath,
    },
    /// Permission bits changed.
    Chmod {
        /// The path.
        path: RelPath,
        /// The new bits.
        mode: u32,
    },
    /// The modification time was set explicitly.
    Touch {
        /// The path.
        path: RelPath,
        /// The new time, in milliseconds.
        mtime: i64,
    },
}

impl Op {
    /// The op log's name for this op.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Write {
                kind: WriteKind::Create,
                ..
            } => "create",
            Self::Write {
                kind: WriteKind::Write,
                ..
            } => "write",
            Self::Write {
                kind: WriteKind::Truncate,
                ..
            } => "truncate",
            Self::Mkdir { .. } => "mkdir",
            Self::Symlink { .. } => "symlink",
            Self::Unlink { .. } => "unlink",
            Self::Rmdir { .. } => "rmdir",
            Self::Rename { .. } => "rename",
            Self::Chmod { .. } => "chmod",
            Self::Touch { .. } => "touch",
        }
    }

    /// The path the op acts on (the source, for a rename).
    #[must_use]
    pub fn path(&self) -> &[u8] {
        match self {
            Self::Write { path, .. }
            | Self::Mkdir { path, .. }
            | Self::Symlink { path, .. }
            | Self::Unlink { path }
            | Self::Rmdir { path }
            | Self::Chmod { path, .. }
            | Self::Touch { path, .. } => path,
            Self::Rename { from, .. } => from,
        }
    }
}

/// One row of the op log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Per-graft sequence number, from 1.
    pub seq: u64,
    /// When it happened.
    pub at: Timestamp,
    /// What happened.
    pub op: Op,
    /// Who did it, if the caller said.
    pub actor: Option<String>,
}

/// What the lower tree has at a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerKind {
    /// A regular file with these permission bits.
    File {
        /// Permission bits.
        mode: u32,
    },
    /// A symbolic link.
    Symlink {
        /// Its target.
        target: Vec<u8>,
    },
    /// A directory with these permission bits.
    Dir {
        /// Permission bits.
        mode: u32,
    },
}

/// The immutable tree under a graft's delta.
pub trait Lower {
    /// What is at `path`, if anything.
    fn kind(&self, path: &[u8]) -> Option<LowerKind>;
    /// The names of the entries of the directory at `path`.
    fn children(&self, path: &[u8]) -> Vec<Vec<u8>>;
}

/// The effective state of a path: delta over lower.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A regular file.
    File {
        /// Its contents.
        content: Content,
        /// Permission bits.
        mode: u32,
        /// Modification time in milliseconds, or `None` for the base time.
        mtime: Option<i64>,
    },
    /// A symbolic link.
    Symlink {
        /// Its target.
        target: Vec<u8>,
        /// Modification time in milliseconds, or `None` for the base time.
        mtime: Option<i64>,
    },
    /// A directory.
    Dir {
        /// Permission bits.
        mode: u32,
        /// Modification time in milliseconds, or `None` for the base time.
        mtime: Option<i64>,
    },
}

impl Node {
    /// Whether this is a directory.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        matches!(self, Self::Dir { .. })
    }
}

/// One changed delta row: its path, its value before, and after (`None`
/// means no row).
pub type Change = (RelPath, Option<Entry>, Option<Entry>);

/// Joins a directory path and a name.
#[must_use]
pub fn join(dir: &[u8], name: &[u8]) -> RelPath {
    if dir.is_empty() {
        return name.to_vec();
    }
    let mut p = Vec::with_capacity(dir.len() + 1 + name.len());
    p.extend_from_slice(dir);
    p.push(b'/');
    p.extend_from_slice(name);
    p
}

/// The parent directory of `path` (the top for a top-level name).
#[must_use]
pub fn parent(path: &[u8]) -> &[u8] {
    path.iter()
        .rposition(|&b| b == b'/')
        .map_or(&[][..], |i| &path[..i])
}

/// The last component of `path`.
#[must_use]
pub fn file_name(path: &[u8]) -> &[u8] {
    path.iter()
        .rposition(|&b| b == b'/')
        .map_or(path, |i| &path[i + 1..])
}

/// Whether `path` is `dir` or below it.
#[must_use]
pub fn is_within(path: &[u8], dir: &[u8]) -> bool {
    dir.is_empty() || path == dir || (path.starts_with(dir) && path.get(dir.len()) == Some(&b'/'))
}

/// A graft's delta.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Delta {
    entries: BTreeMap<RelPath, Entry>,
}

impl Delta {
    /// A delta with these entries.
    #[must_use]
    pub fn from_entries(entries: impl IntoIterator<Item = (RelPath, Entry)>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    /// Every entry, in path order.
    pub fn entries(&self) -> impl Iterator<Item = (&RelPath, &Entry)> {
        self.entries.iter()
    }

    /// The entry for exactly `path`.
    #[must_use]
    pub fn get(&self, path: &[u8]) -> Option<&Entry> {
        self.entries.get(path)
    }

    /// Whether the delta has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries strictly below `dir`, in path order.
    fn below<'a>(&'a self, dir: &'a [u8]) -> impl Iterator<Item = (&'a RelPath, &'a Entry)> + 'a {
        let start = if dir.is_empty() {
            Vec::new()
        } else {
            let mut s = dir.to_vec();
            s.push(b'/');
            s
        };
        self.entries
            .range(start.clone()..)
            .take_while(move |(k, _)| k.starts_with(&start))
            .filter(move |(k, _)| !k.is_empty() && k.as_slice() != dir)
    }

    /// Whether the lower tree is visible at `path`: no ancestor in the delta
    /// is a tombstone, a non-directory or an opaque directory.
    #[must_use]
    pub fn lower_visible(&self, path: &[u8]) -> bool {
        let mut end = 0;
        while let Some(i) = path[end..].iter().position(|&b| b == b'/') {
            let ancestor = &path[..end + i];
            match self.entries.get(ancestor) {
                None | Some(Entry::Dir { opaque: false, .. }) => {}
                Some(_) => return false,
            }
            end += i + 1;
        }
        true
    }

    /// The effective node at `path`.
    #[must_use]
    pub fn resolve(&self, path: &[u8], lower: &dyn Lower) -> Option<Node> {
        if let Some(e) = self.entries.get(path) {
            return match e {
                Entry::Tombstone => None,
                Entry::File {
                    content,
                    mode,
                    mtime,
                } => Some(Node::File {
                    content: content.clone(),
                    mode: *mode,
                    mtime: *mtime,
                }),
                Entry::Symlink { target, mtime } => Some(Node::Symlink {
                    target: target.clone(),
                    mtime: *mtime,
                }),
                Entry::Dir { mode, mtime, .. } => Some(Node::Dir {
                    mode: *mode,
                    mtime: *mtime,
                }),
            };
        }
        if !self.lower_visible(path) {
            return None;
        }
        Some(match lower.kind(path)? {
            LowerKind::File { mode } => Node::File {
                content: Content::Lower(path.to_vec()),
                mode,
                mtime: None,
            },
            LowerKind::Symlink { target } => Node::Symlink {
                target,
                mtime: None,
            },
            LowerKind::Dir { mode } => Node::Dir { mode, mtime: None },
        })
    }

    /// The names in directory `dir`, sorted. Assumes `dir` resolves to a
    /// directory.
    #[must_use]
    pub fn children(&self, dir: &[u8], lower: &dyn Lower) -> Vec<Vec<u8>> {
        let mut names: BTreeSet<Vec<u8>> = BTreeSet::new();
        let opaque = matches!(self.entries.get(dir), Some(Entry::Dir { opaque: true, .. }));
        if !opaque && self.lower_visible(dir) {
            names.extend(lower.children(dir));
        }
        for (path, entry) in self.below(dir) {
            let rest = &path[if dir.is_empty() { 0 } else { dir.len() + 1 }..];
            if rest.contains(&b'/') {
                continue;
            }
            if *entry == Entry::Tombstone {
                names.remove(rest);
            } else {
                names.insert(rest.to_vec());
            }
        }
        names.into_iter().collect()
    }

    fn set(&mut self, changes: &mut Vec<Change>, path: &[u8], value: Option<Entry>) {
        let old = match &value {
            Some(v) => self.entries.insert(path.to_vec(), v.clone()),
            None => self.entries.remove(path),
        };
        if old != value {
            changes.push((path.to_vec(), old, value));
        }
    }

    /// Removes `path` and everything below it from the delta, then marks it
    /// deleted if the lower tree still shows something there.
    fn remove_tree(&mut self, changes: &mut Vec<Change>, path: &[u8], lower: &dyn Lower) {
        let below: Vec<RelPath> = self.below(path).map(|(k, _)| k.clone()).collect();
        for k in below {
            self.set(changes, &k, None);
        }
        let shadowed = self.lower_visible(path) && lower.kind(path).is_some();
        self.set(changes, path, shadowed.then_some(Entry::Tombstone));
    }

    /// The delta entry that recreates `node` at another path.
    fn entry_for(node: Node) -> Entry {
        match node {
            Node::File {
                content,
                mode,
                mtime,
            } => Entry::File {
                content,
                mode,
                mtime,
            },
            Node::Symlink { target, mtime } => Entry::Symlink { target, mtime },
            Node::Dir { mode, mtime } => Entry::Dir {
                mode,
                opaque: true,
                mtime,
            },
        }
    }

    /// Every effective node at and below `path`, as (path relative to
    /// `path`, node), parents before children.
    fn subtree(&self, path: &[u8], lower: &dyn Lower) -> Vec<(RelPath, Node)> {
        let mut out = Vec::new();
        let Some(node) = self.resolve(path, lower) else {
            return out;
        };
        let mut stack = vec![(Vec::new(), node)];
        while let Some((rel, node)) = stack.pop() {
            if node.is_dir() {
                let abs = join(path, &rel);
                let abs = if rel.is_empty() { path.to_vec() } else { abs };
                for name in self.children(&abs, lower).into_iter().rev() {
                    if let Some(child) = self.resolve(&join(&abs, &name), lower) {
                        stack.push((join(&rel, &name), child));
                    }
                }
            }
            out.push((rel, node));
        }
        out
    }

    /// The entry for `path` with a new mode and/or mtime, keeping its
    /// contents. Symlinks have no mode of their own, so chmod leaves them.
    fn restat(
        &self,
        path: &[u8],
        lower: &dyn Lower,
        mode: Option<u32>,
        mtime: Option<i64>,
    ) -> Option<Entry> {
        let opaque = matches!(
            self.entries.get(path),
            Some(Entry::Dir { opaque: true, .. })
        );
        match self.resolve(path, lower)? {
            Node::File {
                content,
                mode: m,
                mtime: t,
            } => Some(Entry::File {
                content,
                mode: mode.unwrap_or(m),
                mtime: mtime.or(t),
            }),
            Node::Symlink { target, mtime: t } => mtime.map(|m| Entry::Symlink {
                target,
                mtime: Some(m).or(t),
            }),
            Node::Dir { mode: m, mtime: t } => Some(Entry::Dir {
                mode: mode.unwrap_or(m),
                opaque,
                mtime: mtime.or(t),
            }),
        }
    }

    /// Applies `op`, done at time `at`, and returns the changed rows. The
    /// caller has checked that the op is valid for the effective tree
    /// (parents exist, targets are of the right kind, and so on).
    pub fn apply(&mut self, op: &Op, at: Timestamp, lower: &dyn Lower) -> Vec<Change> {
        let mut changes = Vec::new();
        let now = Some(at.as_millis());
        match op {
            Op::Write {
                path, blob, mode, ..
            } => self.set(
                &mut changes,
                path,
                Some(Entry::File {
                    content: Content::Blob(*blob),
                    mode: *mode,
                    mtime: now,
                }),
            ),
            Op::Mkdir { path, mode } => self.set(
                &mut changes,
                path,
                Some(Entry::Dir {
                    mode: *mode,
                    opaque: true,
                    mtime: now,
                }),
            ),
            Op::Symlink { path, target } => self.set(
                &mut changes,
                path,
                Some(Entry::Symlink {
                    target: target.clone(),
                    mtime: now,
                }),
            ),
            Op::Unlink { path } | Op::Rmdir { path } => {
                self.remove_tree(&mut changes, path, lower);
            }
            Op::Chmod { path, mode } => {
                if let Some(e) = self.restat(path, lower, Some(*mode), None) {
                    self.set(&mut changes, path, Some(e));
                }
            }
            Op::Touch { path, mtime } => {
                if let Some(e) = self.restat(path, lower, None, Some(*mtime)) {
                    self.set(&mut changes, path, Some(e));
                }
            }
            Op::Rename { from, to } => {
                if from == to {
                    return changes;
                }
                let moved = self.subtree(from, lower);
                if moved.is_empty() {
                    return changes;
                }
                self.remove_tree(&mut changes, to, lower);
                self.remove_tree(&mut changes, from, lower);
                for (rel, node) in moved {
                    let dest = if rel.is_empty() {
                        to.clone()
                    } else {
                        join(to, &rel)
                    };
                    self.set(&mut changes, &dest, Some(Self::entry_for(node)));
                }
            }
        }
        changes
    }

    /// Undoes `changes` (from [`Delta::apply`]), newest first.
    pub fn revert(&mut self, changes: &[Change]) {
        for (path, old, _) in changes.iter().rev() {
            match old {
                Some(e) => {
                    self.entries.insert(path.clone(), e.clone());
                }
                None => {
                    self.entries.remove(path);
                }
            }
        }
    }

    /// Replays `records` from an empty delta.
    #[must_use]
    pub fn replay<'a>(records: impl IntoIterator<Item = &'a Record>, lower: &dyn Lower) -> Self {
        let mut d = Self::default();
        for r in records {
            d.apply(&r.op, r.at, lower);
        }
        d
    }

    /// Every delta blob the entries reference.
    pub fn blobs(&self) -> impl Iterator<Item = BlobHash> + '_ {
        self.entries.values().filter_map(|e| match e {
            Entry::File {
                content: Content::Blob(h),
                ..
            } => Some(*h),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lower tree held in memory.
    #[derive(Default)]
    struct Mem(BTreeMap<RelPath, LowerKind>);

    impl Mem {
        fn with(paths: &[(&str, LowerKind)]) -> Self {
            let mut m = Self::default();
            m.0.insert(Vec::new(), LowerKind::Dir { mode: 0o755 });
            for (p, k) in paths {
                m.0.insert(p.as_bytes().to_vec(), k.clone());
            }
            m
        }
    }

    impl Lower for Mem {
        fn kind(&self, path: &[u8]) -> Option<LowerKind> {
            self.0.get(path).cloned()
        }
        fn children(&self, path: &[u8]) -> Vec<Vec<u8>> {
            self.0
                .keys()
                .filter(|k| !k.is_empty() && parent(k) == path && k.as_slice() != path)
                .map(|k| file_name(k).to_vec())
                .collect()
        }
    }

    const F: LowerKind = LowerKind::File { mode: 0o644 };
    const D: LowerKind = LowerKind::Dir { mode: 0o755 };

    fn p(s: &str) -> RelPath {
        s.as_bytes().to_vec()
    }

    fn names(v: Vec<Vec<u8>>) -> Vec<String> {
        v.into_iter()
            .map(|n| String::from_utf8(n).unwrap())
            .collect()
    }

    fn at() -> Timestamp {
        Timestamp::from_millis(1000)
    }

    #[test]
    fn path_helpers() {
        assert_eq!(join(b"", b"a"), b"a");
        assert_eq!(join(b"a", b"b"), b"a/b");
        assert_eq!(parent(b"a/b/c"), b"a/b");
        assert_eq!(parent(b"a"), b"");
        assert_eq!(file_name(b"a/b/c"), b"c");
        assert!(is_within(b"a/b", b"a"));
        assert!(is_within(b"a", b"a"));
        assert!(!is_within(b"ab", b"a"));
        assert!(is_within(b"x", b""));
    }

    #[test]
    fn unlink_of_a_lower_file_leaves_a_tombstone() {
        let lower = Mem::with(&[("a", F), ("d", D), ("d/x", F)]);
        let mut d = Delta::default();
        d.apply(&Op::Unlink { path: p("a") }, at(), &lower);
        assert_eq!(d.get(b"a"), Some(&Entry::Tombstone));
        assert_eq!(d.resolve(b"a", &lower), None);
        assert_eq!(names(d.children(b"", &lower)), ["d"]);
        // A file that only the delta had leaves nothing behind.
        let blob = BlobHash::from_bytes([1; 32]);
        d.apply(
            &Op::Write {
                kind: WriteKind::Create,
                path: p("n"),
                blob,
                mode: 0o644,
            },
            at(),
            &lower,
        );
        d.apply(&Op::Unlink { path: p("n") }, at(), &lower);
        assert_eq!(d.get(b"n"), None);
    }

    #[test]
    fn a_recreated_directory_hides_the_lower_one() {
        let lower = Mem::with(&[("d", D), ("d/x", F)]);
        let mut d = Delta::default();
        d.apply(&Op::Unlink { path: p("d/x") }, at(), &lower);
        d.apply(&Op::Rmdir { path: p("d") }, at(), &lower);
        assert_eq!(d.entries().count(), 1, "only the tombstone for d");
        d.apply(
            &Op::Mkdir {
                path: p("d"),
                mode: 0o700,
            },
            at(),
            &lower,
        );
        assert_eq!(d.children(b"d", &lower), Vec::<Vec<u8>>::new());
        assert_eq!(d.resolve(b"d/x", &lower), None);
    }

    #[test]
    fn renaming_a_lower_directory_moves_references_not_contents() {
        let lower = Mem::with(&[("src", D), ("src/a", F), ("src/sub", D), ("src/sub/b", F)]);
        let mut d = Delta::default();
        d.apply(
            &Op::Rename {
                from: p("src"),
                to: p("lib"),
            },
            at(),
            &lower,
        );
        assert_eq!(d.resolve(b"src", &lower), None);
        assert_eq!(names(d.children(b"lib", &lower)), ["a", "sub"]);
        assert_eq!(
            d.resolve(b"lib/sub/b", &lower),
            Some(Node::File {
                content: Content::Lower(p("src/sub/b")),
                mode: 0o644,
                mtime: None
            })
        );
        // Renaming back restores the same effective tree.
        d.apply(
            &Op::Rename {
                from: p("lib"),
                to: p("src"),
            },
            at(),
            &lower,
        );
        assert_eq!(names(d.children(b"src", &lower)), ["a", "sub"]);
        assert_eq!(d.resolve(b"lib", &lower), None);
    }

    #[test]
    fn chmod_and_touch_keep_contents() {
        let lower = Mem::with(&[("a", F), ("d", D), ("d/x", F)]);
        let mut d = Delta::default();
        d.apply(
            &Op::Chmod {
                path: p("a"),
                mode: 0o755,
            },
            at(),
            &lower,
        );
        d.apply(
            &Op::Touch {
                path: p("a"),
                mtime: 5,
            },
            at(),
            &lower,
        );
        assert_eq!(
            d.resolve(b"a", &lower),
            Some(Node::File {
                content: Content::Lower(p("a")),
                mode: 0o755,
                mtime: Some(5)
            })
        );
        // chmod of a lower directory does not hide its contents.
        d.apply(
            &Op::Chmod {
                path: p("d"),
                mode: 0o700,
            },
            at(),
            &lower,
        );
        assert_eq!(names(d.children(b"d", &lower)), ["x"]);
    }

    #[test]
    fn revert_undoes_apply() {
        let lower = Mem::with(&[("src", D), ("src/a", F), ("b", F)]);
        let mut d = Delta::default();
        d.apply(&Op::Unlink { path: p("b") }, at(), &lower);
        let before = d.clone();
        let changes = d.apply(
            &Op::Rename {
                from: p("src"),
                to: p("b"),
            },
            at(),
            &lower,
        );
        assert_ne!(d, before);
        d.revert(&changes);
        assert_eq!(d, before);
    }
}
