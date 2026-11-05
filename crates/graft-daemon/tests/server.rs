//! graftd's protocol, single-instance lock and remounting, with a fake
//! mounter so they run on every platform.

// Fixture helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use graft_core::{BackendKind, GraftHome, GraftName, GraftState, RootId};
use graft_daemon::client::{Client, ClientError};
use graft_daemon::paths;
use graft_daemon::protocol::{GRAFT_ERROR, METHOD_NOT_FOUND, PARSE_ERROR};
use graft_daemon::server::{Mounter, ServeError, serve};
use graft_fs::view::LayerMount;
use graft_root::Repo;
use graft_store::GraftRecord;

type MountRow = (RootId, GraftName, PathBuf, usize);

#[derive(Default)]
struct Fake {
    mounted: Mutex<Vec<MountRow>>,
    actors: Mutex<Vec<(String, Option<String>)>>,
    unmounted_all: Mutex<bool>,
}

impl Mounter for Fake {
    fn mount(
        &self,
        root: RootId,
        mounts: &Path,
        _repo: &Repo,
        graft: &GraftRecord,
        layers: Vec<LayerMount>,
    ) -> Result<PathBuf, String> {
        let path = mounts.join(graft.name.as_str());
        self.mounted
            .lock()
            .unwrap()
            .push((root, graft.name.clone(), path.clone(), layers.len()));
        Ok(path)
    }

    fn unmount(
        &self,
        root: RootId,
        _mounts: &Path,
        graft: &GraftName,
    ) -> Result<Option<bool>, String> {
        let mut m = self.mounted.lock().unwrap();
        let before = m.len();
        m.retain(|(r, g, _, _)| !(*r == root && g == graft));
        Ok((m.len() != before).then_some(false))
    }

    fn prune(&self, root: RootId, mounts: &Path, graft: &GraftRecord) -> Result<bool, String> {
        self.unmount(root, mounts, &graft.name).map(|_| false)
    }

    fn set_actor(
        &self,
        _root: RootId,
        graft: &GraftName,
        actor: Option<String>,
    ) -> Result<(), String> {
        self.actors.lock().unwrap().push((graft.to_string(), actor));
        Ok(())
    }

    fn sync(&self, _root: RootId, graft: &GraftName) -> Result<(), String> {
        self.actors
            .lock()
            .unwrap()
            .push((format!("sync {graft}"), None));
        Ok(())
    }

    fn sweep(&self, _root: RootId, _mounts: &Path) -> Result<(u64, u64), String> {
        Ok((2, 10))
    }

    fn mounts(&self) -> Vec<(RootId, GraftName, PathBuf)> {
        self.mounted
            .lock()
            .unwrap()
            .iter()
            .map(|(r, g, p, _)| (*r, g.clone(), p.clone()))
            .collect()
    }

    fn unmount_all(&self) {
        self.mounted.lock().unwrap().clear();
        *self.unmounted_all.lock().unwrap() = true;
    }
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@graft.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@graft.invalid")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

struct Env {
    _tmp: tempfile::TempDir,
    home: GraftHome,
    root: RootId,
    store: graft_store::Store,
    base: graft_core::CommitId,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir(&repo_dir).unwrap();
    git(&repo_dir, &["init", "-q", "-b", "main"]);
    std::fs::write(repo_dir.join("a"), "a").unwrap();
    git(&repo_dir, &["add", "-A"]);
    git(&repo_dir, &["commit", "-q", "-m", "a"]);
    let home = GraftHome::from_path(tmp.path().join("home")).unwrap();
    let repo = Repo::discover(&repo_dir).unwrap();
    let base = repo.head_commit().unwrap();
    let (record, store, _) = graft_store::registry::register(&home, &repo).unwrap();
    Env {
        _tmp: tmp,
        home,
        root: record.id,
        store,
        base,
    }
}

fn add_graft(env: &mut Env, name: &str, backend: BackendKind, state: GraftState) -> GraftRecord {
    let g = env
        .store
        .create_graft(&name.parse().unwrap(), &env.base, backend)
        .unwrap();
    if state == GraftState::Created {
        g
    } else {
        env.store.set_state(g.id, state).unwrap()
    }
}

/// Runs `serve` on its own thread and waits until it answers.
fn start(home: &GraftHome, fake: Arc<Fake>) -> std::thread::JoinHandle<Result<(), ServeError>> {
    let h = home.clone();
    let handle = std::thread::spawn(move || {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(serve(h, fake))
    });
    for _ in 0..200 {
        if let Ok(mut c) = Client::connect(home)
            && c.ping().is_ok()
        {
            return handle;
        }
        assert!(!handle.is_finished(), "serve exited early");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("graftd did not start");
}

fn rpc_code(e: ClientError) -> i64 {
    match e {
        ClientError::Rpc(r) => r.code,
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

#[test]
fn serves_the_protocol_and_shuts_down() {
    let mut env = env();
    add_graft(&mut env, "virt", BackendKind::Fuse, GraftState::Created);
    add_graft(&mut env, "real", BackendKind::Reflink, GraftState::Live);
    let fake = Arc::new(Fake::default());
    let server = start(&env.home, Arc::clone(&fake));

    // Socket and run directory are private.
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&paths::socket(&env.home)), 0o600);
        assert_eq!(mode(&paths::run_dir(&env.home)), 0o700);
    }

    let mut c = Client::connect(&env.home).unwrap();
    let ping = c.ping().unwrap();
    assert_eq!(ping.pid, std::process::id());
    assert_eq!(ping.version, env!("CARGO_PKG_VERSION"));

    let virt: GraftName = "virt".parse().unwrap();
    let path = c.mount(&env.root, &virt).unwrap();
    assert_eq!(path, env.home.root_dir(&env.root).join("mounts/virt"));
    // Mounting again is idempotent.
    assert_eq!(c.mount(&env.root, &virt).unwrap(), path);
    assert_eq!(fake.mounted.lock().unwrap().len(), 1);
    assert_eq!(c.mounts().unwrap().len(), 1);

    // Only fuse grafts of real Roots can be mounted.
    let err = c.mount(&env.root, &"real".parse().unwrap()).unwrap_err();
    assert!(err.to_string().contains("reflink backend"), "{err}");
    assert_eq!(
        rpc_code(c.mount(&env.root, &"ghost".parse().unwrap()).unwrap_err()),
        GRAFT_ERROR
    );
    let other = RootId::from_bytes([9; 16]);
    assert_eq!(rpc_code(c.mount(&other, &virt).unwrap_err()), GRAFT_ERROR);
    assert_eq!(
        rpc_code(
            c.call::<_, serde_json::Value>("frobnicate", &())
                .unwrap_err()
        ),
        METHOD_NOT_FOUND
    );

    c.set_actor(&env.root, &virt, Some("run-1")).unwrap();
    assert_eq!(
        *fake.actors.lock().unwrap(),
        [("virt".to_owned(), Some("run-1".to_owned()))]
    );
    c.sync(&env.root, &virt).unwrap();
    assert_eq!(fake.actors.lock().unwrap().last().unwrap().0, "sync virt");
    let swept = c.gc(&env.root).unwrap();
    assert_eq!((swept.blobs, swept.bytes), (2, 10));
    let u = c.unmount(&env.root, &virt).unwrap();
    assert!(u.was_mounted && !u.lazy);
    // Prune goes through the mounter, and only for fuse grafts.
    c.mount(&env.root, &virt).unwrap();
    c.prune(&env.root, &virt).unwrap();
    assert!(fake.mounted.lock().unwrap().is_empty());
    let err = c.prune(&env.root, &"real".parse().unwrap()).unwrap_err();
    assert!(err.to_string().contains("reflink backend"), "{err}");
    assert!(!c.unmount(&env.root, &virt).unwrap().was_mounted);

    // Garbage on the wire gets a parse error, and the connection survives.
    let mut raw = UnixStream::connect(paths::socket(&env.home)).unwrap();
    raw.write_all(b"{not json\n").unwrap();
    let mut line = String::new();
    let mut reader = BufReader::new(raw.try_clone().unwrap());
    reader.read_line(&mut line).unwrap();
    let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(resp["error"]["code"], PARSE_ERROR);
    raw.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n")
        .unwrap();
    line.clear();
    reader.read_line(&mut line).unwrap();
    let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(resp["id"], 7);

    c.mount(&env.root, &virt).unwrap();
    c.shutdown().unwrap();
    server.join().unwrap().unwrap();
    assert!(
        *fake.unmounted_all.lock().unwrap(),
        "shutdown unmounts everything"
    );
    assert!(!paths::socket(&env.home).exists());
    assert!(Client::connect(&env.home).is_err());
}

#[test]
fn one_daemon_per_home() {
    let env = env();
    let server = start(&env.home, Arc::new(Fake::default()));
    let second = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(serve(env.home.clone(), Arc::new(Fake::default())));
    assert!(
        matches!(second, Err(ServeError::AlreadyRunning { .. })),
        "{second:?}"
    );
    // The first is unaffected.
    Client::connect(&env.home).unwrap().ping().unwrap();
    Client::connect(&env.home).unwrap().shutdown().unwrap();
    server.join().unwrap().unwrap();
}

#[test]
fn remounts_live_fuse_grafts_on_start() {
    let mut env = env();
    add_graft(&mut env, "live-virt", BackendKind::Fuse, GraftState::Live);
    add_graft(&mut env, "new-virt", BackendKind::Fuse, GraftState::Created);
    add_graft(
        &mut env,
        "live-real",
        BackendKind::Reflink,
        GraftState::Live,
    );
    let gone = add_graft(&mut env, "gone", BackendKind::Fuse, GraftState::Live);
    env.store.set_state(gone.id, GraftState::Pruned).unwrap();
    // A stale socket from a crashed daemon is replaced.
    graft_daemon::server::ensure_run_dir(&paths::run_dir(&env.home)).unwrap();
    std::fs::write(paths::socket(&env.home), "").unwrap();

    let fake = Arc::new(Fake::default());
    let server = start(&env.home, Arc::clone(&fake));
    let names: Vec<_> = fake
        .mounted
        .lock()
        .unwrap()
        .iter()
        .map(|(_, g, _, _)| g.to_string())
        .collect();
    assert_eq!(names, ["live-virt"]);
    Client::connect(&env.home).unwrap().shutdown().unwrap();
    server.join().unwrap().unwrap();
}

#[test]
fn refuses_a_run_directory_others_can_read() {
    use std::os::unix::fs::PermissionsExt;
    let env = env();
    let run = paths::run_dir(&env.home);
    std::fs::create_dir_all(&run).unwrap();
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755)).unwrap();
    let r = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(serve(env.home.clone(), Arc::new(Fake::default())));
    let err = r.unwrap_err();
    assert!(
        err.to_string().contains("accessible by other users"),
        "{err}"
    );
}
