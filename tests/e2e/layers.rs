//! Layers: `graft.toml` `[[layer]]`, `graft trust` and `graft gc`.
//!
//! Most tests use a stand-in build command that logs each run, so they can
//! count builds exactly without npm. `node_fixture_*` uses real npm.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use graft_core::GraftHome;
use predicates::prelude::*;

use crate::{git, graft};

/// A Root whose graft.toml declares one `deps` layer built from `deps.lock`.
struct Fixture {
    home: tempfile::TempDir,
    repo: tempfile::TempDir,
    log: PathBuf,
}

const BUILD: &str =
    r#"mkdir deps && cp deps.lock deps/copy && echo "$GRAFT_NAME" >> "$GRAFT_E2E_BUILD_LOG""#;

fn graft_toml(build: &str, toolchain: Option<&str>) -> String {
    let mut s =
        format!("[[layer]]\npath = \"deps\"\ninputs = [\"deps.lock\"]\nbuild = '{build}'\n");
    if let Some(t) = toolchain {
        let _ = writeln!(s, "toolchain = '{t}'");
    }
    s
}

impl Fixture {
    fn new() -> Self {
        Self::with_toml(&graft_toml(BUILD, None))
    }

    fn with_toml(toml: &str) -> Self {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let p = repo.path();
        git(p, &["init", "-q", "-b", "main"]);
        std::fs::write(p.join("graft.toml"), toml).unwrap();
        std::fs::write(p.join("deps.lock"), "v1\n").unwrap();
        std::fs::write(p.join(".gitignore"), "deps/\nbuild.log\n").unwrap();
        git(p, &["add", "-A"]);
        git(p, &["commit", "-q", "-m", "layers"]);
        let log = home.path().join("build.log");
        let f = Self { home, repo, log };
        f.run(&["init"]).success();
        f
    }

    fn cmd(&self, args: &[&str]) -> assert_cmd::Command {
        let mut c = graft(self.home.path());
        c.args(args)
            .current_dir(self.repo.path())
            .env("GRAFT_E2E_BUILD_LOG", &self.log);
        c
    }

    fn run(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        self.cmd(args).assert()
    }

    fn stdout(&self, args: &[&str]) -> String {
        String::from_utf8(self.run(args).success().get_output().stdout.clone()).unwrap()
    }

    fn path(&self, name: &str) -> PathBuf {
        PathBuf::from(self.stdout(&["path", name]).trim_end())
    }

    fn builds(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn commit(&self, file: &str, contents: &str) {
        std::fs::write(self.repo.path().join(file), contents).unwrap();
        git(self.repo.path(), &["add", "-A"]);
        git(self.repo.path(), &["commit", "-q", "-m", file]);
    }

    fn home(&self) -> GraftHome {
        GraftHome::from_path(self.home.path()).unwrap()
    }

    fn store(&self) -> graft_store::Store {
        graft_store::registry::list_roots(&self.home())
            .unwrap()
            .pop()
            .unwrap()
            .1
    }

    fn cache_dirs(&self) -> Vec<String> {
        let root = graft_store::registry::list_roots(&self.home())
            .unwrap()
            .pop()
            .unwrap()
            .0;
        let dir = self.home().root_dir(&root.id).join("layers");
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|r| {
                r.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| Path::new(n).extension().is_none_or(|e| e != "lock"))
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

#[test]
fn unapproved_builds_are_refused_before_anything_is_created() {
    let f = Fixture::new();
    f.run(&["new", "a"])
        .failure()
        .stderr(predicate::str::contains(
            "have not been approved, so nothing was created",
        ))
        .stderr(predicate::str::contains("deps: build `mkdir deps"))
        .stderr(predicate::str::contains("`graft trust`"));
    assert!(f.builds().is_empty(), "an unapproved build ran");
    assert_eq!(f.store().grafts().unwrap().len(), 0);
    assert_eq!(f.cache_dirs().len(), 0);
}

/// AGENTS.md Phase 3: the second and later `graft new` calls do not run the
/// install command.
#[test]
fn builds_once_and_reuses_the_layer() {
    let f = Fixture::new();
    f.stdout(&["trust"]);
    let a = f.stdout(&["new", "a"]);
    assert!(a.contains("layer deps: built"), "{a}");
    let b = f.stdout(&["new", "b"]);
    assert!(b.contains("layer deps: reused"), "{b}");
    f.stdout(&["new", "c"]);
    assert_eq!(f.builds(), ["a"], "only the first graft may build");
    for g in ["a", "b", "c"] {
        assert_eq!(
            std::fs::read_to_string(f.path(g).join("deps/copy")).unwrap(),
            "v1\n"
        );
    }
    assert_eq!(f.cache_dirs().len(), 1);
}

#[test]
fn writes_to_a_layer_stay_private_to_the_graft() {
    let f = Fixture::new();
    f.stdout(&["trust"]);
    f.stdout(&["new", "a"]);
    f.stdout(&["new", "b"]);
    std::fs::write(f.path("a").join("deps/copy"), "changed in a\n").unwrap();
    std::fs::write(f.path("a").join("deps/extra"), "x").unwrap();
    assert_eq!(
        std::fs::read_to_string(f.path("b").join("deps/copy")).unwrap(),
        "v1\n"
    );
    assert!(!f.path("b").join("deps/extra").exists());
    f.stdout(&["new", "c"]);
    assert_eq!(
        std::fs::read_to_string(f.path("c").join("deps/copy")).unwrap(),
        "v1\n"
    );
}

#[test]
fn layers_are_not_changes_and_do_not_block_prune() {
    let f = Fixture::new();
    f.stdout(&["trust"]);
    f.stdout(&["new", "a"]);
    std::fs::write(f.path("a").join("deps/copy"), "edited\n").unwrap();
    let status = f.stdout(&["status", "a"]);
    assert!(status.contains("nothing changed since base"), "{status}");
    let exclude = std::fs::read_to_string(f.path("a").join(".git/info/exclude")).unwrap();
    assert!(exclude.contains("/deps/"), "{exclude}");
    f.run(&["prune", "a"]).success();
}

/// AGENTS.md Phase 3: changing the lockfile produces a new layer; the old
/// one is kept while referenced.
#[test]
fn a_lockfile_change_makes_a_new_layer_and_keeps_the_old() {
    let f = Fixture::new();
    f.stdout(&["trust"]);
    f.stdout(&["new", "old"]);
    f.commit("deps.lock", "v2\n");
    let out = f.stdout(&["new", "new"]);
    assert!(out.contains("layer deps: built"), "{out}");
    assert_eq!(f.builds(), ["old", "new"]);
    assert_eq!(f.cache_dirs().len(), 2);
    assert_eq!(
        std::fs::read_to_string(f.path("old").join("deps/copy")).unwrap(),
        "v1\n"
    );
    assert_eq!(
        std::fs::read_to_string(f.path("new").join("deps/copy")).unwrap(),
        "v2\n"
    );

    // Both referenced: gc keeps both.
    let gc = f.stdout(&["gc"]);
    assert!(
        gc.starts_with("Removed 0 layer(s)") && gc.contains("Kept 2 in use"),
        "{gc}"
    );
    assert_eq!(f.cache_dirs().len(), 2);

    // Pruning the old graft releases the old layer; gc removes only it.
    f.run(&["prune", "old"]).success();
    let gc = f.stdout(&["gc"]);
    assert!(
        gc.starts_with("Removed 1 layer(s)") && gc.contains("Kept 1 in use"),
        "{gc}"
    );
    assert_eq!(f.cache_dirs().len(), 1);
    assert_eq!(
        std::fs::read_to_string(f.path("new").join("deps/copy")).unwrap(),
        "v2\n"
    );
    // A graft of the old lockfile now needs a rebuild.
    f.stdout(&["new", "again", "--from", "HEAD~1"]);
    assert_eq!(f.builds(), ["old", "new", "again"]);
}

/// AGENTS.md Phase 3: `graft gc` never removes a layer referenced by a live
/// graft.
#[test]
fn gc_never_removes_a_referenced_layer() {
    let f = Fixture::new();
    f.stdout(&["trust"]);
    f.stdout(&["new", "live"]);
    f.run(&["gc", "--dry-run"])
        .success()
        .stdout(predicate::str::starts_with("Would remove 0 layer(s)"));
    for _ in 0..3 {
        f.stdout(&["gc"]);
    }
    assert_eq!(f.cache_dirs().len(), 1);
    let key = f.store().layers().unwrap()[0].key;
    assert_eq!(f.store().layer_ref_count(&key).unwrap(), 1);
    // A new graft still reuses it.
    f.stdout(&["new", "second"]);
    assert_eq!(f.builds(), ["live"]);
}

#[test]
fn gc_racing_graft_new_never_strands_a_graft() {
    let f = Fixture::new();
    f.stdout(&["trust"]);
    let spawn = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_graft"))
            .args(args)
            .current_dir(f.repo.path())
            .env("GRAFT_HOME", f.home.path())
            .env("GRAFT_E2E_BUILD_LOG", &f.log)
            .env_remove("GRAFT_LOG")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let mut children = Vec::new();
    for i in 0..6 {
        children.push(spawn(&["new", &format!("g{i}")]));
        children.push(spawn(&["gc"]));
    }
    for c in children {
        let out = c.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    // Every graft got its layer, and the cache still holds what they use.
    let store = f.store();
    let layers = store.layers().unwrap();
    assert_eq!(layers.len(), 1, "{layers:?}");
    assert_eq!(layers[0].refs, 6);
    assert_eq!(f.cache_dirs().len(), 1);
    assert!(f.builds().len() <= 6);
    for i in 0..6 {
        assert_eq!(
            std::fs::read_to_string(f.path(&format!("g{i}")).join("deps/copy")).unwrap(),
            "v1\n"
        );
    }
}

#[test]
fn changing_a_command_requires_new_approval() {
    let f = Fixture::new();
    f.stdout(&["trust"]);
    f.stdout(&["new", "a"]);
    let edited = graft_toml(&format!("{BUILD} && touch deps/sneaky"), None);
    f.commit("graft.toml", &edited);
    f.run(&["new", "b"])
        .failure()
        .stderr(predicate::str::contains("have not been approved"))
        .stderr(predicate::str::contains("touch deps/sneaky"));
    assert_eq!(f.builds(), ["a"]);
    let trust = f.stdout(&["trust"]);
    assert!(trust.starts_with("Approved deps:"), "{trust}");
    assert!(
        f.stdout(&["trust"]).starts_with("Already approved"),
        "idempotent"
    );
    f.stdout(&["new", "b"]);
    assert!(f.path("b").join("deps/sneaky").exists());
}

#[test]
fn toolchain_commands_are_gated_and_part_of_the_key() {
    let toolchain = r#"echo toolchain >> "$GRAFT_E2E_BUILD_LOG"; cat toolchain-version"#;
    let f = Fixture::with_toml(&graft_toml(BUILD, Some(toolchain)));
    std::fs::write(f.repo.path().join("toolchain-version"), "1.0\n").unwrap();
    f.run(&["new", "a"]).failure();
    assert!(f.builds().is_empty(), "an unapproved toolchain command ran");
    f.stdout(&["trust"]);
    f.stdout(&["new", "a"]);
    f.stdout(&["new", "b"]);
    assert_eq!(f.builds(), ["toolchain", "a", "toolchain"]);
    // A different toolchain output is a different layer.
    std::fs::write(f.repo.path().join("toolchain-version"), "2.0\n").unwrap();
    let out = f.stdout(&["new", "c"]);
    assert!(out.contains("layer deps: built"), "{out}");
    assert_eq!(f.cache_dirs().len(), 2);
}

#[test]
fn a_failing_build_leaves_nothing_behind() {
    let f = Fixture::with_toml(&graft_toml("echo building >&2; exit 3", None));
    f.stdout(&["trust"]);
    f.run(&["new", "broken"])
        .failure()
        .stderr(predicate::str::contains("building"))
        .stderr(predicate::str::contains(
            "layer deps: build `echo building >&2; exit 3` failed",
        ));
    assert_eq!(f.store().grafts().unwrap().len(), 0);
    assert_eq!(f.cache_dirs().len(), 0);
    assert_eq!(git_refs(f.repo.path()), "", "pin ref left behind");
}

#[test]
fn a_build_that_creates_nothing_is_an_error() {
    let f = Fixture::with_toml(&graft_toml("true", None));
    f.stdout(&["trust"]);
    f.run(&["new", "empty"])
        .failure()
        .stderr(predicate::str::contains(
            "did not create the directory deps",
        ));
    assert_eq!(f.store().grafts().unwrap().len(), 0);
}

#[test]
fn tracked_layer_paths_and_missing_inputs_are_rejected() {
    let f = Fixture::new();
    std::fs::create_dir(f.repo.path().join("deps")).unwrap();
    std::fs::write(f.repo.path().join("deps/tracked"), "x").unwrap();
    git(f.repo.path(), &["add", "-f", "deps/tracked"]);
    git(f.repo.path(), &["commit", "-q", "-m", "track deps"]);
    f.stdout(&["trust"]);
    f.run(&["new", "a"])
        .failure()
        .stderr(predicate::str::contains("layer deps is tracked in git"));

    let g = Fixture::with_toml(
        "[[layer]]\npath = \"deps\"\ninputs = [\"missing.lock\"]\nbuild = \"true\"\n",
    );
    g.stdout(&["trust"]);
    g.run(&["new", "a"])
        .failure()
        .stderr(predicate::str::contains(
            "input missing.lock does not exist",
        ));
}

#[test]
fn invalid_graft_toml_reports_file_and_line() {
    let f =
        Fixture::with_toml("[[layer]]\npath = \"../escape\"\ninputs = [\"a\"]\nbuild = \"b\"\n");
    f.run(&["new", "a"])
        .failure()
        .stderr(predicate::str::contains(
            "graft.toml:2:8: layer path must not contain . or .. components",
        ));
}

#[test]
fn repositories_without_graft_toml_are_unaffected() {
    let f = Fixture::new();
    git(f.repo.path(), &["rm", "-q", "graft.toml"]);
    git(f.repo.path(), &["commit", "-q", "-m", "no blueprint"]);
    let out = f.stdout(&["new", "plain"]);
    assert!(!out.contains("layer"), "{out}");
    f.run(&["trust"])
        .success()
        .stdout(predicate::str::contains("nothing to approve"));
}

fn git_refs(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["for-each-ref", "refs/graft"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap()
}

/// AGENTS.md Phase 3 with real npm: the second graft does not run the
/// install, and reaches a passing `npm test` quickly (target < 3 s). Skipped
/// without npm, unless `GRAFT_E2E_REQUIRE_NPM` is set (CI sets it).
#[test]
fn node_fixture_reuses_node_modules_and_passes_npm_test_quickly() {
    if Command::new("npm").arg("--version").output().is_err() {
        assert!(
            std::env::var_os("GRAFT_E2E_REQUIRE_NPM").is_none(),
            "npm is required but missing"
        );
        return;
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/make-fixtures.sh");
    let fixtures = tempfile::tempdir().unwrap();
    let work = fixtures.path().join("tests/fixtures");
    std::fs::create_dir_all(&work).unwrap();
    // Run the generator against a scratch copy of the scripts dir so the
    // repository's own tests/fixtures is untouched.
    std::fs::create_dir_all(fixtures.path().join("scripts")).unwrap();
    std::fs::copy(&script, fixtures.path().join("scripts/make-fixtures.sh")).unwrap();
    let status = Command::new("bash")
        .arg(fixtures.path().join("scripts/make-fixtures.sh"))
        .status()
        .unwrap();
    assert!(status.success());
    let app = work.join("node-app");

    let home = tempfile::tempdir().unwrap();
    let log = home.path().join("npm.log");
    let run = |args: &[&str]| {
        graft(home.path())
            .args(args)
            .current_dir(&app)
            .env("GRAFT_E2E_BUILD_LOG", &log)
            .assert()
    };
    run(&["init"]).success();
    run(&["trust"]).success();
    let first = run(&["new", "first"]).success();
    assert!(
        String::from_utf8_lossy(&first.get_output().stdout).contains("layer node_modules: built")
    );

    let started = std::time::Instant::now();
    let second = run(&["new", "second"]).success();
    let out = String::from_utf8_lossy(&second.get_output().stdout).into_owned();
    let err = String::from_utf8_lossy(&second.get_output().stderr).into_owned();
    assert!(out.contains("layer node_modules: reused"), "{out}");
    assert!(!err.contains("added 1 package"), "npm ci ran again: {err}");
    let path = PathBuf::from(
        String::from_utf8(
            run(&["path", "second"])
                .success()
                .get_output()
                .stdout
                .clone(),
        )
        .unwrap()
        .trim_end(),
    );
    let test = Command::new("npm")
        .args(["test", "--silent"])
        .current_dir(&path)
        .output()
        .unwrap();
    let elapsed = started.elapsed();
    assert!(
        test.status.success(),
        "{}",
        String::from_utf8_lossy(&test.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&test.stdout).trim(), "ok");
    // Reported, not asserted: it is a performance target (AGENTS.md §9).
    #[allow(clippy::print_stderr)]
    {
        eprintln!(
            "graft new -> passing npm test: {:.2} s (target < 3 s)",
            elapsed.as_secs_f64()
        );
    }
}
