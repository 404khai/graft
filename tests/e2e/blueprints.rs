//! Blueprints: `graft new --blueprint`, `graft env`, and prune releasing
//! what a blueprint provisioned (AGENTS.md Phase 11, ADR 0015).
//!
//! Tests that need Postgres run with `GRAFT_TEST_POSTGRES=1` and `initdb`,
//! `pg_ctl`, `createdb` and `psql` on `PATH`; they start a throwaway
//! cluster. Docker tests need `GRAFT_TEST_DOCKER=1` and a running daemon.
//! The rest run everywhere.

// Skipped tests say so in CI logs; setup failures panic with context.
#![allow(clippy::print_stderr, clippy::expect_used)]

use std::path::PathBuf;
use std::process::Command;

use predicates::prelude::*;

use crate::{git, git_repo, graft};

struct Fixture {
    home: tempfile::TempDir,
    repo: tempfile::TempDir,
    env: Vec<(String, String)>,
}

impl Fixture {
    /// A Root whose graft.toml is `toml`, plus `files` (tracked).
    fn new(toml: &str, files: &[(&str, &str)]) -> Self {
        let home = tempfile::tempdir().unwrap();
        let repo = git_repo();
        std::fs::write(repo.path().join("graft.toml"), toml).unwrap();
        for (p, c) in files {
            let path = repo.path().join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, c).unwrap();
        }
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-qm", "blueprint"]);
        let f = Self {
            home,
            repo,
            env: Vec::new(),
        };
        f.graft().arg("init").assert().success();
        f
    }

    fn graft(&self) -> assert_cmd::Command {
        let mut cmd = graft(self.home.path());
        cmd.current_dir(self.repo.path())
            .env_remove("GRAFT_FS")
            .envs(self.env.iter().map(|(a, b)| (a, b)));
        cmd
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.graft().args(args).output().unwrap();
        assert!(
            out.status.success(),
            "graft {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn path(&self, name: &str) -> PathBuf {
        PathBuf::from(self.ok(&["path", name]).trim())
    }

    fn env_var(&self, graft: &str, var: &str) -> String {
        let env = self.ok(&["env", graft, "--reveal"]);
        env.lines()
            .find_map(|l| l.strip_prefix(&format!("{var}=")))
            .unwrap_or_else(|| panic!("no {var} in {env}"))
            .to_owned()
    }
}

const PORTS: &str = r#"[blueprint.web]
env_files = [".env.example", ".secrets"]

[blueprint.web.env]
APP_PORT = "{{port.app}}"
API_TOKEN = "token-{{graft.name}}"

[[blueprint.web.port]]
name = "app"
"#;

#[test]
fn ports_env_files_and_env_are_per_graft() {
    let f = Fixture::new(
        PORTS,
        &[
            (".env.example", "PORT={{ port.app }}\nNAME={{graft.name}}\n"),
            (".gitignore", ".secrets\n"),
        ],
    );
    std::fs::write(f.repo.path().join(".secrets"), "from root\n").unwrap();
    f.graft()
        .args(["new", "a", "--blueprint", "web"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not been approved"))
        .stderr(predicate::str::contains(".secrets copied from Root"));
    f.graft()
        .args(["ls"])
        .assert()
        .stdout(predicate::str::contains("No grafts"));
    f.ok(&["trust"]);
    f.ok(&["new", "a", "--blueprint", "web"]);
    f.ok(&["new", "b", "--blueprint", "web"]);
    let (pa, pb) = (f.env_var("a", "APP_PORT"), f.env_var("b", "APP_PORT"));
    assert_ne!(pa, pb);
    assert_eq!(
        std::fs::read_to_string(f.path("a").join(".env")).unwrap(),
        format!("PORT={pa}\nNAME=a\n")
    );
    assert_eq!(
        std::fs::read_to_string(f.path("b").join(".secrets")).unwrap(),
        "from root\n"
    );
    // Secrets are redacted unless asked for.
    let env = f.ok(&["env", "a"]);
    assert!(env.contains("API_TOKEN=<redacted>\n"), "{env}");
    assert!(env.contains("COMPOSE_PROJECT_NAME=graft-a\n"), "{env}");
    assert_eq!(f.env_var("a", "API_TOKEN"), "token-a");
    // graft run gives the command the same environment.
    f.graft()
        .args(["run", "b", "--", "sh", "-c", "echo $APP_PORT $API_TOKEN"])
        .assert()
        .success()
        .stdout(format!("{pb} token-b\n"));

    // Prune releases the port: a new graft may get it, and env is gone.
    f.ok(&["prune", "a", "--force"]);
    f.graft().args(["env", "a"]).assert().failure();
    f.ok(&["new", "c"]);
    f.graft()
        .args(["env", "c"])
        .assert()
        .success()
        .stdout("graft c has no blueprint\n");
}

#[test]
fn template_errors_name_the_file_and_line_and_leave_nothing() {
    let f = Fixture::new(
        "[blueprint.web]\nenv_files = [\"conf/app.env.template\"]\n\n[blueprint.web.env]\nA = \"{{graft.name}}\"\n",
        &[(
            "conf/app.env.template",
            "ok={{graft.name}}\nbad = {{ port.nope }}\n",
        )],
    );
    f.graft()
        .args(["new", "a", "--blueprint", "web"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("conf/app.env.template:2:7"))
        .stderr(predicate::str::contains(
            "unknown template variable port.nope",
        ))
        .stderr(predicate::str::contains("graft a was removed again"));
    f.graft()
        .arg("ls")
        .assert()
        .stdout(predicate::str::contains("No grafts"));
    f.graft()
        .args(["new", "a", "--blueprint", "nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no blueprint \"nope\""));

    let f = Fixture::new(
        "[blueprint.web.env]\nURL = \"http://localhost:{{port.web}}\"\n",
        &[],
    );
    f.graft()
        .args(["new", "a", "--blueprint", "web"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("graft.toml:2:25"))
        .stderr(predicate::str::contains("no port named web"));
}

#[test]
fn a_failing_setup_command_removes_the_graft() {
    let f = Fixture::new(
        "[blueprint.web]\nsetup = [\"echo ran > ran.txt\", \"exit 4\"]\n",
        &[],
    );
    f.ok(&["trust"]);
    f.graft()
        .args(["new", "a", "--blueprint", "web"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("setup command `exit 4` failed"));
    f.graft()
        .arg("ls")
        .assert()
        .stdout(predicate::str::contains("No grafts"));
}

/// A throwaway Postgres cluster on a loopback TCP port.
struct Postgres {
    dir: tempfile::TempDir,
    port: u16,
}

impl Postgres {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let status = Command::new("initdb")
            .args(["-A", "trust", "-U", "postgres", "-D"])
            .arg(&data)
            .output()
            .expect("initdb on PATH (GRAFT_TEST_POSTGRES=1)");
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let ok = Command::new("pg_ctl")
            .arg("-D")
            .arg(&data)
            .args([
                "-o",
                &format!("-p {port} -k '' -c listen_addresses=127.0.0.1"),
                "-l",
            ])
            .arg(dir.path().join("log"))
            .args(["-w", "start"])
            .status()
            .unwrap();
        assert!(ok.success());
        let pg = Self { dir, port };
        pg.psql("postgres", "CREATE DATABASE app_dev");
        pg.psql("app_dev", "CREATE TABLE hits (graft text)");
        pg
    }

    fn env(&self) -> Vec<(String, String)> {
        vec![
            ("PGHOST".into(), "127.0.0.1".into()),
            ("PGPORT".into(), self.port.to_string()),
            ("PGUSER".into(), "postgres".into()),
        ]
    }

    fn psql(&self, db: &str, sql: &str) -> String {
        let out = Command::new("psql")
            .envs(self.env())
            .args(["-v", "ON_ERROR_STOP=1", "-Atc", sql, db])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
}

impl Drop for Postgres {
    fn drop(&mut self) {
        let _ = Command::new("pg_ctl")
            .arg("-D")
            .arg(self.dir.path().join("data"))
            .args(["-m", "immediate", "stop"])
            .output();
    }
}

const WEB: &str = r#"[blueprint.web]
setup = ["psql -v ON_ERROR_STOP=1 -qc \"insert into hits values ('setup')\" \"$DATABASE_URL\""]

[blueprint.web.env]
DATABASE_URL = "postgresql://postgres@127.0.0.1:{{env.PGPORT}}/{{service.db.database}}"
APP_PORT = "{{port.app}}"

[[blueprint.web.port]]
name = "app"
container_port = 3000

[[blueprint.web.service]]
name = "db"
kind = "postgres-template"
template = "app_dev"
"#;

/// The web fixture's "test suite": write a row, read the graft's count.
const TEST: &str = "#!/bin/sh\nset -e\npsql -v ON_ERROR_STOP=1 -qc \"insert into hits values ('$GRAFT_NAME')\" \"$DATABASE_URL\"\nn=$(psql -Atc \"select count(*) from hits where graft = '$GRAFT_NAME'\" \"$DATABASE_URL\")\ntest \"$n\" = 1\ncurl -fsS \"http://127.0.0.1:$APP_PORT/name.txt\"\n";

fn wait_for(url: &str) -> String {
    for _ in 0..100 {
        if let Ok(out) = Command::new("curl").args(["-fsS", url]).output()
            && out.status.success()
        {
            return String::from_utf8(out.stdout).unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("{url} never answered");
}

/// AGENTS.md Phase 11: two grafts of the web fixture run their dev servers
/// and test suites at the same time with no port or database collision,
/// and prune removes the graft's database and port.
#[test]
fn two_grafts_run_servers_and_tests_side_by_side() {
    if std::env::var("GRAFT_TEST_POSTGRES").as_deref() != Ok("1") {
        eprintln!("skipped: set GRAFT_TEST_POSTGRES=1 with Postgres tools on PATH");
        return;
    }
    let pg = Postgres::start();
    let mut f = Fixture::new(WEB, &[("test.sh", TEST)]);
    f.env = pg.env();
    f.ok(&["trust"]);
    for g in ["a", "b"] {
        f.ok(&["new", g, "--blueprint", "web"]);
        std::fs::write(f.path(g).join("name.txt"), format!("{g}\n")).unwrap();
    }
    let mut servers = Vec::new();
    for g in ["a", "b"] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_graft"));
        cmd.current_dir(f.repo.path())
            .env("GRAFT_HOME", f.home.path())
            .envs(pg.env())
            .args(["run", g, "--", "sh", "-c"])
            .arg("exec python3 -m http.server \"$APP_PORT\" --bind 127.0.0.1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // Its own process group, so the server under `graft run` can be
        // stopped with it.
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        servers.push(Server(cmd.spawn().unwrap()));
    }
    let ports: Vec<String> = ["a", "b"]
        .iter()
        .map(|g| f.env_var(g, "APP_PORT"))
        .collect();
    assert_ne!(ports[0], ports[1]);
    for (g, p) in ["a", "b"].iter().zip(&ports) {
        assert_eq!(
            wait_for(&format!("http://127.0.0.1:{p}/name.txt")),
            format!("{g}\n")
        );
    }
    // Both test suites at once, each against its own database.
    let runs: Vec<_> = ["a", "b"]
        .iter()
        .map(|g| {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_graft"));
            cmd.current_dir(f.repo.path())
                .env("GRAFT_HOME", f.home.path())
                .envs(pg.env())
                .args(["run", g, "--", "sh", "test.sh"]);
            cmd.output_spawned()
        })
        .collect();
    for (g, out) in ["a", "b"].iter().zip(runs) {
        let out = out.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{g}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{g}\n"));
    }
    drop(servers);
    let dbs = pg.psql(
        "postgres",
        "select datname from pg_database where datname like 'app\\_dev\\_%' order by 1",
    );
    assert_eq!(dbs, "app_dev_a\napp_dev_b\n");
    assert_eq!(
        pg.psql("app_dev_a", "select graft from hits order by 1"),
        "a\nsetup\n"
    );
    assert_eq!(
        pg.psql("app_dev", "select count(*) from hits"),
        "0\n",
        "template untouched"
    );

    f.ok(&["prune", "a", "--force"]);
    let dbs = pg.psql(
        "postgres",
        "select datname from pg_database where datname like 'app\\_dev\\_%' order by 1",
    );
    assert_eq!(dbs, "app_dev_b\n");
    f.ok(&["prune", "b", "--force"]);
    let dbs = pg.psql(
        "postgres",
        "select count(*) from pg_database where datname like 'app\\_dev\\_%'",
    );
    assert_eq!(dbs, "0\n");
}

/// A `graft run` dev server; dropping it kills its whole process group,
/// whatever the test's outcome.
struct Server(std::process::Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{}", self.0.id())])
            .status();
        let _ = self.0.wait();
    }
}

trait SpawnOutput {
    fn output_spawned(&mut self) -> std::process::Child;
}

impl SpawnOutput for Command {
    fn output_spawned(&mut self) -> std::process::Child {
        self.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    }
}

fn docker(args: &[&str]) -> std::process::Output {
    Command::new("docker").args(args).output().unwrap()
}

/// Docker volumes and networks are named per graft, labelled, and removed
/// by prune.
#[test]
fn docker_resources_are_namespaced_and_pruned() {
    if std::env::var("GRAFT_TEST_DOCKER").as_deref() != Ok("1") {
        eprintln!("skipped: set GRAFT_TEST_DOCKER=1 with a running Docker daemon");
        return;
    }
    let f = Fixture::new(
        "[blueprint.web.env]\nVOLUME = \"{{service.data.name}}\"\nNETWORK = \"{{service.net.name}}\"\n\n[[blueprint.web.service]]\nname = \"data\"\nkind = \"docker-volume\"\n\n[[blueprint.web.service]]\nname = \"net\"\nkind = \"docker-network\"\n",
        &[],
    );
    f.ok(&["trust"]);
    f.ok(&["new", "a", "--blueprint", "web"]);
    f.ok(&["new", "b", "--blueprint", "web"]);
    let (va, vb) = (f.env_var("a", "VOLUME"), f.env_var("b", "VOLUME"));
    let na = f.env_var("a", "NETWORK");
    assert_ne!(va, vb);
    assert!(va.starts_with("graft-") && va.ends_with("-a-data"), "{va}");
    assert!(docker(&["volume", "inspect", &va]).status.success());
    assert!(docker(&["network", "inspect", &na]).status.success());
    f.ok(&["prune", "a", "--force"]);
    assert!(
        !docker(&["volume", "inspect", &va]).status.success(),
        "volume left"
    );
    assert!(
        !docker(&["network", "inspect", &na]).status.success(),
        "network left"
    );
    assert!(
        docker(&["volume", "inspect", &vb]).status.success(),
        "b's volume removed"
    );
    f.ok(&["prune", "b", "--force"]);
    assert!(!docker(&["volume", "inspect", &vb]).status.success());
}
