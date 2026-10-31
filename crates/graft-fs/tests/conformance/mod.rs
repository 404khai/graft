//! The backend conformance suite (AGENTS.md §6).
//!
//! Every `FsBackend` must pass every check here. Each check gets a freshly
//! created graft of the fixture Root and exercises one behaviour through the
//! ordinary filesystem API, then checks what `diff` and `prune` report.
//! Later backends (FUSE, NFS) reuse this module unchanged.

use std::path::{Path, PathBuf};
use std::process::Command;

use graft_core::{CommitId, GraftId, GraftName};
use graft_fs::{FsBackend, GraftSpec};
use graft_root::Repo;
use graft_root::graft::ChangeKind;

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Graft Test")
        .env("GIT_AUTHOR_EMAIL", "test@graft.invalid")
        .env("GIT_COMMITTER_NAME", "Graft Test")
        .env("GIT_COMMITTER_EMAIL", "test@graft.invalid")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim_end().to_owned()
}

/// A Root with a handful of files plus one large file.
pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub base: CommitId,
}

pub const LARGE_FILE_BYTES: usize = 8 << 20;

impl Fixture {
    pub fn new_in(parent: &Path) -> Self {
        let dir = tempfile::tempdir_in(parent).unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        std::fs::create_dir_all(p.join("src")).unwrap();
        std::fs::write(p.join("README.md"), "hello\n").unwrap();
        std::fs::write(p.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        let large: Vec<u8> = (0..251u8).cycle().take(LARGE_FILE_BYTES).collect();
        std::fs::write(p.join("large.bin"), large).unwrap();
        git(p, &["add", "-A"]);
        git(p, &["commit", "-q", "-m", "base"]);
        let base = git(p, &["rev-parse", "HEAD"]).parse().unwrap();
        Self { dir, base }
    }

    pub fn root(&self) -> Repo {
        Repo::discover(self.dir.path()).unwrap()
    }
}

/// One graft created for a check, pruned when the check is done.
pub struct Graft<'a> {
    pub backend: &'a dyn FsBackend,
    pub name: GraftName,
    pub id: GraftId,
    pub path: PathBuf,
}

pub fn create<'a>(backend: &'a dyn FsBackend, fixture: &Fixture, name: &str) -> Graft<'a> {
    let name: GraftName = name.parse().unwrap();
    let id = GraftId::random().unwrap();
    let report = backend
        .create(
            &fixture.root(),
            &GraftSpec {
                id,
                name: &name,
                base: &fixture.base,
            },
        )
        .unwrap();
    assert_eq!(report.path, backend.path(&name));
    Graft {
        backend,
        name,
        id,
        path: report.path,
    }
}

fn changes(g: &Graft<'_>, base: &CommitId) -> Vec<(String, ChangeKind)> {
    g.backend
        .diff(&g.name, base)
        .unwrap()
        .into_iter()
        .map(|c| (c.path.display().to_string(), c.kind))
        .collect()
}

fn s(path: &str, kind: ChangeKind) -> (String, ChangeKind) {
    (path.to_owned(), kind)
}

/// Runs every check against `backend`.
pub fn run(backend: &dyn FsBackend, fixture: &Fixture) {
    reads_match_root(backend, fixture);
    create_and_write(backend, fixture);
    rename_and_unlink(backend, fixture);
    symlinks(backend, fixture);
    permissions(backend, fixture);
    large_files(backend, fixture);
    many_files(backend, fixture);
    concurrent_writers(backend, fixture);
    independent_grafts(backend, fixture);
    prune_leaves_nothing(backend, fixture);
    refuses_an_existing_path(backend, fixture);
}

fn reads_match_root(backend: &dyn FsBackend, f: &Fixture) {
    let g = create(backend, f, "reads");
    for rel in ["README.md", "src/lib.rs", "large.bin"] {
        assert_eq!(
            std::fs::read(g.path.join(rel)).unwrap(),
            std::fs::read(f.dir.path().join(rel)).unwrap(),
            "{rel}"
        );
    }
    assert_eq!(changes(&g, &f.base), Vec::new());
    backend.prune(&g.name, g.id).unwrap();
}

fn create_and_write(backend: &dyn FsBackend, f: &Fixture) {
    let g = create(backend, f, "write");
    std::fs::write(g.path.join("src/new.rs"), "new\n").unwrap();
    std::fs::write(g.path.join("README.md"), "changed\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(g.path.join("README.md")).unwrap(),
        "changed\n"
    );
    // Writes in a graft never reach Root.
    assert_eq!(
        std::fs::read_to_string(f.dir.path().join("README.md")).unwrap(),
        "hello\n"
    );
    assert_eq!(
        changes(&g, &f.base),
        [
            s("README.md", ChangeKind::Modified),
            s("src/new.rs", ChangeKind::Added)
        ]
    );
    backend.prune(&g.name, g.id).unwrap();
}

fn rename_and_unlink(backend: &dyn FsBackend, f: &Fixture) {
    let g = create(backend, f, "rename");
    std::fs::rename(g.path.join("src/lib.rs"), g.path.join("src/moved.rs")).unwrap();
    std::fs::remove_file(g.path.join("README.md")).unwrap();
    assert!(f.dir.path().join("README.md").exists());
    assert_eq!(
        changes(&g, &f.base),
        [
            s("README.md", ChangeKind::Deleted),
            s("src/lib.rs", ChangeKind::Deleted),
            s("src/moved.rs", ChangeKind::Added),
        ]
    );
    backend.prune(&g.name, g.id).unwrap();
}

fn symlinks(backend: &dyn FsBackend, f: &Fixture) {
    #[cfg(unix)]
    {
        let g = create(backend, f, "symlinks");
        std::os::unix::fs::symlink("README.md", g.path.join("link")).unwrap();
        assert_eq!(
            std::fs::read_to_string(g.path.join("link")).unwrap(),
            "hello\n"
        );
        assert_eq!(changes(&g, &f.base), [s("link", ChangeKind::Added)]);
        backend.prune(&g.name, g.id).unwrap();
    }
    let _ = (backend, f);
}

fn permissions(backend: &dyn FsBackend, f: &Fixture) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let g = create(backend, f, "perms");
        let file = g.path.join("README.md");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o755
        );
        let root_mode = std::fs::metadata(f.dir.path().join("README.md"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(root_mode & 0o111, 0, "chmod in a graft must not reach Root");
        assert_eq!(changes(&g, &f.base), [s("README.md", ChangeKind::Modified)]);
        backend.prune(&g.name, g.id).unwrap();
    }
    let _ = (backend, f);
}

fn large_files(backend: &dyn FsBackend, f: &Fixture) {
    use std::io::{Seek, SeekFrom, Write};
    let g = create(backend, f, "large");
    let path = g.path.join("large.bin");
    let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start((LARGE_FILE_BYTES / 2) as u64))
        .unwrap();
    file.write_all(b"graft").unwrap();
    drop(file);
    let data = std::fs::read(&path).unwrap();
    assert_eq!(data.len(), LARGE_FILE_BYTES);
    assert_eq!(
        &data[LARGE_FILE_BYTES / 2..LARGE_FILE_BYTES / 2 + 5],
        b"graft"
    );
    let root = std::fs::read(f.dir.path().join("large.bin")).unwrap();
    assert_ne!(
        &root[LARGE_FILE_BYTES / 2..LARGE_FILE_BYTES / 2 + 5],
        b"graft"
    );
    assert_eq!(changes(&g, &f.base), [s("large.bin", ChangeKind::Modified)]);
    backend.prune(&g.name, g.id).unwrap();
}

fn many_files(backend: &dyn FsBackend, f: &Fixture) {
    let g = create(backend, f, "many");
    let dir = g.path.join("gen");
    std::fs::create_dir(&dir).unwrap();
    for i in 0..2000 {
        std::fs::write(dir.join(format!("f{i:04}.txt")), i.to_string()).unwrap();
    }
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2000);
    assert_eq!(changes(&g, &f.base).len(), 2000);
    backend.prune(&g.name, g.id).unwrap();
}

fn concurrent_writers(backend: &dyn FsBackend, f: &Fixture) {
    let g = create(backend, f, "concurrent");
    std::thread::scope(|scope| {
        for t in 0..8 {
            let path = g.path.clone();
            scope.spawn(move || {
                for i in 0..50 {
                    std::fs::write(path.join(format!("t{t}-{i}.txt")), format!("{t}:{i}")).unwrap();
                }
                // Everyone also rewrites one shared file.
                std::fs::write(path.join("README.md"), format!("writer {t}\n")).unwrap();
            });
        }
    });
    for t in 0..8 {
        for i in 0..50 {
            let got = std::fs::read_to_string(g.path.join(format!("t{t}-{i}.txt"))).unwrap();
            assert_eq!(got, format!("{t}:{i}"));
        }
    }
    let readme = std::fs::read_to_string(g.path.join("README.md")).unwrap();
    assert!(readme.starts_with("writer "), "{readme}");
    assert_eq!(changes(&g, &f.base).len(), 8 * 50 + 1);
    backend.prune(&g.name, g.id).unwrap();
}

fn independent_grafts(backend: &dyn FsBackend, f: &Fixture) {
    let a = create(backend, f, "ind-a");
    let b = create(backend, f, "ind-b");
    std::fs::write(a.path.join("README.md"), "from a\n").unwrap();
    std::fs::write(b.path.join("only-b.txt"), "b\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(b.path.join("README.md")).unwrap(),
        "hello\n"
    );
    assert!(!a.path.join("only-b.txt").exists());
    assert_eq!(changes(&a, &f.base), [s("README.md", ChangeKind::Modified)]);
    assert_eq!(changes(&b, &f.base), [s("only-b.txt", ChangeKind::Added)]);
    backend.prune(&a.name, a.id).unwrap();
    assert!(
        b.path.join("only-b.txt").exists(),
        "pruning a must not touch b"
    );
    backend.prune(&b.name, b.id).unwrap();
}

fn prune_leaves_nothing(backend: &dyn FsBackend, f: &Fixture) {
    let g = create(backend, f, "prune");
    std::fs::write(g.path.join("x"), "x").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Read-only directories (as Go's module cache makes) must not block prune.
        let ro = g.path.join("ro");
        std::fs::create_dir(&ro).unwrap();
        std::fs::write(ro.join("f"), "f").unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
        // A symlink out of the graft must be removed, not followed.
        std::os::unix::fs::symlink(f.dir.path(), g.path.join("escape")).unwrap();
    }
    backend.prune(&g.name, g.id).unwrap();
    assert!(!g.path.exists());
    backend.prune(&g.name, g.id).unwrap(); // idempotent
    assert!(
        f.dir.path().join("README.md").exists(),
        "prune must never follow symlinks into Root"
    );
}

fn refuses_an_existing_path(backend: &dyn FsBackend, f: &Fixture) {
    let g = create(backend, f, "taken");
    let err = backend
        .create(
            &f.root(),
            &GraftSpec {
                id: GraftId::random().unwrap(),
                name: &g.name,
                base: &f.base,
            },
        )
        .unwrap_err();
    assert!(
        matches!(err, graft_fs::FsError::AlreadyExists { .. }),
        "{err:?}"
    );
    assert_eq!(
        std::fs::read_to_string(g.path.join("README.md")).unwrap(),
        "hello\n"
    );
    backend.prune(&g.name, g.id).unwrap();
}
