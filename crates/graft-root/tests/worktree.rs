//! Grafts registered as git linked worktrees of Root (ADR 0036), checked
//! against real git.

// Fixture helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use graft_core::{CommitId, GraftName};
use graft_root::graft::{FileCloner, GraftRepo, branch_name, create_worktree, exclude_paths};
use graft_root::worktree::{BranchOutcome, BranchPolicy, Layout, prepare_virtual_git};
use graft_root::{Repo, RootError};

fn run_git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Graft Test")
        .env("GIT_AUTHOR_EMAIL", "test@graft.invalid")
        .env("GIT_COMMITTER_NAME", "Graft Test")
        .env("GIT_COMMITTER_EMAIL", "test@graft.invalid")
        .output()
        .unwrap()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = run_git(dir, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim_end().to_owned()
}

struct Copier;

impl FileCloner for Copier {
    fn clone_file(&self, src: &Path, dst: &Path) -> std::io::Result<()> {
        if dst.exists() {
            return Err(std::io::ErrorKind::AlreadyExists.into());
        }
        std::fs::copy(src, dst).map(|_| ())
    }
}

fn name(s: &str) -> GraftName {
    s.parse().unwrap()
}

struct Fixture {
    root_dir: tempfile::TempDir,
    home: tempfile::TempDir,
    base: CommitId,
}

impl Fixture {
    fn new() -> Self {
        let root_dir = tempfile::tempdir().unwrap();
        let p = root_dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        std::fs::create_dir_all(p.join("src")).unwrap();
        std::fs::write(p.join("README.md"), "hello\n").unwrap();
        std::fs::write(p.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        git(p, &["add", "-A"]);
        git(p, &["commit", "-q", "-m", "base"]);
        let base = git(p, &["rev-parse", "HEAD"]).parse().unwrap();
        Self {
            root_dir,
            home: tempfile::tempdir().unwrap(),
            base,
        }
    }

    fn root(&self) -> Repo {
        Repo::discover(self.root_dir.path()).unwrap()
    }

    fn path(&self, graft: &str) -> PathBuf {
        self.home.path().join(graft)
    }

    /// A reflink-style graft at `path(graft)`, registered.
    fn reflink(&self, graft: &str) -> PathBuf {
        let root = self.root();
        let dest = self.path(graft);
        create_worktree(
            &root,
            &dest,
            &branch_name(&name(graft)),
            &self.base,
            &Copier,
        )
        .unwrap();
        root.register(&name(graft), &dest, &self.base, Layout::Reflink, &[])
            .unwrap();
        dest
    }

    fn worktree_list(&self) -> String {
        git(self.root_dir.path(), &["worktree", "list", "--porcelain"])
    }

    fn root_index(&self) -> Vec<u8> {
        std::fs::read(self.root_dir.path().join(".git/index")).unwrap()
    }
}

fn canonical(p: &Path) -> String {
    p.parent()
        .unwrap()
        .canonicalize()
        .unwrap()
        .join(p.file_name().unwrap())
        .display()
        .to_string()
}

#[test]
fn registered_reflink_graft_is_a_git_worktree_of_root() {
    let f = Fixture::new();
    let before = f.root_index();
    let dest = f.reflink("auth");

    let list = f.worktree_list();
    assert!(
        list.contains(&format!("worktree {}\n", canonical(&dest))),
        "{list}"
    );
    assert!(
        list.contains("branch refs/heads/graft-live/auth/live"),
        "{list}"
    );
    assert!(
        !list.contains("locked"),
        "a reflink graft is not locked: {list}"
    );
    assert!(dest.join(".git").is_file(), ".git is a file");
    assert!(!dest.join(".git.graft-private").exists());
    assert_eq!(git(&dest, &["status", "--porcelain"]), "");
    assert_eq!(
        git(&dest, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "graft-live/auth/live"
    );
    assert_eq!(
        git(f.root_dir.path(), &["config", "extensions.worktreeConfig"]),
        "true"
    );
    assert_eq!(before, f.root_index(), "Root's index is untouched");
    assert!(f.root().registration(&name("auth"), &dest).is_some());

    // Commits land in Root at once, on the graft's branch.
    std::fs::write(dest.join("README.md"), "changed\n").unwrap();
    git(&dest, &["commit", "-q", "-am", "feat: change readme"]);
    assert_eq!(
        git(
            f.root_dir.path(),
            &["log", "-1", "--format=%s", "graft-live/auth/live"]
        ),
        "feat: change readme"
    );
    git(f.root_dir.path(), &["fsck", "--no-progress"]);
    // Root's own branch and checkout are untouched.
    assert_eq!(
        git(f.root_dir.path(), &["symbolic-ref", "HEAD"]),
        "refs/heads/main"
    );
    assert_eq!(git(f.root_dir.path(), &["status", "--porcelain"]), "");
}

#[test]
fn graft_repo_reads_a_registered_graft() {
    let f = Fixture::new();
    let dest = f.reflink("auth");
    let repo = GraftRepo::open(&dest).unwrap();
    assert_eq!(repo.registered_name(), Some(name("auth")));
    std::fs::write(dest.join("new.txt"), "x").unwrap();
    let changes = repo.changes_since(&f.base).unwrap();
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(
        repo.uncommitted_paths().unwrap(),
        vec![PathBuf::from("new.txt")]
    );
}

#[test]
fn unintegrated_work_is_what_only_the_graft_branch_holds() {
    let f = Fixture::new();
    let root = f.root();
    let dest = f.reflink("auth");
    let repo = GraftRepo::open(&dest).unwrap();
    assert_eq!(repo.unintegrated(&root).unwrap(), Vec::new());

    std::fs::write(dest.join("README.md"), "changed\n").unwrap();
    git(&dest, &["commit", "-q", "-am", "work"]);
    let lost = repo.unintegrated(&root).unwrap();
    assert_eq!(lost.len(), 1, "{lost:?}");
    assert!(lost[0].in_root);
    let tip = git(&dest, &["rev-parse", "HEAD"]);
    assert_eq!(lost[0].commit.as_str(), tip);

    // Once another branch has it, pruning would lose nothing.
    git(f.root_dir.path(), &["branch", "keep", &tip]);
    assert_eq!(repo.unintegrated(&root).unwrap(), Vec::new());
}

#[test]
fn unregister_leaves_nothing_behind() {
    let f = Fixture::new();
    let root = f.root();
    let dest = f.reflink("auth");
    std::fs::remove_dir_all(&dest).unwrap();
    let outcome = root
        .unregister(&name("auth"), &dest, BranchPolicy::Delete)
        .unwrap();
    assert_eq!(outcome, BranchOutcome::Deleted);
    assert_eq!(f.worktree_list().matches("worktree ").count(), 1);
    let git_dir = f.root_dir.path().join(".git");
    assert!(!git_dir.join("worktrees").exists());
    assert!(!git_dir.join("refs/heads/graft-live").exists());
    assert!(!git_dir.join("logs/refs/heads/graft-live").exists());
    assert_eq!(
        git(f.root_dir.path(), &["branch", "--format=%(refname)"]),
        "refs/heads/main"
    );
    // Idempotent.
    assert_eq!(
        root.unregister(&name("auth"), &dest, BranchPolicy::Delete)
            .unwrap(),
        BranchOutcome::Absent
    );
}

#[test]
fn unregister_keeps_a_branch_with_work_of_its_own_unless_told() {
    let f = Fixture::new();
    let root = f.root();
    let dest = f.reflink("auth");
    std::fs::write(dest.join("README.md"), "changed\n").unwrap();
    git(&dest, &["commit", "-q", "-am", "work"]);
    // As after `git worktree remove`: the files and registration are gone.
    git(
        f.root_dir.path(),
        &["worktree", "remove", &dest.display().to_string()],
    );
    let outcome = root
        .unregister(&name("auth"), &dest, BranchPolicy::KeepIfUnique)
        .unwrap();
    assert!(matches!(outcome, BranchOutcome::Kept { .. }), "{outcome:?}");
    assert!(git(f.root_dir.path(), &["branch"]).contains("graft-live/auth/live"));
    assert_eq!(
        root.unregister(&name("auth"), &dest, BranchPolicy::Delete)
            .unwrap(),
        BranchOutcome::Deleted
    );
}

#[test]
fn unregister_never_deletes_a_branch_checked_out_elsewhere() {
    let f = Fixture::new();
    let root = f.root();
    let dest = f.reflink("auth");
    std::fs::remove_dir_all(&dest).unwrap();
    root.unregister(&name("auth"), &dest, BranchPolicy::KeepIfUnique)
        .unwrap();
    // Root itself checks out a branch named like a graft's.
    git(
        f.root_dir.path(),
        &["checkout", "-q", "-b", "graft-live/auth/live"],
    );
    let outcome = root
        .unregister(&name("auth"), &dest, BranchPolicy::Delete)
        .unwrap();
    assert!(matches!(outcome, BranchOutcome::Kept { .. }), "{outcome:?}");
}

#[test]
fn register_refuses_a_worktree_or_branch_that_is_not_the_grafts() {
    let f = Fixture::new();
    let root = f.root();
    // A user's own worktree that took the id `graft-auth`.
    let theirs = f.home.path().join("theirs").join("graft-auth");
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    git(
        f.root_dir.path(),
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &theirs.display().to_string(),
        ],
    );
    let dest = f.path("auth");
    create_worktree(&root, &dest, &branch_name(&name("auth")), &f.base, &Copier).unwrap();
    let err = root
        .register(&name("auth"), &dest, &f.base, Layout::Reflink, &[])
        .unwrap_err();
    assert!(matches!(err, RootError::AlreadyExists { .. }), "{err}");
    assert!(dest.join(".git").is_dir(), "the graft is left as it was");
    assert!(!git(f.root_dir.path(), &["branch"]).contains("graft-live/auth/live"));

    // A branch with work of its own.
    git(f.root_dir.path(), &["branch", "graft-live/ui/live"]);
    let wt = f.home.path().join("ui-work");
    git(
        f.root_dir.path(),
        &[
            "worktree",
            "add",
            "-q",
            &wt.display().to_string(),
            "graft-live/ui/live",
        ],
    );
    std::fs::write(wt.join("x"), "x").unwrap();
    git(&wt, &["add", "x"]);
    git(&wt, &["commit", "-q", "-m", "theirs"]);
    git(
        f.root_dir.path(),
        &["worktree", "remove", &wt.display().to_string()],
    );
    let tip = git(f.root_dir.path(), &["rev-parse", "graft-live/ui/live"]);
    let ui = f.path("ui");
    create_worktree(&root, &ui, &branch_name(&name("ui")), &f.base, &Copier).unwrap();
    let err = root
        .register(&name("ui"), &ui, &f.base, Layout::Reflink, &[])
        .unwrap_err();
    assert!(err.to_string().contains("graft-live/ui/live"), "{err}");
    assert_eq!(
        git(f.root_dir.path(), &["rev-parse", "graft-live/ui/live"]),
        tip,
        "the branch is untouched"
    );
    assert!(!root.registration_dir(&name("ui")).exists());
}

#[test]
fn register_reuses_a_branch_whose_work_is_on_another_ref() {
    let f = Fixture::new();
    git(f.root_dir.path(), &["branch", "graft-live/auth/live"]);
    let dest = f.reflink("auth");
    assert_eq!(git(&dest, &["rev-parse", "HEAD"]), f.base.as_str());
}

#[test]
fn register_refuses_a_root_that_disables_worktree_config() {
    let f = Fixture::new();
    git(
        f.root_dir.path(),
        &["config", "extensions.worktreeConfig", "false"],
    );
    let root = f.root();
    let dest = f.path("auth");
    create_worktree(&root, &dest, &branch_name(&name("auth")), &f.base, &Copier).unwrap();
    let err = root
        .register(&name("auth"), &dest, &f.base, Layout::Reflink, &[])
        .unwrap_err();
    assert!(err.to_string().contains("worktreeConfig"), "{err}");
    assert!(!root.registration_dir(&name("auth")).exists());
}

#[test]
fn worktree_config_is_enabled_once() {
    let f = Fixture::new();
    f.reflink("one");
    f.reflink("two");
    let config = std::fs::read_to_string(f.root_dir.path().join(".git/config")).unwrap();
    assert_eq!(config.matches("worktreeConfig").count(), 1, "{config}");
    assert!(!f.root_dir.path().join(".git/config.lock").exists());
}

#[test]
fn layers_are_ignored_in_the_graft_only_and_global_excludes_are_kept() {
    let f = Fixture::new();
    let excludes = f.home.path().join("excludes");
    std::fs::write(&excludes, "*.log\n").unwrap();
    git(
        f.root_dir.path(),
        &[
            "config",
            "core.excludesFile",
            &excludes.display().to_string(),
        ],
    );
    let dest = f.reflink("auth");
    exclude_paths(&dest, &[Path::new("node_modules")]).unwrap();
    for dir in [&dest, &f.root_dir.path().to_owned()] {
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        std::fs::write(dir.join("node_modules/x.js"), "x").unwrap();
        std::fs::write(dir.join("debug.log"), "x").unwrap();
    }
    assert_eq!(git(&dest, &["status", "--porcelain"]), "");
    assert_eq!(
        git(f.root_dir.path(), &["status", "--porcelain"]),
        "?? node_modules/",
        "Root does not ignore the graft's layers"
    );
    let ignored = GraftRepo::open(&dest)
        .unwrap()
        .ignored(&[PathBuf::from("node_modules/x.js")])
        .unwrap();
    assert_eq!(ignored.len(), 1, "gix agrees: {ignored:?}");
}

#[test]
fn virtual_registration_is_locked_and_built_on_first_use() {
    let f = Fixture::new();
    let root = f.root();
    let before = f.root_index();
    let mount = f.path("v");
    let admin = root
        .register(
            &name("v"),
            &mount,
            &f.base,
            Layout::Virtual,
            &[Path::new("node_modules")],
        )
        .unwrap();
    assert!(!admin.join("index").exists(), "the index is built lazily");
    let list = f.worktree_list();
    assert!(
        list.contains("branch refs/heads/graft-live/v/live"),
        "{list}"
    );
    assert!(list.contains("locked graft: virtual graft"), "{list}");
    let config = std::fs::read_to_string(admin.join("config.worktree")).unwrap();
    assert!(config.contains("checkStat = minimal"), "{config}");
    assert!(config.contains("excludesFile"), "{config}");

    // git refuses to remove or prune it, even with nothing mounted.
    let removed = run_git(
        f.root_dir.path(),
        &["worktree", "remove", &mount.display().to_string()],
    );
    assert!(!removed.status.success());
    git(f.root_dir.path(), &["worktree", "prune"]);
    assert!(admin.join("gitdir").is_file(), "prune kept it");

    let link = f.home.path().join("dotgit");
    prepare_virtual_git(&root, &admin, &link, &f.base).unwrap();
    assert!(admin.join("index").is_file());
    let text = std::fs::read_to_string(&link).unwrap();
    assert_eq!(
        text,
        format!("gitdir: {}\n", admin.canonicalize().unwrap().display())
    );
    assert_eq!(before, f.root_index(), "Root's index is untouched");
    // Built once: a second call keeps what is there.
    let index = std::fs::read(admin.join("index")).unwrap();
    prepare_virtual_git(&root, &admin, &link, &f.base).unwrap();
    assert_eq!(index, std::fs::read(admin.join("index")).unwrap());

    // The index describes the base tree.
    assert_eq!(
        git(
            f.root_dir.path(),
            &["--git-dir", &admin.display().to_string(), "ls-files",]
        ),
        "README.md\nsrc/lib.rs"
    );
}
