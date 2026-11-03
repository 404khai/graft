//! The virtual graft tree model, checked against Root's checkout without
//! mounting anything, so it runs on every platform.

// Fixture helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used)]

mod conformance;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use conformance::{Fixture, git};
use graft_fs::view::{Errno, GraftView, Ino, LayerMount, NodeKind, ROOT_INO};
use graft_root::tree::ObjectStore;

#[derive(Debug, PartialEq, Eq)]
enum Item {
    Dir,
    File { data: Vec<u8>, exec: bool },
    Link(Vec<u8>),
}

fn walk_view(view: &GraftView, ino: Ino, prefix: &Path, out: &mut BTreeMap<PathBuf, Item>) {
    for e in view.readdir(ino).unwrap() {
        let path = prefix.join(&e.name);
        let attr = view.lookup(ino, &e.name).unwrap();
        assert_eq!(attr.ino, e.ino, "{}", path.display());
        assert_eq!(attr.kind, e.kind, "{}", path.display());
        match e.kind {
            NodeKind::Directory => {
                assert_eq!(view.parent(e.ino).unwrap(), ino);
                out.insert(path.clone(), Item::Dir);
                walk_view(view, e.ino, &path, out);
            }
            NodeKind::File => {
                let data = view.read(e.ino, 0, u32::MAX).unwrap();
                assert_eq!(attr.size, data.len() as u64, "size of {}", path.display());
                out.insert(
                    path,
                    Item::File {
                        data,
                        exec: attr.perm & 0o111 != 0,
                    },
                );
            }
            NodeKind::Symlink => {
                out.insert(path, Item::Link(view.readlink(e.ino).unwrap()));
            }
        }
    }
}

fn walk_disk(dir: &Path, prefix: &Path, out: &mut BTreeMap<PathBuf, Item>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        if prefix.as_os_str().is_empty() && e.file_name() == ".git" {
            continue;
        }
        let path = prefix.join(e.file_name());
        let meta = std::fs::symlink_metadata(e.path()).unwrap();
        if meta.is_dir() {
            out.insert(path.clone(), Item::Dir);
            walk_disk(&e.path(), &path, out);
        } else if meta.file_type().is_symlink() {
            let target = std::fs::read_link(e.path()).unwrap();
            out.insert(
                path,
                Item::Link(target.into_os_string().into_encoded_bytes()),
            );
        } else {
            use std::os::unix::fs::PermissionsExt;
            out.insert(
                path,
                Item::File {
                    data: std::fs::read(e.path()).unwrap(),
                    exec: meta.permissions().mode() & 0o111 != 0,
                },
            );
        }
    }
}

fn view_of(f: &Fixture, layers: Vec<LayerMount>) -> GraftView {
    GraftView::new(ObjectStore::open(&f.root()), &f.base, layers, 1000, 1000).unwrap()
}

/// A Root with nested directories, an executable, symlinks, CRLF
/// attributes and an empty-ish tree, on top of the conformance fixture.
fn rich_fixture() -> (tempfile::TempDir, Fixture) {
    let tmp = tempfile::tempdir().unwrap();
    let f = Fixture::new_in(tmp.path());
    let p = f.dir.path();
    std::fs::create_dir_all(p.join("a/b/c")).unwrap();
    std::fs::write(p.join("a/b/c/deep.txt"), "deep\n").unwrap();
    std::fs::write(p.join("run.sh"), "#!/bin/sh\necho hi\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::os::unix::fs::symlink("../README.md", p.join("a/up")).unwrap();
    std::os::unix::fs::symlink("missing", p.join("dangling")).unwrap();
    std::fs::create_dir_all(p.join("win")).unwrap();
    std::fs::write(p.join("win/.gitattributes"), "*.txt eol=crlf\n").unwrap();
    std::fs::write(p.join("win/notes.txt"), "a\nb\n").unwrap();
    std::fs::write(p.join("empty"), "").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "-m", "rich"]);
    // Check out again so Root's files are in working-tree form (CRLF).
    std::fs::remove_file(p.join("win/notes.txt")).unwrap();
    git(p, &["checkout", "--", "win/notes.txt"]);
    let base = git(p, &["rev-parse", "HEAD"]).parse().unwrap();
    (tmp, Fixture { base, ..f })
}

#[test]
fn serves_exactly_what_a_checkout_has() {
    let (_tmp, f) = rich_fixture();
    let view = view_of(&f, Vec::new());
    let mut seen = BTreeMap::new();
    walk_view(&view, ROOT_INO, Path::new(""), &mut seen);
    let mut disk = BTreeMap::new();
    walk_disk(f.dir.path(), Path::new(""), &mut disk);
    assert_eq!(seen, disk);
    assert_eq!(
        std::fs::read(f.dir.path().join("win/notes.txt")).unwrap(),
        b"a\r\nb\r\n",
        "the fixture must exercise conversion"
    );
}

#[test]
fn reads_lazily() {
    let (_tmp, f) = rich_fixture();
    let view = view_of(&f, Vec::new());
    assert_eq!(view.allocated(), 1, "creating a view reads nothing");
    let deep = view.resolve(Path::new("a/b/c/deep.txt")).unwrap();
    assert_eq!(view.read(deep.ino, 1, 3).unwrap(), b"eep");
    assert_eq!(view.read(deep.ino, 100, 3).unwrap(), b"");
    // Only the directories on the way were listed.
    let top = view.readdir(ROOT_INO).unwrap().len();
    assert!(view.allocated() < top + 8, "{} inodes", view.allocated());
}

#[test]
fn reports_errors_as_errno() {
    let (_tmp, f) = rich_fixture();
    let view = view_of(&f, Vec::new());
    let readme = view.resolve(Path::new("README.md")).unwrap();
    assert_eq!(view.lookup(ROOT_INO, "nope".as_ref()), Err(Errno::ENOENT));
    assert_eq!(view.readdir(readme.ino), Err(Errno::ENOTDIR));
    assert_eq!(view.lookup(readme.ino, "x".as_ref()), Err(Errno::ENOTDIR));
    assert_eq!(view.read(ROOT_INO, 0, 1), Err(Errno::EISDIR));
    assert_eq!(view.readlink(readme.ino), Err(Errno::EINVAL));
    assert_eq!(view.getattr(0), Err(Errno::ENOENT));
    assert_eq!(view.getattr(9_999_999), Err(Errno::ENOENT));
    assert_eq!(
        view.getattr(ROOT_INO).unwrap().mtime,
        view.getattr(readme.ino).unwrap().mtime
    );
}

#[test]
fn overlays_layers_from_the_cache() {
    let (tmp, f) = rich_fixture();
    let cache = tmp.path().join("cache");
    let nm = cache.join("k1");
    std::fs::create_dir_all(nm.join("left-pad/lib")).unwrap();
    std::fs::write(nm.join("left-pad/index.js"), "module.exports = 1;\n").unwrap();
    std::fs::write(nm.join("left-pad/lib/x.js"), "x").unwrap();
    std::os::unix::fs::symlink("../left-pad/index.js", nm.join("alias.js")).unwrap();
    let nested = cache.join("k2");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("built.o"), "obj").unwrap();

    let view = view_of(
        &f,
        vec![
            LayerMount {
                path: "node_modules".into(),
                source: nm.clone(),
            },
            // Parent exists in the tree.
            LayerMount {
                path: "a/b/out".into(),
                source: nested.clone(),
            },
            // Parent does not exist anywhere.
            LayerMount {
                path: "gen/x/out".into(),
                source: nested,
            },
        ],
    );
    let read = |p: &str| {
        let a = view.resolve(Path::new(p)).unwrap();
        view.read(a.ino, 0, 1 << 20).unwrap()
    };
    assert_eq!(
        read("node_modules/left-pad/index.js"),
        b"module.exports = 1;\n"
    );
    assert_eq!(read("node_modules/left-pad/lib/x.js"), b"x");
    assert_eq!(read("a/b/out/built.o"), b"obj");
    assert_eq!(read("gen/x/out/built.o"), b"obj");
    let alias = view.resolve(Path::new("node_modules/alias.js")).unwrap();
    assert_eq!(
        alias.kind,
        NodeKind::Symlink,
        "layer symlinks are not followed"
    );
    assert_eq!(view.readlink(alias.ino).unwrap(), b"../left-pad/index.js");
    // Tree contents next to a layer are still there.
    assert_eq!(read("a/b/c/deep.txt"), b"deep\n");
    let names: Vec<_> = view
        .readdir(ROOT_INO)
        .unwrap()
        .into_iter()
        .map(|e| e.name.into_string().unwrap())
        .collect();
    assert!(names.contains(&"node_modules".to_owned()) && names.contains(&"gen".to_owned()));
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "entries are sorted");
    // The layer cache itself is never modified by reading.
    assert!(nm.join("left-pad/index.js").is_file());
}

#[test]
fn refuses_unsafe_layer_paths() {
    let (tmp, f) = rich_fixture();
    for bad in ["../escape", "/abs", ".git/hooks"] {
        let r = GraftView::new(
            ObjectStore::open(&f.root()),
            &f.base,
            vec![LayerMount {
                path: bad.into(),
                source: tmp.path().to_owned(),
            }],
            0,
            0,
        );
        assert!(r.is_err(), "{bad}");
    }
}

#[test]
fn concurrent_readers_agree() {
    let (_tmp, f) = rich_fixture();
    let view = view_of(&f, Vec::new());
    let results: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                s.spawn(|| {
                    let mut seen = BTreeMap::new();
                    walk_view(&view, ROOT_INO, Path::new(""), &mut seen);
                    let inos: Vec<_> = ["a/b/c/deep.txt", "win/notes.txt", "large.bin"]
                        .iter()
                        .map(|p| view.resolve(Path::new(p)).unwrap().ino)
                        .collect();
                    (seen, inos)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for r in &results[1..] {
        assert_eq!(r, &results[0], "one path, one inode, same contents");
    }
}
