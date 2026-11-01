//! Lazy tree and blob reads for virtual grafts.

// Fixture helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::Command;

use graft_core::CommitId;
use graft_root::Repo;
use graft_root::tree::{EntryKind, Filter, ObjectStore, TreeEntry};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Graft Test")
        .env("GIT_AUTHOR_EMAIL", "test@graft.invalid")
        .env("GIT_COMMITTER_NAME", "Graft Test")
        .env("GIT_COMMITTER_EMAIL", "test@graft.invalid")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim_end().to_owned()
}

fn fixture() -> (tempfile::TempDir, CommitId) {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(p.join("src")).unwrap();
    std::fs::write(p.join("README.md"), "hello\n").unwrap();
    std::fs::write(p.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
    std::fs::write(p.join("run.sh"), "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("README.md", p.join("link")).unwrap();
    }
    std::fs::create_dir_all(p.join("crlf")).unwrap();
    std::fs::write(p.join("crlf/.gitattributes"), "*.txt eol=crlf\n").unwrap();
    std::fs::write(p.join("crlf/a.txt"), "one\ntwo\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "-m", "base"]);
    let base = git(p, &["rev-parse", "HEAD"]).parse().unwrap();
    (dir, base)
}

fn entry<'a>(entries: &'a [TreeEntry], name: &str) -> &'a TreeEntry {
    entries
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("no entry {name} in {entries:?}"))
}

#[test]
fn reads_trees_and_blobs_lazily() {
    let (dir, base) = fixture();
    let store = ObjectStore::open(&Repo::discover(dir.path()).unwrap());
    let root = store.commit_tree(&base).unwrap();
    assert_eq!(root.time, 1_700_000_000);

    let entries = store.tree(root.tree).unwrap();
    let names: Vec<_> = entries.iter().map(|e| e.name.to_str().unwrap()).collect();
    assert_eq!(names, ["README.md", "crlf", "link", "run.sh", "src"]);
    assert_eq!(entry(&entries, "src").kind, EntryKind::Tree);
    assert_eq!(entry(&entries, "README.md").kind, EntryKind::Blob);
    #[cfg(unix)]
    {
        assert_eq!(entry(&entries, "run.sh").kind, EntryKind::BlobExecutable);
        assert_eq!(entry(&entries, "link").kind, EntryKind::Symlink);
        assert_eq!(
            store.blob(entry(&entries, "link").id).unwrap(),
            b"README.md"
        );
    }

    let readme = entry(&entries, "README.md").id;
    assert_eq!(store.blob_size(readme).unwrap(), 6);
    assert_eq!(store.blob(readme).unwrap(), b"hello\n");

    let src = store.tree(entry(&entries, "src").id).unwrap();
    assert_eq!(
        store.blob(entry(&src, "lib.rs").id).unwrap(),
        b"pub fn a() {}\n"
    );
    assert!(
        store.blob(entry(&entries, "src").id).is_err(),
        "a tree is not a blob"
    );
}

#[test]
fn filters_with_attributes_from_the_base_tree() {
    let (dir, base) = fixture();
    // Root's working tree no longer has the attributes; the base tree does.
    std::fs::remove_file(dir.path().join("crlf/.gitattributes")).unwrap();
    let store = ObjectStore::open(&Repo::discover(dir.path()).unwrap());
    let root = store.commit_tree(&base).unwrap();
    let crlf = store
        .tree(entry(&store.tree(root.tree).unwrap(), "crlf").id)
        .unwrap();
    let raw = store.blob(entry(&crlf, "a.txt").id).unwrap();
    assert_eq!(raw, b"one\ntwo\n");

    let filter = Filter::spawn(&store, root.tree).unwrap();
    assert_eq!(
        filter.to_worktree(b"crlf/a.txt", raw.clone()).unwrap(),
        b"one\r\ntwo\r\n"
    );
    assert_eq!(
        filter.to_worktree(b"README.md", b"x\n".to_vec()).unwrap(),
        b"x\n"
    );
    // Usable from several threads at once.
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                assert_eq!(
                    filter.to_worktree(b"crlf/a.txt", raw.clone()).unwrap(),
                    b"one\r\ntwo\r\n"
                );
            });
        }
    });
}

#[test]
fn notices_attributes_configured_outside_the_tree() {
    let (dir, _) = fixture();
    git(dir.path(), &["config", "core.autocrlf", "true"]);
    assert!(ObjectStore::open(&Repo::discover(dir.path()).unwrap()).attributes_configured());
    git(dir.path(), &["config", "core.autocrlf", "false"]);
    std::fs::create_dir_all(dir.path().join(".git/info")).unwrap();
    std::fs::write(dir.path().join(".git/info/attributes"), "* text\n").unwrap();
    assert!(ObjectStore::open(&Repo::discover(dir.path()).unwrap()).attributes_configured());
}
