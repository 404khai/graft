//! `graft init` and `graft root`.

use std::path::Path;
use std::process::{Command, Stdio};

use graft_core::GraftHome;
use predicates::prelude::*;

use crate::{git, git_repo, graft};

fn stdout_of(cmd: &mut assert_cmd::Command) -> String {
    String::from_utf8(cmd.assert().success().get_output().stdout.clone()).unwrap()
}

fn root_id(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|l| l.strip_prefix("root"))
        .map_or_else(
            || panic!("no root line in:\n{stdout}"),
            |v| v.trim().to_owned(),
        )
}

#[test]
fn init_registers_then_is_idempotent() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    let top = repo.path().canonicalize().unwrap();

    let first = stdout_of(graft(home.path()).arg("init").current_dir(repo.path()));
    assert!(first.starts_with("Registered Root "), "{first}");
    assert!(first.contains(&top.display().to_string()), "{first}");

    let second = stdout_of(graft(home.path()).arg("init").current_dir(repo.path()));
    assert!(second.contains("is already registered as Root"), "{second}");

    let roots: Vec<_> = std::fs::read_dir(home.path().join("roots"))
        .unwrap()
        .collect();
    assert_eq!(roots.len(), 1, "init twice must leave one Root");
}

#[test]
fn init_refuses_a_directory_that_is_not_a_repository() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    graft(home.path())
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::starts_with("graft: not a git repository: "));
    assert!(
        !home.path().join("roots").exists(),
        "nothing written on refusal"
    );
}

#[test]
fn init_refuses_an_empty_repository() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    graft(home.path())
        .arg("init")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("has no commits yet"));
}

#[test]
fn init_refuses_an_invalid_config() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("config.toml"), "bogus = 1\n").unwrap();
    let repo = git_repo();
    graft(home.path())
        .arg("init")
        .arg(repo.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid config file"));
}

#[test]
fn root_shows_the_registered_root_from_anywhere_in_the_repo() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    let sub = repo.path().join("deep/dir");
    std::fs::create_dir_all(&sub).unwrap();
    graft(home.path())
        .arg("init")
        .arg(repo.path())
        .assert()
        .success();

    let out = stdout_of(graft(home.path()).arg("root").current_dir(&sub));
    for label in [
        "root",
        "path",
        "first commit",
        "head",
        "registered",
        "grafts",
        "schema",
        "database",
    ] {
        assert!(
            out.lines().any(|l| l.starts_with(label)),
            "no {label} in:\n{out}"
        );
    }
    assert!(out.contains("(main)"), "{out}");
    assert!(out.contains("schema        v1"), "{out}");
    assert!(out.contains("grafts        0"), "{out}");
    let id = root_id(&out);
    assert!(
        home.path()
            .join("roots")
            .join(&id)
            .join("graft.db")
            .is_file()
    );
}

#[test]
fn root_reports_an_unregistered_repository() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    graft(home.path())
        .arg("root")
        .arg(repo.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "is not registered as a Root; run `graft init` first",
        ));
}

fn spawn(home: &Path, args: &[&str], dir: &Path) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_graft"))
        .args(args)
        .current_dir(dir)
        .env("GRAFT_HOME", home)
        .env_remove("GRAFT_LOG")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// AGENTS.md Phase 1: concurrent CLI invocations do not corrupt the DB.
#[test]
fn parallel_inits_register_each_root_once_without_corruption() {
    const PER_REPO: usize = 12;
    let home = tempfile::tempdir().unwrap();
    let repos = [git_repo(), git_repo()];

    let children: Vec<_> = (0..PER_REPO)
        .flat_map(|_| {
            repos
                .iter()
                .map(|r| spawn(home.path(), &["init"], r.path()))
        })
        .collect();
    let outputs: Vec<String> = children
        .into_iter()
        .map(|c| {
            let out = c.wait_with_output().unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap()
        })
        .collect();
    let created = outputs
        .iter()
        .filter(|o| o.starts_with("Registered Root"))
        .count();
    assert_eq!(
        created,
        repos.len(),
        "each Root registered exactly once:\n{outputs:#?}"
    );

    let home = GraftHome::from_path(home.path()).unwrap();
    let roots = graft_store::registry::list_roots(&home).unwrap();
    assert_eq!(roots.len(), repos.len());
    for (record, store) in &roots {
        assert_eq!(
            store.integrity_check().unwrap(),
            Vec::<String>::new(),
            "{}",
            record.id
        );
        assert_eq!(store.schema_version().unwrap(), graft_store::SCHEMA_VERSION);
    }

    // Readers and writers together, after registration.
    let mixed: Vec<_> = (0..PER_REPO)
        .flat_map(|i| {
            let args: &[&str] = if i % 2 == 0 { &["root"] } else { &["init"] };
            repos.iter().map(move |r| (args, r))
        })
        .map(|(args, r)| spawn(home.path(), args, r.path()))
        .collect();
    for c in mixed {
        let out = c.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
