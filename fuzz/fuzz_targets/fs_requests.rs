//! The virtual graft request handlers that FUSE and NFS call (`GraftView`):
//! arbitrary sequences of lookups, creates, writes, renames, deletes,
//! symlinks and attribute changes. Errors are fine; panics are not. After
//! every sequence, replaying the stored op log must reproduce the stored
//! delta exactly (ADR 0008).
#![no_main]

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use arbitrary::Arbitrary;
use graft_core::{BackendKind, CommitId, RootId, Timestamp};
use graft_fs::view::{GraftView, Journal, ROOT_INO};
use graft_fs::{OpenMode, SetAttr};
use graft_root::Repo;
use graft_root::tree::ObjectStore;
use graft_store::blobs::BlobStore;
use graft_store::delta::Delta;
use graft_store::{RootRecord, Store};
use libfuzzer_sys::fuzz_target;

/// A small repository, built once per process.
struct Fixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
    base: CommitId,
}

fn fixture() -> &'static Fixture {
    static F: OnceLock<Fixture> = OnceLock::new();
    F.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_owned();
        std::fs::create_dir_all(p.join("a/b")).unwrap();
        std::fs::write(p.join("a/x"), "x\n").unwrap();
        std::fs::write(p.join("a/b/y"), "y\n").unwrap();
        std::fs::write(p.join("top"), "top\n").unwrap();
        std::os::unix::fs::symlink("x", p.join("a/link")).unwrap();
        let git = |args: &[&str]| {
            let ok = Command::new("git")
                .args(args)
                .current_dir(&p)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_AUTHOR_NAME", "f")
                .env("GIT_AUTHOR_EMAIL", "f@f")
                .env("GIT_COMMITTER_NAME", "f")
                .env("GIT_COMMITTER_EMAIL", "f@f")
                .status()
                .unwrap();
            assert!(ok.success());
        };
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        git(&["commit", "-qm", "base"]);
        let base = Repo::discover(&p).unwrap().head_commit().unwrap();
        Fixture { _dir: dir, path: p, base }
    })
}

#[derive(Debug, Arbitrary)]
enum Req {
    Lookup { dir: u8, name: u8 },
    Create { dir: u8, name: u8, mode: u16 },
    Open { node: u8, write: bool, truncate: bool },
    Write { fh: u8, offset: u16, data: Vec<u8> },
    Read { fh: u8, offset: u16, size: u8 },
    Flush { fh: u8 },
    Fsync { fh: u8 },
    Release { fh: u8 },
    Mkdir { dir: u8, name: u8 },
    Unlink { dir: u8, name: u8 },
    Rmdir { dir: u8, name: u8 },
    Rename { from_dir: u8, from: u8, to_dir: u8, to: u8, noreplace: bool },
    Symlink { dir: u8, name: u8, target: u8 },
    Chmod { node: u8, mode: u16 },
    Truncate { node: u8, size: u16 },
    Readdir { dir: u8 },
}

const NAMES: &[&str] = &["a", "b", "x", "y", "z", "top", "link", "new", ".", "..", "", "a/b", ".git"];

fn name(i: u8) -> &'static OsStr {
    OsStr::new(NAMES[usize::from(i) % NAMES.len()])
}

fn pick<T: Copy>(v: &[T], i: u8) -> Option<T> {
    (!v.is_empty()).then(|| v[usize::from(i) % v.len()])
}

fuzz_target!(|reqs: Vec<Req>| {
    let f = fixture();
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("root");
    std::fs::create_dir_all(&root_dir).unwrap();
    let (mut store, _) = Store::create(
        &root_dir,
        &RootRecord {
            id: RootId::from_bytes([5; 16]),
            path: f.path.clone(),
            first_commit: f.base.clone(),
            registered_at: Timestamp::from_millis(0),
        },
    )
    .unwrap();
    let graft = store
        .create_graft(&"w".parse().unwrap(), &f.base, BackendKind::Fuse)
        .unwrap()
        .id;
    let objects = ObjectStore::open(&Repo::discover(&f.path).unwrap());
    let view = GraftView::writable(
        objects,
        &f.base,
        Vec::new(),
        1000,
        1000,
        Journal {
            store: Store::open(&root_dir).unwrap(),
            blobs: BlobStore::new(root_dir.join("blobs")),
            graft,
            dir: root_dir.join("grafts").join(graft.to_string()),
            git_init: None,
            git_in_background: false,
            admission: None,
        },
    )
    .unwrap();
    let mut nodes = vec![ROOT_INO];
    let mut fhs: Vec<u64> = Vec::new();
    for r in reqs.into_iter().take(64) {
        match r {
            Req::Lookup { dir, name: n } => {
                if let Ok(a) = view.lookup(pick(&nodes, dir).unwrap(), name(n)) {
                    nodes.push(a.ino);
                }
            }
            Req::Create { dir, name: n, mode } => {
                if let Ok((a, fh)) = view.create(pick(&nodes, dir).unwrap(), name(n), u32::from(mode), 0o022) {
                    nodes.push(a.ino);
                    fhs.push(fh);
                }
            }
            Req::Open { node, write, truncate } => {
                let mode = OpenMode {
                    write,
                    truncate,
                    ..OpenMode::default()
                };
                if let Ok(fh) = view.open(pick(&nodes, node).unwrap(), mode) {
                    fhs.push(fh);
                }
            }
            Req::Write { fh, offset, data } => {
                if let Some(h) = pick(&fhs, fh) {
                    let _ = view.write(h, u64::from(offset), &data[..data.len().min(4096)]);
                }
            }
            Req::Read { fh, offset, size } => {
                if let Some(h) = pick(&fhs, fh) {
                    let _ = view.read_handle(h, u64::from(offset), u32::from(size));
                }
            }
            Req::Flush { fh } => {
                if let Some(h) = pick(&fhs, fh) {
                    let _ = view.flush(h);
                }
            }
            Req::Fsync { fh } => {
                if let Some(h) = pick(&fhs, fh) {
                    let _ = view.fsync(h);
                }
            }
            Req::Release { fh } => {
                if let Some(h) = pick(&fhs, fh) {
                    let _ = view.release(h);
                    fhs.retain(|x| *x != h);
                }
            }
            Req::Mkdir { dir, name: n } => {
                if let Ok(a) = view.mkdir(pick(&nodes, dir).unwrap(), name(n), 0o755, 0o022) {
                    nodes.push(a.ino);
                }
            }
            Req::Unlink { dir, name: n } => {
                let _ = view.unlink(pick(&nodes, dir).unwrap(), name(n));
            }
            Req::Rmdir { dir, name: n } => {
                let _ = view.rmdir(pick(&nodes, dir).unwrap(), name(n));
            }
            Req::Rename { from_dir, from, to_dir, to, noreplace } => {
                let _ = view.rename(
                    pick(&nodes, from_dir).unwrap(),
                    name(from),
                    pick(&nodes, to_dir).unwrap(),
                    name(to),
                    noreplace,
                );
            }
            Req::Symlink { dir, name: n, target } => {
                if let Ok(a) = view.symlink(pick(&nodes, dir).unwrap(), name(n), Path::new(name(target))) {
                    nodes.push(a.ino);
                }
            }
            Req::Chmod { node, mode } => {
                let set = SetAttr {
                    mode: Some(u32::from(mode) & 0o7777),
                    ..SetAttr::default()
                };
                let _ = view.setattr(pick(&nodes, node).unwrap(), set);
            }
            Req::Truncate { node, size } => {
                let set = SetAttr {
                    size: Some(u64::from(size)),
                    ..SetAttr::default()
                };
                let _ = view.setattr(pick(&nodes, node).unwrap(), set);
            }
            Req::Readdir { dir } => {
                let _ = view.readdir(pick(&nodes, dir).unwrap());
            }
        }
    }
    for fh in fhs {
        let _ = view.release(fh);
    }
    // The journal invariant: replay of the op log equals the stored delta.
    let stored = store.delta(graft).unwrap();
    let log = store.op_log(graft).unwrap();
    assert_eq!(Delta::replay(&log, view.lower()), stored, "replay differs from the delta");
});
