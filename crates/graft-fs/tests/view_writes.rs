//! The write path of a virtual graft through the view API (ADR 0008),
//! without mounting, so it runs on every platform.

// Fixture helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used)]

mod conformance;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use conformance::{Fixture, git};
use graft_core::{BackendKind, GraftId, RootId, Timestamp};
use graft_fs::view::{Errno, GraftView, Journal, NodeKind, ROOT_INO};
use graft_fs::{OpenMode, SetAttr};
use graft_root::tree::ObjectStore;
use graft_store::blobs::BlobStore;
use graft_store::delta::{Delta, Op};
use graft_store::{RootRecord, Store};

const W: OpenMode = OpenMode {
    write: true,
    truncate: false,
};
const R: OpenMode = OpenMode {
    write: false,
    truncate: false,
};

struct Env {
    tmp: tempfile::TempDir,
    fixture: Fixture,
    root_dir: PathBuf,
    graft: GraftId,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let fixture = Fixture::new_in(tmp.path());
        let root_dir = tmp.path().join("root");
        std::fs::create_dir_all(&root_dir).unwrap();
        let (mut store, _) = Store::create(
            &root_dir,
            &RootRecord {
                id: RootId::from_bytes([3; 16]),
                path: fixture.dir.path().to_owned(),
                first_commit: fixture.base.clone(),
                registered_at: Timestamp::from_millis(0),
            },
        )
        .unwrap();
        let graft = store
            .create_graft(&"w".parse().unwrap(), &fixture.base, BackendKind::Fuse)
            .unwrap()
            .id;
        Self {
            tmp,
            fixture,
            root_dir,
            graft,
        }
    }

    fn view(&self) -> GraftView {
        self.view_with_git(false)
    }

    fn view_with_git(&self, with_git: bool) -> GraftView {
        self.view_with(with_git, false)
    }

    fn view_with(&self, with_git: bool, git_in_background: bool) -> GraftView {
        let objects = ObjectStore::open(&self.fixture.root());
        let base = self.fixture.base.clone();
        let git_init: Option<graft_fs::view::GitInit> = with_git.then(|| {
            let objects = objects.clone();
            Box::new(move |dir: &Path| {
                graft_root::graft::init_virtual_git(&objects.root(), dir, "graft/w", &base, &[])
                    .map_err(|e| e.to_string())
            }) as graft_fs::view::GitInit
        });
        GraftView::writable(
            objects,
            &self.fixture.base,
            Vec::new(),
            1000,
            1000,
            Journal {
                store: Store::open(&self.root_dir).unwrap(),
                blobs: BlobStore::new(self.root_dir.join("blobs")),
                graft: self.graft,
                dir: self.root_dir.join("grafts").join(self.graft.to_string()),
                git_init,
                git_in_background,
            },
        )
        .unwrap()
    }

    fn store(&self) -> Store {
        Store::open(&self.root_dir).unwrap()
    }

    fn kinds(&self) -> Vec<&'static str> {
        self.store()
            .op_log(self.graft)
            .unwrap()
            .into_iter()
            .map(|r| r.op.kind())
            .collect()
    }
}

fn ino(v: &GraftView, p: &str) -> u64 {
    v.resolve(Path::new(p)).unwrap().ino
}

fn cat(v: &GraftView, p: &str) -> Vec<u8> {
    let i = ino(v, p);
    let fh = v.open(i, R).unwrap();
    let data = v.read_handle(fh, 0, 1 << 24).unwrap();
    v.release(fh).unwrap();
    data
}

fn dir_of(v: &GraftView, p: &str) -> u64 {
    if p.is_empty() { ROOT_INO } else { ino(v, p) }
}

fn put(v: &GraftView, dir: &str, name: &str, data: &[u8]) {
    let (_, fh) = v
        .create(dir_of(v, dir), OsStr::new(name), 0o644, 0o022)
        .unwrap();
    v.write(fh, 0, data).unwrap();
    v.release(fh).unwrap();
}

fn names(v: &GraftView, p: &str) -> Vec<String> {
    v.readdir(dir_of(v, p))
        .unwrap()
        .into_iter()
        .map(|e| e.name.into_string().unwrap())
        .collect()
}

/// The stored op log replayed from empty must equal the stored delta.
fn assert_replays(env: &Env, v: &GraftView) {
    let store = env.store();
    let stored = store.delta(env.graft).unwrap();
    assert_eq!(stored, v.delta(), "memory and disk disagree");
    let log = store.op_log(env.graft).unwrap();
    assert_eq!(Delta::replay(&log, v.lower()), stored, "replay differs");
}

#[test]
fn creates_writes_and_journals_files() {
    let env = Env::new();
    let v = env.view();
    put(&v, "src", "new.rs", b"fn new() {}\n");
    assert_eq!(cat(&v, "src/new.rs"), b"fn new() {}\n");
    assert_eq!(v.resolve(Path::new("src/new.rs")).unwrap().perm, 0o644);
    assert_eq!(names(&v, "src"), ["lib.rs", "new.rs"]);
    assert_eq!(env.kinds(), ["create", "write"]);
    // Root is untouched.
    assert!(!env.fixture.dir.path().join("src/new.rs").exists());
    assert_replays(&env, &v);
}

#[test]
fn copies_up_lower_files_and_writes_in_place() {
    let env = Env::new();
    let v = env.view();
    let fh = v.open(ino(&v, "README.md"), W).unwrap();
    v.write(fh, 5, b"!").unwrap();
    // Readers through other handles see the write before it is journaled.
    assert_eq!(cat(&v, "README.md"), b"hello!");
    v.release(fh).unwrap();
    assert_eq!(cat(&v, "README.md"), b"hello!");
    assert_eq!(
        std::fs::read(env.fixture.dir.path().join("README.md")).unwrap(),
        b"hello\n"
    );
    // A large file: write in the middle, size kept.
    let big = ino(&v, "large.bin");
    let fh = v.open(big, W).unwrap();
    v.write(fh, 1 << 20, b"graft").unwrap();
    v.release(fh).unwrap();
    let data = cat(&v, "large.bin");
    assert_eq!(data.len(), conformance::LARGE_FILE_BYTES);
    assert_eq!(&data[1 << 20..(1 << 20) + 5], b"graft");
    assert_eq!(env.kinds(), ["write", "write"]);
    assert_replays(&env, &v);
}

#[test]
fn truncates_through_setattr_and_o_trunc() {
    let env = Env::new();
    let v = env.view();
    let i = ino(&v, "README.md");
    let a = v
        .setattr(
            i,
            SetAttr {
                size: Some(2),
                ..SetAttr::default()
            },
        )
        .unwrap();
    assert_eq!(a.size, 2);
    assert_eq!(cat(&v, "README.md"), b"he");
    let fh = v
        .open(
            i,
            OpenMode {
                write: true,
                truncate: true,
            },
        )
        .unwrap();
    v.write(fh, 0, b"new").unwrap();
    v.release(fh).unwrap();
    assert_eq!(cat(&v, "README.md"), b"new");
    assert_eq!(env.kinds(), ["truncate", "truncate"]);
    assert_replays(&env, &v);
}

#[test]
fn renames_unlinks_and_directories() {
    let env = Env::new();
    let v = env.view();
    v.mkdir(ROOT_INO, OsStr::new("lib"), 0o755, 0).unwrap();
    v.rename(
        ino(&v, "src"),
        OsStr::new("lib.rs"),
        ino(&v, "lib"),
        OsStr::new("moved.rs"),
        false,
    )
    .unwrap();
    assert_eq!(cat(&v, "lib/moved.rs"), b"pub fn a() {}\n");
    assert_eq!(names(&v, "src"), Vec::<String>::new());
    v.rmdir(ROOT_INO, OsStr::new("src")).unwrap();
    // Rename a whole directory, then delete a file from Root's tree.
    v.rename(
        ROOT_INO,
        OsStr::new("lib"),
        ROOT_INO,
        OsStr::new("src"),
        false,
    )
    .unwrap();
    v.unlink(ROOT_INO, OsStr::new("README.md")).unwrap();
    assert_eq!(names(&v, ""), ["large.bin", "src"]);
    assert_eq!(names(&v, "src"), ["moved.rs"]);
    assert_eq!(
        env.kinds(),
        ["mkdir", "rename", "rmdir", "rename", "unlink"]
    );
    assert_replays(&env, &v);
    // A fresh view of the same graft sees the same tree.
    drop(v);
    let v = env.view();
    assert_eq!(names(&v, ""), ["large.bin", "src"]);
    assert_eq!(cat(&v, "src/moved.rs"), b"pub fn a() {}\n");
}

#[test]
fn refuses_what_posix_refuses() {
    let env = Env::new();
    let v = env.view();
    let src = ino(&v, "src");
    let e = |r: Result<(), Errno>| r.unwrap_err();
    assert_eq!(
        v.create(ROOT_INO, OsStr::new("README.md"), 0o644, 0)
            .unwrap_err(),
        Errno::EEXIST
    );
    assert_eq!(
        v.mkdir(ROOT_INO, OsStr::new("src"), 0o755, 0).unwrap_err(),
        Errno::EEXIST
    );
    assert_eq!(e(v.unlink(ROOT_INO, OsStr::new("src"))), Errno::EISDIR);
    assert_eq!(
        e(v.rmdir(ROOT_INO, OsStr::new("README.md"))),
        Errno::ENOTDIR
    );
    assert_eq!(e(v.rmdir(ROOT_INO, OsStr::new("src"))), Errno::ENOTEMPTY);
    assert_eq!(e(v.unlink(ROOT_INO, OsStr::new("nope"))), Errno::ENOENT);
    assert_eq!(
        e(v.rename(
            ROOT_INO,
            OsStr::new("src"),
            src,
            OsStr::new("inside"),
            false
        )),
        Errno::EINVAL
    );
    assert_eq!(
        e(v.rename(
            ROOT_INO,
            OsStr::new("README.md"),
            ROOT_INO,
            OsStr::new("src"),
            false
        )),
        Errno::EISDIR
    );
    assert_eq!(
        e(v.rename(
            ROOT_INO,
            OsStr::new("src"),
            ROOT_INO,
            OsStr::new("README.md"),
            false
        )),
        Errno::ENOTDIR
    );
    assert_eq!(
        e(v.rename(
            ROOT_INO,
            OsStr::new("README.md"),
            ROOT_INO,
            OsStr::new("large.bin"),
            true
        )),
        Errno::EEXIST
    );
    assert_eq!(
        v.create(ino(&v, "README.md"), OsStr::new("x"), 0o644, 0)
            .unwrap_err(),
        Errno::ENOTDIR
    );
    assert_eq!(
        v.create(ROOT_INO, OsStr::new(&"n".repeat(300)), 0o644, 0)
            .unwrap_err(),
        Errno::ENAMETOOLONG
    );
    assert_eq!(
        v.setattr(
            ROOT_INO,
            SetAttr {
                uid: Some(0),
                ..SetAttr::default()
            }
        )
        .unwrap_err(),
        Errno::EPERM
    );
    assert_eq!(
        v.mknod(ROOT_INO, OsStr::new("fifo"), 0o010_644, 0)
            .unwrap_err(),
        Errno::EPERM
    );
    assert_eq!(
        env.kinds(),
        Vec::<&str>::new(),
        "refused ops journal nothing"
    );
}

#[test]
fn symlinks_modes_and_times() {
    let env = Env::new();
    let v = env.view();
    let a = v
        .symlink(ROOT_INO, OsStr::new("link"), Path::new("README.md"))
        .unwrap();
    assert_eq!(a.kind, NodeKind::Symlink);
    assert_eq!(v.readlink(a.ino).unwrap(), b"README.md");
    let i = ino(&v, "README.md");
    assert_eq!(
        v.setattr(
            i,
            SetAttr {
                mode: Some(0o755),
                ..SetAttr::default()
            }
        )
        .unwrap()
        .perm,
        0o755
    );
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
    assert_eq!(
        v.setattr(
            i,
            SetAttr {
                mtime: Some(t),
                ..SetAttr::default()
            }
        )
        .unwrap()
        .mtime,
        t
    );
    // Contents are unchanged by chmod and touch.
    assert_eq!(cat(&v, "README.md"), b"hello\n");
    assert_eq!(env.kinds(), ["symlink", "chmod", "touch"]);
    assert_replays(&env, &v);
}

#[test]
fn an_unlinked_open_file_keeps_working_but_is_gone() {
    let env = Env::new();
    let v = env.view();
    let (attr, fh) = v.create(ROOT_INO, OsStr::new("tmp"), 0o600, 0).unwrap();
    v.write(fh, 0, b"scratch").unwrap();
    v.unlink(ROOT_INO, OsStr::new("tmp")).unwrap();
    v.write(fh, 7, b"!").unwrap();
    assert_eq!(v.read_handle(fh, 0, 64).unwrap(), b"scratch!");
    assert_eq!(
        v.getattr(attr.ino).unwrap().size,
        7,
        "fstat sees the orphan"
    );
    v.release(fh).unwrap();
    assert_eq!(
        v.lookup(ROOT_INO, OsStr::new("tmp")).unwrap_err(),
        Errno::ENOENT
    );
    assert_eq!(
        env.kinds(),
        ["create", "unlink"],
        "writes to a deleted file are not journaled"
    );
    assert_replays(&env, &v);
}

#[test]
fn a_crash_loses_nothing_written_before_it() {
    let env = Env::new();
    {
        let v = env.view();
        let fh = v.open(ino(&v, "README.md"), W).unwrap();
        v.write(fh, 0, b"HELLO").unwrap();
        let (_, fh2) = v.create(ROOT_INO, OsStr::new("half"), 0o644, 0).unwrap();
        v.write(fh2, 0, b"written").unwrap();
        // The daemon dies here: no flush, no release.
        std::mem::forget(v);
    }
    assert_eq!(env.kinds(), ["create"]);
    let v = env.view();
    assert_eq!(cat(&v, "README.md"), b"HELLO\n");
    assert_eq!(cat(&v, "half"), b"written");
    assert_eq!(env.kinds(), ["create", "write", "write"]);
    assert_eq!(env.store().staged(env.graft).unwrap(), Vec::new());
    assert_replays(&env, &v);
}

#[test]
fn concurrent_writers() {
    let env = Env::new();
    let v = env.view();
    std::thread::scope(|s| {
        for t in 0..8 {
            let v = &v;
            s.spawn(move || {
                for i in 0..25 {
                    put(v, "", &format!("t{t}-{i}"), format!("{t}:{i}").as_bytes());
                }
                let fh = v
                    .open(
                        ino(v, "README.md"),
                        OpenMode {
                            write: true,
                            truncate: true,
                        },
                    )
                    .unwrap();
                v.write(fh, 0, format!("writer {t}\n").as_bytes()).unwrap();
                v.release(fh).unwrap();
            });
        }
    });
    for t in 0..8 {
        for i in 0..25 {
            assert_eq!(cat(&v, &format!("t{t}-{i}")), format!("{t}:{i}").as_bytes());
        }
    }
    assert!(cat(&v, "README.md").starts_with(b"writer "));
    assert_replays(&env, &v);
}

#[test]
fn read_only_views_refuse_writes() {
    let env = Env::new();
    let v = GraftView::new(
        ObjectStore::open(&env.fixture.root()),
        &env.fixture.base,
        Vec::new(),
        0,
        0,
    )
    .unwrap();
    assert_eq!(v.open(ino(&v, "README.md"), W).unwrap_err(), Errno::EROFS);
    assert_eq!(
        v.mkdir(ROOT_INO, OsStr::new("d"), 0o755, 0).unwrap_err(),
        Errno::EROFS
    );
    let _ = &env.tmp;
}

#[test]
fn git_lives_in_a_real_directory_created_on_first_use() {
    let env = Env::new();
    let v = env.view_with_git(true);
    let git_dir = env
        .root_dir
        .join("grafts")
        .join(env.graft.to_string())
        .join("git");
    assert!(!git_dir.exists(), "creating a view does not build .git");
    assert_eq!(names(&v, "")[0], ".git");
    let head = cat(&v, ".git/HEAD");
    assert_eq!(head, b"ref: refs/heads/graft/w\n");
    // Writes under .git go to the real directory, not the delta.
    put(&v, ".git", "graft-test", b"x");
    assert_eq!(std::fs::read(git_dir.join("graft-test")).unwrap(), b"x");
    assert!(v.delta().is_empty());
    assert_eq!(
        v.rename(
            ino(&v, ".git"),
            OsStr::new("graft-test"),
            ROOT_INO,
            OsStr::new("out"),
            false
        )
        .unwrap_err(),
        Errno::EXDEV
    );
    // The index matches the base tree: git status of an identical tree is
    // clean.
    let copy = env.tmp.path().join("copy");
    git(
        env.fixture.dir.path(),
        &["worktree", "add", "-q", "--detach", copy.to_str().unwrap()],
    );
    let out = std::process::Command::new("git")
        .args([
            "--git-dir",
            git_dir.to_str().unwrap(),
            "--work-tree",
            copy.to_str().unwrap(),
        ])
        .args(["status", "--porcelain", "--untracked-files=no"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
    // Every write op above was under .git, so nothing was journaled.
    assert_eq!(env.kinds(), Vec::<&str>::new());
    let _: Option<Op> = None;
}

#[test]
fn changes_come_from_the_delta_and_match_git_semantics() {
    use graft_root::graft::ChangeKind::{Added, Deleted, Modified};
    let env = Env::new();
    let v = env.view();
    put(&v, "src", "new.rs", b"new\n");
    // Rewriting a file with its original contents is not a change.
    let fh = v
        .open(
            ino(&v, "README.md"),
            OpenMode {
                write: true,
                truncate: true,
            },
        )
        .unwrap();
    v.write(fh, 0, b"hello\n").unwrap();
    v.release(fh).unwrap();
    // chmod without the executable bit is invisible to git; with it, not.
    let lib = ino(&v, "src/lib.rs");
    v.setattr(
        lib,
        SetAttr {
            mode: Some(0o600),
            ..SetAttr::default()
        },
    )
    .unwrap();
    v.setattr(
        ino(&v, "large.bin"),
        SetAttr {
            mode: Some(0o755),
            ..SetAttr::default()
        },
    )
    .unwrap();
    // Renaming a directory deletes every file under the old name.
    v.rename(
        ROOT_INO,
        OsStr::new("src"),
        ROOT_INO,
        OsStr::new("lib"),
        false,
    )
    .unwrap();
    let changes = graft_fs::virtual_changes(
        &ObjectStore::open(&env.fixture.root()),
        &env.fixture.base,
        Vec::new(),
        &env.store().delta(env.graft).unwrap(),
        &BlobStore::new(env.root_dir.join("blobs")),
    )
    .unwrap();
    let got: Vec<_> = changes
        .iter()
        .map(|c| (c.path.to_str().unwrap().to_owned(), c.kind))
        .collect();
    let want = [
        ("large.bin", Modified),
        ("lib/lib.rs", Added),
        ("lib/new.rs", Added),
        ("src/lib.rs", Deleted),
    ];
    assert_eq!(got, want.map(|(p, k)| (p.to_owned(), k)));
}

/// NFS grafts build .git in the background: callers get EAGAIN (which the
/// NFS client retries) until it is ready, never a long blocking call.
#[test]
fn git_can_be_built_in_the_background() {
    let env = Env::new();
    let v = env.view_with(true, true);
    let first = v.lookup(ROOT_INO, OsStr::new(".git"));
    assert!(first == Err(Errno::EAGAIN) || first.is_ok(), "{first:?}");
    let started = std::time::Instant::now();
    let attr = loop {
        match v.lookup(ROOT_INO, OsStr::new(".git")) {
            Ok(a) => break a,
            Err(Errno::EAGAIN) => {
                assert!(started.elapsed().as_secs() < 30, "never became ready");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => panic!("{e:?}"),
        }
    };
    assert_eq!(attr.kind, NodeKind::Directory);
    assert_eq!(cat(&v, ".git/HEAD"), b"ref: refs/heads/graft/w\n");
}

/// A .git a crash left half-built (no index) is rebuilt; a finished one,
/// with whatever was committed in it, is kept.
#[test]
fn a_half_built_git_is_rebuilt_and_a_finished_one_kept() {
    let env = Env::new();
    let git_dir = env
        .root_dir
        .join("grafts")
        .join(env.graft.to_string())
        .join("git");
    std::fs::create_dir_all(&git_dir).unwrap();
    std::fs::write(git_dir.join("HEAD"), "garbage").unwrap();
    let v = env.view_with_git(true);
    assert_eq!(cat(&v, ".git/HEAD"), b"ref: refs/heads/graft/w\n");
    drop(v);
    std::fs::write(git_dir.join("graft-kept"), "mine").unwrap();
    let v = env.view_with_git(true);
    assert_eq!(cat(&v, ".git/graft-kept"), b"mine");
}
