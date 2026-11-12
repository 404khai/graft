//! `graft materialize` and `graft import` (AGENTS.md Phase 16): a graft
//! written out, edited as plain files, and imported back gives exactly the
//! expected delta.

// Short names for the fixture, its grafts and the export directory.
#![allow(clippy::many_single_char_names)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{git, git_repo, graft};

struct Fixture {
    home: tempfile::TempDir,
    repo: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let repo = git_repo();
        let r = repo.path();
        git(r, &["config", "user.name", "Graft Test"]);
        git(r, &["config", "user.email", "test@graft.invalid"]);
        for (p, c) in [
            ("keep.txt", "keep\n"),
            ("edit.txt", "edit\n"),
            ("gone.txt", "gone\n"),
            ("script.sh", "echo hi\n"),
            ("dir/nested.txt", "nested\n"),
        ] {
            std::fs::create_dir_all(r.join(p).parent().unwrap()).unwrap();
            std::fs::write(r.join(p), c).unwrap();
        }
        std::fs::write(r.join(".gitignore"), "ignored/\n").unwrap();
        git(r, &["add", "-A"]);
        git(r, &["commit", "-qm", "files"]);
        let f = Self { home, repo };
        f.ok(&["init"]);
        f
    }

    fn cmd(&self) -> assert_cmd::Command {
        let mut c = graft(self.home.path());
        c.current_dir(self.repo.path())
            .env_remove("GRAFT_FS")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        c
    }

    fn ok(&self, args: &[&str]) -> String {
        let o = self.cmd().args(args).output().unwrap();
        assert!(
            o.status.success(),
            "graft {args:?}: {}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8(o.stdout).unwrap()
    }

    fn new_graft(&self, name: &str) -> PathBuf {
        self.ok(&["new", name, "--backend", "reflink"]);
        PathBuf::from(self.ok(&["path", name]).trim())
    }
}

/// `git status --porcelain` of a graft, sorted.
fn status(dir: &Path) -> Vec<String> {
    let o = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(dir)
        .output()
        .unwrap();
    let mut v: Vec<String> = String::from_utf8(o.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    v.sort();
    v
}

#[test]
fn materialize_edit_and_import_round_trips() {
    let f = Fixture::new();
    let a = f.new_graft("a");
    std::fs::write(a.join("edit.txt"), "edited in the graft\n").unwrap();
    std::fs::write(a.join("new.txt"), "new\n").unwrap();
    std::fs::create_dir(a.join("ignored")).unwrap();
    std::fs::write(a.join("ignored/cache"), "not exported\n").unwrap();

    // Out: a plain directory with the graft's files, no .git, no ignored files.
    let out = tempfile::tempdir().unwrap();
    let d = out.path().join("export");
    f.ok(&["materialize", "a", "--dir", d.to_str().unwrap()]);
    assert_eq!(
        std::fs::read_to_string(d.join("edit.txt")).unwrap(),
        "edited in the graft\n"
    );
    assert_eq!(std::fs::read_to_string(d.join("new.txt")).unwrap(), "new\n");
    assert_eq!(
        std::fs::read_to_string(d.join("dir/nested.txt")).unwrap(),
        "nested\n"
    );
    assert!(!d.join(".git").exists() && !d.join("ignored").exists());

    // Edit it as plain files.
    std::fs::write(d.join("keep.txt"), "changed outside\n").unwrap();
    std::fs::remove_file(d.join("gone.txt")).unwrap();
    std::fs::create_dir(d.join("added")).unwrap();
    std::fs::write(d.join("added/file.txt"), "added\n").unwrap();
    std::fs::set_permissions(d.join("script.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink("keep.txt", d.join("link")).unwrap();
    let expected = [
        " D gone.txt",
        " M edit.txt",
        " M keep.txt",
        " M script.sh",
        "?? added/file.txt",
        "?? link",
        "?? new.txt",
    ];

    // In: into a fresh graft, and back into the original.
    for (name, graft_dir) in [("b", f.new_graft("b")), ("a", a.clone())] {
        f.ok(&["import", name, "--dir", d.to_str().unwrap()]);
        assert_eq!(status(&graft_dir), expected, "graft {name}");
        assert_eq!(
            std::fs::read_to_string(graft_dir.join("keep.txt")).unwrap(),
            "changed outside\n"
        );
        assert_eq!(
            std::fs::read_link(graft_dir.join("link")).unwrap(),
            PathBuf::from("keep.txt")
        );
        let diff = f.ok(&["diff", name]);
        assert!(
            diff.contains("+changed outside") && diff.contains("-gone"),
            "{diff}"
        );
    }
    assert!(
        a.join("ignored/cache").exists(),
        "import touched an ignored file"
    );

    // Importing an unchanged export changes nothing.
    let again = out.path().join("again");
    f.ok(&["materialize", "a", "--dir", again.to_str().unwrap()]);
    assert!(
        f.ok(&["import", "a", "--dir", again.to_str().unwrap()])
            .contains("0 written, 0 removed")
    );

    // A branch carries the same tree, and imports to the same delta.
    f.ok(&["materialize", "a", "--branch", "exported"]);
    let o = Command::new("git")
        .args(["show", "exported:keep.txt"])
        .current_dir(f.repo.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout), "changed outside\n");
    let c = f.new_graft("c");
    f.ok(&["import", "c", "--branch", "exported"]);
    assert_eq!(status(&c), expected);
    let fsck = Command::new("git")
        .args(["fsck", "--strict"])
        .current_dir(f.repo.path())
        .status()
        .unwrap();
    assert!(fsck.success());
}

#[test]
fn materialize_never_overwrites() {
    let f = Fixture::new();
    f.new_graft("a");
    let out = tempfile::tempdir().unwrap();
    std::fs::write(out.path().join("precious"), "mine\n").unwrap();
    let o = f
        .cmd()
        .args(["materialize", "a", "--dir"])
        .arg(out.path())
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("never overwrites"));
    assert_eq!(
        std::fs::read_to_string(out.path().join("precious")).unwrap(),
        "mine\n"
    );
    f.ok(&["materialize", "a", "--branch", "x"]);
    let o = f
        .cmd()
        .args(["materialize", "a", "--branch", "x"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("already exists"));
    assert!(
        !f.cmd()
            .args(["materialize", "a"])
            .output()
            .unwrap()
            .status
            .success()
    );
}
