//! Property tests for the delta and op log (AGENTS.md §6):
//!
//! - the overlay (delta over lower) always equals a plain in-memory
//!   filesystem that received the same operations;
//! - replaying the stored op log from empty reproduces the stored delta.

// Test helpers outside #[test] fns: panicking on setup failure is the point.
// The interpreter's raw choices are clearer as short names.
#![allow(clippy::unwrap_used, clippy::many_single_char_names)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use graft_core::{BackendKind, BlobHash, RootId, Timestamp};
use graft_store::delta::{
    Content, Delta, Lower, LowerKind, Node, Op, RelPath, WriteKind, file_name, is_within, join,
    parent,
};
use graft_store::{Journaled, RootRecord, Store};
use proptest::prelude::*;

/// What the plain model holds at a path.
#[derive(Debug, Clone, PartialEq, Eq)]
enum M {
    File { content: Content, mode: u32 },
    Link(Vec<u8>),
    Dir { mode: u32 },
}

/// The lower tree: a few directories, files and a symlink.
struct Base(BTreeMap<RelPath, LowerKind>);

fn base() -> Base {
    let mut m = BTreeMap::new();
    let mut add = |p: &str, k| {
        m.insert(p.as_bytes().to_vec(), k);
    };
    add("", LowerKind::Dir { mode: 0o755 });
    for d in ["a", "a/b", "c"] {
        add(d, LowerKind::Dir { mode: 0o755 });
    }
    for f in ["a/x", "a/b/y", "c/z", "top"] {
        add(f, LowerKind::File { mode: 0o644 });
    }
    add(
        "a/link",
        LowerKind::Symlink {
            target: b"x".to_vec(),
        },
    );
    Base(m)
}

impl Lower for Base {
    fn kind(&self, path: &[u8]) -> Option<LowerKind> {
        self.0.get(path).cloned()
    }
    fn children(&self, path: &[u8]) -> Vec<Vec<u8>> {
        self.0
            .keys()
            .filter(|k| !k.is_empty() && parent(k) == path)
            .map(|k| file_name(k).to_vec())
            .collect()
    }
}

fn model_of(base: &Base) -> BTreeMap<RelPath, M> {
    base.0
        .iter()
        .map(|(p, k)| {
            let m = match k {
                LowerKind::File { mode } => M::File {
                    content: Content::Lower(p.clone()),
                    mode: *mode,
                },
                LowerKind::Symlink { target } => M::Link(target.clone()),
                LowerKind::Dir { mode } => M::Dir { mode: *mode },
            };
            (p.clone(), m)
        })
        .collect()
}

fn is_dir(model: &BTreeMap<RelPath, M>, p: &[u8]) -> bool {
    matches!(model.get(p), Some(M::Dir { .. }))
}

fn kids(model: &BTreeMap<RelPath, M>, dir: &[u8]) -> Vec<Vec<u8>> {
    model
        .keys()
        .filter(|k| !k.is_empty() && parent(k) == dir)
        .map(|k| file_name(k).to_vec())
        .collect()
}

/// Applies a rename to the model, or `None` if POSIX would refuse it.
fn model_rename(model: &mut BTreeMap<RelPath, M>, from: &[u8], to: &[u8]) -> Option<()> {
    if is_within(to, from) || to == from {
        return None;
    }
    match (model.get(from).cloned(), model.get(to).cloned()) {
        (Some(M::Dir { .. }), Some(M::Dir { .. })) if !kids(model, to).is_empty() => {
            return None;
        }
        (Some(M::Dir { .. }), Some(M::File { .. } | M::Link(_)))
        | (Some(M::File { .. } | M::Link(_)), Some(M::Dir { .. })) => return None,
        _ => {}
    }
    model.retain(|k, _| !is_within(k, to));
    let moved: Vec<(RelPath, M)> = model
        .iter()
        .filter(|(k, _)| is_within(k, from))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    model.retain(|k, _| !is_within(k, from));
    for (k, v) in moved {
        let mut dest = to.to_vec();
        dest.extend_from_slice(&k[from.len()..]);
        model.insert(dest, v);
    }
    Some(())
}

/// Turns raw choices into a valid op against `model`, applying it there.
fn interpret(model: &mut BTreeMap<RelPath, M>, c: (u8, u8, u8, u8)) -> Option<Op> {
    let (kind, a, b, n) = c;
    let paths: Vec<RelPath> = model.keys().cloned().collect();
    let dirs: Vec<RelPath> = paths.iter().filter(|p| is_dir(model, p)).cloned().collect();
    let pick = |v: &[RelPath], i: u8| v[usize::from(i) % v.len()].clone();
    let names = [b"n1".as_slice(), b"n2", b"x", b"b", b"z"];
    let fresh = |d: &[u8], i: u8| join(d, names[usize::from(i) % names.len()]);
    let nonroot: Vec<RelPath> = paths.iter().filter(|p| !p.is_empty()).cloned().collect();
    let blob = BlobHash::from_bytes([n; 32]);
    match kind % 8 {
        0 => {
            // Create or overwrite a file.
            let p = fresh(&pick(&dirs, a), b);
            if is_dir(model, &p) || matches!(model.get(&p), Some(M::Link(_))) {
                return None;
            }
            let mode = if n % 2 == 0 { 0o644 } else { 0o755 };
            model.insert(
                p.clone(),
                M::File {
                    content: Content::Blob(blob),
                    mode,
                },
            );
            Some(Op::Write {
                kind: WriteKind::Write,
                path: p,
                blob,
                mode,
            })
        }
        1 => {
            let p = fresh(&pick(&dirs, a), b);
            if model.contains_key(&p) {
                return None;
            }
            model.insert(p.clone(), M::Dir { mode: 0o755 });
            Some(Op::Mkdir {
                path: p,
                mode: 0o755,
            })
        }
        2 => {
            let p = fresh(&pick(&dirs, a), b);
            if model.contains_key(&p) {
                return None;
            }
            model.insert(p.clone(), M::Link(vec![n]));
            Some(Op::Symlink {
                path: p,
                target: vec![n],
            })
        }
        3 => {
            if nonroot.is_empty() {
                return None;
            }
            let p = pick(&nonroot, a);
            if is_dir(model, &p) {
                return None;
            }
            model.remove(&p);
            Some(Op::Unlink { path: p })
        }
        4 => {
            if nonroot.is_empty() {
                return None;
            }
            let p = pick(&nonroot, a);
            if !is_dir(model, &p) || !kids(model, &p).is_empty() {
                return None;
            }
            model.remove(&p);
            Some(Op::Rmdir { path: p })
        }
        5 => {
            if nonroot.is_empty() {
                return None;
            }
            let from = pick(&nonroot, a);
            let to = fresh(&pick(&dirs, b), n);
            model_rename(model, &from, &to)?;
            Some(Op::Rename { from, to })
        }
        6 => {
            let p = pick(&paths, a);
            let mode = if b % 2 == 0 { 0o700 } else { 0o644 };
            match model.get_mut(&p)? {
                M::File { mode: m, .. } | M::Dir { mode: m } => *m = mode,
                M::Link(_) => {}
            }
            Some(Op::Chmod { path: p, mode })
        }
        _ => {
            let p = pick(&paths, a);
            Some(Op::Touch {
                path: p,
                mtime: i64::from(b),
            })
        }
    }
}

/// Checks the overlay against the model: every path the model has, and
/// nothing else in any listed directory.
fn check(delta: &Delta, lower: &Base, model: &BTreeMap<RelPath, M>) {
    for (p, m) in model {
        let node = delta
            .resolve(p, lower)
            .unwrap_or_else(|| panic!("{} is missing", String::from_utf8_lossy(p)));
        match (m, &node) {
            (
                M::File { content, mode },
                Node::File {
                    content: c,
                    mode: md,
                    ..
                },
            ) => assert_eq!((content, mode), (c, md), "{}", String::from_utf8_lossy(p)),
            (M::Link(t), Node::Symlink { target, .. }) => assert_eq!(t, target),
            (M::Dir { mode }, Node::Dir { mode: md, .. }) => {
                assert_eq!(mode, md, "{}", String::from_utf8_lossy(p));
                assert_eq!(
                    delta.children(p, lower),
                    kids(model, p),
                    "children of {:?}",
                    String::from_utf8_lossy(p)
                );
            }
            _ => panic!("{}: {m:?} vs {node:?}", String::from_utf8_lossy(p)),
        }
    }
}

fn choices() -> impl Strategy<Value = Vec<(u8, u8, u8, u8)>> {
    prop::collection::vec(any::<(u8, u8, u8, u8)>(), 1..60)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn overlay_matches_a_plain_filesystem(cs in choices()) {
        let lower = base();
        let mut model = model_of(&lower);
        let mut delta = Delta::default();
        for (i, c) in cs.into_iter().enumerate() {
            if let Some(op) = interpret(&mut model, c) {
                delta.apply(&op, Timestamp::from_millis(i64::try_from(i).unwrap()), &lower);
                check(&delta, &lower, &model);
            }
        }
    }

    #[test]
    fn replaying_the_op_log_reproduces_the_delta(cs in choices()) {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = Store::create(dir.path(), &RootRecord {
            id: RootId::from_bytes([1; 16]),
            path: PathBuf::from("/r"),
            first_commit: "a".repeat(40).parse().unwrap(),
            registered_at: Timestamp::from_millis(0),
        }).unwrap();
        let g = store
            .create_graft(&"g".parse().unwrap(), &"a".repeat(40).parse().unwrap(), BackendKind::Fuse)
            .unwrap();
        let lower = base();
        let mut model = model_of(&lower);
        let mut live = Delta::default();
        for (i, c) in cs.into_iter().enumerate() {
            let Some(op) = interpret(&mut model, c) else { continue };
            let at = Timestamp::from_millis(i64::try_from(i).unwrap() * 7);
            let changes = live.apply(&op, at, &lower);
            store.record(g.id, &Journaled {
                at, op: &op, actor: Some("prop"), changes: &changes, staged: &[], sync: false,
            }).unwrap();
        }
        let stored = store.delta(g.id).unwrap();
        prop_assert_eq!(&stored, &live);
        let log = store.op_log(g.id).unwrap();
        prop_assert_eq!(Delta::replay(&log, &lower), stored);
        prop_assert!(log.iter().enumerate().all(|(i, r)| r.seq == u64::try_from(i).unwrap() + 1));
    }
}
