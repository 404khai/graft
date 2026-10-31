//! Building and inspecting graft repositories against real git repos.

// Fixture helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use graft_core::{CommitId, GraftId, GraftName};
use graft_root::graft::{
    ChangeKind, FileCloner, GraftRepo, branch_name, create_worktree, unified_diff,
};
use graft_root::{Repo, RootError};

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
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim_end().to_owned()
}

/// Copies files and counts calls, standing in for the reflink cloner.
#[derive(Default)]
struct CountingCopier(AtomicUsize);

impl FileCloner for CountingCopier {
    fn clone_file(&self, src: &Path, dst: &Path) -> std::io::Result<()> {
        if dst.exists() {
            return Err(std::io::ErrorKind::AlreadyExists.into());
        }
        std::fs::copy(src, dst)?;
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// A Root with nested files, an executable, a symlink and an ignored file.
fn root_repo() -> (tempfile::TempDir, CommitId) {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(p.join("src/deep")).unwrap();
    std::fs::write(p.join("README.md"), "hello\n").unwrap();
    std::fs::write(p.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
    std::fs::write(p.join("src/deep/mod.rs"), "// deep\n").unwrap();
    std::fs::write(p.join("run.sh"), "#!/bin/sh\necho hi\n").unwrap();
    std::fs::write(p.join(".gitignore"), "target/\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("README.md", p.join("link")).unwrap();
    }
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "-m", "base"]);
    let head = git(p, &["rev-parse", "HEAD"]).parse().unwrap();
    (dir, head)
}

fn name(s: &str) -> GraftName {
    s.parse().unwrap()
}

fn make_graft(
    root_dir: &Path,
    base: &CommitId,
    graft_name: &str,
) -> (tempfile::TempDir, std::path::PathBuf, CountingCopier) {
    let root = Repo::discover(root_dir).unwrap();
    let home = tempfile::tempdir().unwrap();
    let dest = home.path().join(graft_name);
    let copier = CountingCopier::default();
    create_worktree(&root, &dest, &branch_name(&name(graft_name)), base, &copier).unwrap();
    (home, dest, copier)
}

#[test]
fn creates_a_clean_checkout_sharing_roots_objects() {
    let (root_dir, base) = root_repo();
    let (_home, dest, copier) = make_graft(root_dir.path(), &base, "auth");

    assert_eq!(
        std::fs::read_to_string(dest.join("src/deep/mod.rs")).unwrap(),
        "// deep\n"
    );
    assert_eq!(
        git(&dest, &["status", "--porcelain"]),
        "",
        "graft must start clean"
    );
    assert_eq!(git(&dest, &["rev-parse", "HEAD"]), base.as_str());
    assert_eq!(
        git(&dest, &["symbolic-ref", "HEAD"]),
        "refs/heads/graft/auth"
    );
    // Shares Root's object store instead of copying it.
    let alternates = std::fs::read_to_string(dest.join(".git/objects/info/alternates")).unwrap();
    assert!(alternates.trim_end().ends_with("objects"), "{alternates}");
    assert_eq!(git(&dest, &["count-objects"]), "0 objects, 0 kilobytes");
    git(&dest, &["fsck", "--no-progress"]);
    // Every clean regular file was cloned from Root's working tree.
    assert_eq!(copier.0.load(Ordering::Relaxed), 5);
    // `root` remote points at Root.
    let url = git(&dest, &["remote", "get-url", "root"]);
    assert_eq!(Path::new(&url), root_dir.path().canonicalize().unwrap());
}

#[cfg(unix)]
#[test]
fn preserves_modes_and_symlinks() {
    use std::os::unix::fs::PermissionsExt;
    let (root_dir, base) = root_repo();
    let (_home, dest, _) = make_graft(root_dir.path(), &base, "modes");
    let mode = std::fs::metadata(dest.join("run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_ne!(mode & 0o111, 0, "run.sh must stay executable");
    assert_eq!(
        std::fs::read_link(dest.join("link")).unwrap(),
        Path::new("README.md")
    );
}

#[test]
fn dirty_root_files_come_from_the_object_store() {
    let (root_dir, base) = root_repo();
    std::fs::write(root_dir.path().join("README.md"), "uncommitted edit\n").unwrap();
    std::fs::write(root_dir.path().join("untracked.txt"), "not in git\n").unwrap();
    std::fs::create_dir(root_dir.path().join("target")).unwrap();
    std::fs::write(root_dir.path().join("target/big"), "ignored\n").unwrap();

    let (_home, dest, copier) = make_graft(root_dir.path(), &base, "dirty");
    assert_eq!(
        std::fs::read_to_string(dest.join("README.md")).unwrap(),
        "hello\n"
    );
    assert!(!dest.join("untracked.txt").exists());
    assert!(!dest.join("target").exists());
    assert_eq!(
        copier.0.load(Ordering::Relaxed),
        4,
        "README.md must not be cloned"
    );
    assert_eq!(git(&dest, &["status", "--porcelain"]), "");
}

#[test]
fn an_older_base_is_checked_out_exactly() {
    let (root_dir, base) = root_repo();
    let p = root_dir.path();
    std::fs::write(p.join("src/lib.rs"), "pub fn b() {}\n").unwrap();
    std::fs::write(p.join("NEW.md"), "new\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "-m", "second"]);

    let (_home, dest, _) = make_graft(p, &base, "old");
    assert_eq!(
        std::fs::read_to_string(dest.join("src/lib.rs")).unwrap(),
        "pub fn a() {}\n"
    );
    assert!(!dest.join("NEW.md").exists());
    assert_eq!(git(&dest, &["status", "--porcelain"]), "");
}

#[test]
fn applies_git_filters_when_writing_from_objects() {
    let (root_dir, _) = root_repo();
    let p = root_dir.path();
    std::fs::write(p.join(".gitattributes"), "*.txt text eol=crlf\n").unwrap();
    std::fs::write(p.join("notes.txt"), "one\ntwo\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "-m", "crlf"]);
    let head: CommitId = git(p, &["rev-parse", "HEAD"]).parse().unwrap();
    // Force the object path by dirtying Root's copy.
    std::fs::write(p.join("notes.txt"), "dirty\r\n").unwrap();

    let (_home, dest, _) = make_graft(p, &head, "filters");
    assert_eq!(
        std::fs::read(dest.join("notes.txt")).unwrap(),
        b"one\r\ntwo\r\n"
    );
    assert_eq!(git(&dest, &["status", "--porcelain"]), "");
}

#[test]
fn refuses_trees_with_paths_that_escape() {
    let (root_dir, _) = root_repo();
    let p = root_dir.path();
    let blob = git(p, &["hash-object", "-w", "README.md"]);
    let mut checked = 0;
    for bad in [".git", ".GIT", "..", "."] {
        let inner = with_stdin(p, &["mktree"], &format!("100644 blob {blob}\tconfig\n")).unwrap();
        // Older or stricter gits refuse to build some of these trees at all.
        let Some(tree) = with_stdin(p, &["mktree"], &format!("040000 tree {inner}\t{bad}\n"))
        else {
            continue;
        };
        let commit: CommitId = git(p, &["commit-tree", &tree, "-m", "evil"])
            .parse()
            .unwrap();
        let root = Repo::discover(p).unwrap();
        let home = tempfile::tempdir().unwrap();
        let dest = home.path().join("evil");
        let err = create_worktree(
            &root,
            &dest,
            "graft/evil",
            &commit,
            &CountingCopier::default(),
        )
        .unwrap_err();
        // gix rejects some names while building the index; Graft's own
        // validation catches the rest. Either way the graft is refused.
        let refused = matches!(err, RootError::UnsafePath { .. })
            || format!("{err:?}").contains("is invalid");
        assert!(refused, "{bad}: {err:?}");
        assert!(!home.path().join("config").exists());
        checked += 1;
    }
    assert!(
        checked > 0,
        "git refused every malicious tree; the test checked nothing"
    );
}

/// Runs git with `input` on stdin; `None` if git fails.
fn with_stdin(dir: &Path, args: &[&str], input: &str) -> Option<String> {
    use std::io::Write;
    let mut child = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    out.status
        .success()
        .then(|| String::from_utf8(out.stdout).unwrap().trim().to_owned())
}

#[test]
fn reports_changes_since_the_base() {
    let (root_dir, base) = root_repo();
    let (_home, dest, _) = make_graft(root_dir.path(), &base, "work");
    let graft = GraftRepo::open(&dest).unwrap();
    assert_eq!(graft.changes_since(&base).unwrap(), Vec::new());

    std::fs::write(dest.join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
    std::fs::remove_file(dest.join("src/deep/mod.rs")).unwrap();
    std::fs::write(dest.join("added.txt"), "new\n").unwrap();
    std::fs::create_dir(dest.join("target")).unwrap();
    std::fs::write(dest.join("target/out"), "ignored\n").unwrap();
    // A committed change counts too.
    std::fs::write(dest.join("README.md"), "committed\n").unwrap();
    git(&dest, &["commit", "-q", "-am", "edit readme"]);

    let changes: Vec<_> = graft
        .changes_since(&base)
        .unwrap()
        .into_iter()
        .map(|c| (c.path.display().to_string(), c.kind))
        .collect();
    assert_eq!(
        changes,
        [
            ("README.md".to_owned(), ChangeKind::Modified),
            ("added.txt".to_owned(), ChangeKind::Added),
            ("src/deep/mod.rs".to_owned(), ChangeKind::Deleted),
            ("src/lib.rs".to_owned(), ChangeKind::Modified),
        ]
    );
}

#[test]
fn detects_uncommitted_and_unintegrated_work() {
    let (root_dir, base) = root_repo();
    let root = Repo::discover(root_dir.path()).unwrap();
    let (_home, dest, _) = make_graft(root_dir.path(), &base, "safety");
    let graft = GraftRepo::open(&dest).unwrap();
    assert_eq!(
        graft.uncommitted_paths().unwrap(),
        Vec::<std::path::PathBuf>::new()
    );
    assert_eq!(graft.unintegrated(&root).unwrap(), Vec::new());

    std::fs::write(dest.join("README.md"), "edit\n").unwrap();
    assert_eq!(graft.uncommitted_paths().unwrap(), [Path::new("README.md")]);

    git(&dest, &["commit", "-q", "-am", "edit"]);
    assert_eq!(
        graft.uncommitted_paths().unwrap(),
        Vec::<std::path::PathBuf>::new()
    );
    let lost = graft.unintegrated(&root).unwrap();
    assert_eq!(lost.len(), 1, "{lost:?}");
    assert_eq!(lost[0].commit.as_str(), git(&dest, &["rev-parse", "HEAD"]));

    // Once Root has the commit, nothing would be lost.
    git(
        root_dir.path(),
        &["fetch", "-q", dest.to_str().unwrap(), "graft/safety"],
    );
    let root = Repo::discover(root_dir.path()).unwrap();
    assert_eq!(graft.unintegrated(&root).unwrap(), Vec::new());
}

#[test]
fn renders_unified_diffs() {
    let (root_dir, base) = root_repo();
    let (_home, dest, _) = make_graft(root_dir.path(), &base, "diffs");
    std::fs::write(dest.join("README.md"), "hello\nworld\n").unwrap();
    std::fs::write(dest.join("bin.dat"), [0u8, 1, 2]).unwrap();
    let diffs = GraftRepo::open(&dest).unwrap().diff_since(&base).unwrap();
    let text: String = diffs.iter().map(unified_diff).collect();
    assert!(text.contains("diff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1,1 +1,2 @@\n hello\n+world\n"), "{text}");
    assert!(
        text.contains("new file\nBinary files /dev/null and b/bin.dat differ\n"),
        "{text}"
    );
}

#[test]
fn pins_keep_the_base_reachable() {
    let (root_dir, base) = root_repo();
    let root = Repo::discover(root_dir.path()).unwrap();
    let id = GraftId::from_bytes([5; 16]);
    assert_eq!(root.pinned(id).unwrap(), None);
    root.pin(id, &base, &name("p")).unwrap();
    assert_eq!(root.pinned(id).unwrap(), Some(base.clone()));
    assert_eq!(
        git(root_dir.path(), &["rev-parse", &format!("refs/graft/{id}")]),
        base.as_str()
    );
    root.unpin(id).unwrap();
    root.unpin(id).unwrap(); // idempotent
    assert_eq!(root.pinned(id).unwrap(), None);
}

#[test]
fn two_grafts_do_not_share_branch_locks() {
    let (root_dir, base) = root_repo();
    let (_h1, a, _) = make_graft(root_dir.path(), &base, "one");
    let (_h2, b, _) = make_graft(root_dir.path(), &base, "two");
    // Both can check out the same branch name Root has checked out.
    git(&a, &["checkout", "-q", "-b", "main"]);
    git(&b, &["checkout", "-q", "-b", "main"]);
    assert_eq!(
        git(root_dir.path(), &["symbolic-ref", "HEAD"]),
        "refs/heads/main"
    );
    // Neither left anything in Root's worktree list.
    assert_eq!(
        git(root_dir.path(), &["worktree", "list"]).lines().count(),
        1
    );
}
