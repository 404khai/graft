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

/// A graft home with a space (the macOS default) makes doctor say how to
/// quote graft paths; one without says nothing (ADR 0022).
#[test]
fn warns_when_graft_paths_contain_a_space() {
    let base = tempfile::tempdir().unwrap();
    let repo = fake_repo();
    let spaced = base.path().join("Application Support").join("graft");
    let stdout = doctor_stdout(&spaced, repo.path());
    assert!(
        line(&stdout, "spaces").contains("cd \"$(graft path NAME)\""),
        "{stdout}"
    );
    let plain = doctor_stdout(&base.path().join("graft"), repo.path());
    assert!(!plain.contains("spaces"), "{plain}");
}

#[test]
fn json_has_the_same_findings() {
    let home = tempfile::tempdir().unwrap();
    let repo = fake_repo();
    let out = graft(home.path())
        .args(["doctor", "--json"])
        .arg(repo.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["healthy"], true);
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    for key in [
        "platform",
        "repository",
        "reflink",
        "graft_home",
        "fuse",
        "sandbox",
    ] {
        assert!(v[key].is_string(), "{key} missing from {v}");
    }
}

/// The bug-report bundle: findings, Roots, graftd's log and crash files,
/// with secret-looking values redacted.
#[test]
fn report_bundles_diagnostics_and_redacts_secrets() {
    let home = tempfile::tempdir().unwrap();
    let repo = crate::git_repo();
    graft(home.path())
        .arg("init")
        .current_dir(repo.path())
        .assert()
        .success();
    std::fs::write(
        home.path().join("config.toml"),
        "[log]\nfilter = \"warn\"\n",
    )
    .unwrap();
    // Placeholders made at run time, so no credential-looking literal
    // sits in the source for secret scanners to flag.
    let fake = |n: u32| format!("placeholder{n}x{}", std::process::id());
    let (log_value, crash_value, env_value) = (fake(1), fake(2), fake(3));
    let run = home.path().join("run");
    std::fs::create_dir_all(&run).unwrap();
    std::fs::write(
        run.join("graftd.log"),
        format!("old\nmounted ok\nAPI_TOKEN={log_value} next\n"),
    )
    .unwrap();
    graft_core::crash::write_crash(
        &graft_core::crash::crash_dir(&graft_core::GraftHome::from_path(home.path()).unwrap()),
        "graftd",
        "0.0.0",
        &format!("boom with password={crash_value}"),
        "x.rs:1",
        "bt",
    )
    .unwrap();

    let out = graft(home.path())
        .args(["doctor", "--json", "--report"])
        .current_dir(repo.path())
        .env("GRAFT_SECRET_KEY", &env_value)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let path = std::path::PathBuf::from(v["report"].as_str().unwrap());
    assert!(path.starts_with(home.path().join("reports")), "{path:?}");
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "report must be private: {mode:o}");
    }
    let text = std::fs::read_to_string(&path).unwrap();
    for secret in [&log_value, &crash_value, &env_value] {
        assert!(!text.contains(secret), "{secret} leaked into the report");
    }
    let b: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(b["bundle_version"], 1);
    assert_eq!(b["doctor"]["healthy"], true);
    assert_eq!(b["environment"]["GRAFT_SECRET_KEY"], "<redacted>");
    assert!(b["config"].as_str().unwrap().contains("filter = \"warn\""));
    let roots = b["roots"].as_array().unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0]["integrity"], "ok");
    assert_eq!(
        roots[0]["schema_version"],
        roots[0]["supported_schema_version"]
    );
    assert_eq!(b["daemon"]["status"]["running"], false);
    let log = b["daemon"]["log_tail"].as_str().unwrap();
    assert!(
        log.contains("mounted ok") && log.contains("API_TOKEN=<redacted> next"),
        "{log}"
    );
    let crashes = b["crashes"].as_array().unwrap();
    assert_eq!(crashes.len(), 1);
    assert!(
        crashes[0]["text"]
            .as_str()
            .unwrap()
            .contains("password=<redacted>")
    );
}

#[test]
fn report_can_be_written_to_a_chosen_file() {
    let home = tempfile::tempdir().unwrap();
    let repo = fake_repo();
    let dest = tempfile::tempdir().unwrap();
    let file = dest.path().join("bug.json");
    graft(home.path())
        .args(["doctor", "--report-to"])
        .arg(&file)
        .arg(repo.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("report written to"));
    let b: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(b["roots"], serde_json::json!([]));
}
