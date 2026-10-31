use std::path::Path;

use predicates::prelude::*;

use crate::{fake_repo, graft};

fn doctor_stdout(home: &Path, repo: &Path) -> String {
    let out = graft(home)
        .args(["doctor"])
        .arg(repo)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap()
}

fn line<'a>(stdout: &'a str, label: &str) -> &'a str {
    stdout
        .lines()
        .find(|l| l.trim_start().starts_with(label))
        .unwrap_or_else(|| panic!("no {label:?} line in:\n{stdout}"))
}

#[test]
fn reports_every_section() {
    let home = tempfile::tempdir().unwrap();
    let repo = fake_repo();
    let stdout = doctor_stdout(home.path(), repo.path());
    for label in [
        "platform",
        "repository",
        "filesystem",
        "reflink",
        "graft home",
        "same fs",
        "config",
        "fuse",
        "nfs client",
    ] {
        line(&stdout, label);
    }
    assert!(line(&stdout, "platform").contains(std::env::consts::OS));
    let top = repo.path().canonicalize().unwrap();
    assert!(line(&stdout, "repository").contains(&top.display().to_string()));
    assert!(line(&stdout, "config").contains("not present, using defaults"));
}

/// The acceptance check for reflink reporting: what doctor prints matches
/// the filesystem it runs on. CI runs this on APFS (macOS) and ext4 (Ubuntu).
#[test]
fn reflink_report_matches_the_filesystem() {
    let home = tempfile::tempdir().unwrap();
    let repo = fake_repo();
    let stdout = doctor_stdout(home.path(), repo.path());
    let fs = graft_fs::filesystem_type(repo.path()).unwrap();
    assert!(line(&stdout, "filesystem").ends_with(fs.name()), "{stdout}");

    let reflink = line(&stdout, "reflink");
    let probed = graft_fs::probe_reflink(repo.path()).unwrap();
    let expected = if probed { "supported" } else { "not supported" };
    assert!(
        reflink.contains(&format!("reflink     {expected}")),
        "{stdout}"
    );
    if let Some(known) = fs.reflink_expected() {
        assert_eq!(probed, known, "{fs}: {reflink}");
    }
}

#[test]
fn leaves_no_trace_in_the_repository_or_graft_home() {
    let home = tempfile::tempdir().unwrap();
    let not_created = home.path().join("graft");
    let repo = fake_repo();
    std::fs::write(repo.path().join("README.md"), "hi\n").unwrap();
    let stdout = doctor_stdout(&not_created, repo.path());
    assert!(line(&stdout, "graft home").contains("not created yet"));
    assert!(!not_created.exists(), "doctor must not create $GRAFT_HOME");
    let entries = |p: &Path| std::fs::read_dir(p).unwrap().count();
    assert_eq!(entries(repo.path()), 2, ".git and README.md only");
    assert_eq!(
        entries(&repo.path().join(".git")),
        0,
        "probe left files in .git"
    );
}

#[test]
fn finds_the_repository_from_a_subdirectory() {
    let home = tempfile::tempdir().unwrap();
    let repo = fake_repo();
    let sub = repo.path().join("src/nested");
    std::fs::create_dir_all(&sub).unwrap();
    let stdout = doctor_stdout(home.path(), &sub);
    let top = repo.path().canonicalize().unwrap();
    assert_eq!(
        line(&stdout, "repository"),
        format!("repository    {}", top.display())
    );
}

#[test]
fn notes_a_directory_that_is_not_a_repository() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let stdout = doctor_stdout(home.path(), dir.path());
    assert!(line(&stdout, "repository").contains("not a git repository"));
}

#[test]
fn fails_on_an_invalid_config() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("config.toml"), "[log]\nlevel = 3\n").unwrap();
    let repo = fake_repo();
    graft(home.path())
        .arg("doctor")
        .arg(repo.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("error: invalid config file"))
        .stdout(predicate::str::contains("line 2"));
}

#[test]
fn reports_a_loaded_config() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("config.toml"),
        "[log]\nfilter = \"info\"\n",
    )
    .unwrap();
    let repo = fake_repo();
    let stdout = doctor_stdout(home.path(), repo.path());
    assert!(line(&stdout, "config").contains("(loaded)"));
}

#[test]
fn fails_on_a_relative_graft_home() {
    let repo = fake_repo();
    graft(Path::new("relative/home"))
        .arg("doctor")
        .arg(repo.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("must be an absolute path"));
}

#[test]
fn fails_on_a_missing_path() {
    let home = tempfile::tempdir().unwrap();
    graft(home.path())
        .args(["doctor", "/definitely/not/here"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("cannot inspect"));
}
