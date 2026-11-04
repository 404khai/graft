//! The `graftd` server: single instance, socket, dispatch and remounting
//! (ADR 0007).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use graft_core::{BackendKind, GraftHome, GraftName, GraftState, RootId};
use graft_fs::view::LayerMount;
use graft_root::Repo;
use graft_store::{GraftRecord, Store};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;

use crate::paths;
use crate::protocol::{
    ActorParams, GRAFT_ERROR, GraftRef, INVALID_PARAMS, INVALID_REQUEST, JSONRPC, METHOD_NOT_FOUND,
    MountInfo, Mounted, PARSE_ERROR, Ping, Request, Response, RootRef, Swept, Unmounted,
};

/// What `graftd` needs from a filesystem backend. Implemented over
/// `FuseBackend` on Linux; tests substitute a fake.
pub trait Mounter: Send + Sync {
    /// Mounts graft `graft` of Root `root` under `mounts`, reading from
    /// `repo`, with `layers` overlaid. Returns the mount point.
    ///
    /// # Errors
    ///
    /// A message for the client.
    fn mount(
        &self,
        root: RootId,
        mounts: &Path,
        repo: &Repo,
        graft: &GraftRecord,
        layers: Vec<LayerMount>,
    ) -> Result<PathBuf, String>;

    /// Unmounts a graft. `Ok(None)` if it was not mounted; `Ok(Some(true))`
    /// if it was busy and detached lazily.
    ///
    /// # Errors
    ///
    /// A message for the client.
    /// `mounts` is the Root's mounts directory, for detaching a stale mount
    /// left by a daemon that died.
    fn unmount(
        &self,
        root: RootId,
        mounts: &Path,
        graft: &GraftName,
    ) -> Result<Option<bool>, String>;

    /// Unmounts a graft (lazily if busy) and removes everything the
    /// backend stored for it: mount point, staging files, `.git`, delta
    /// rows and blobs nothing else uses. `Ok(true)` if the unmount was lazy.
    ///
    /// # Errors
    ///
    /// A message for the client.
    fn prune(&self, root: RootId, mounts: &Path, graft: &GraftRecord) -> Result<bool, String>;

    /// Sets the actor recorded on a mounted graft's following ops.
    ///
    /// # Errors
    ///
    /// A message for the client, e.g. if it is not mounted.
    fn set_actor(
        &self,
        root: RootId,
        graft: &GraftName,
        actor: Option<String>,
    ) -> Result<(), String>;

    /// Deletes blobs of Root `root` that no graft references.
    ///
    /// # Errors
    ///
    /// A message for the client.
    fn sweep(&self, root: RootId, mounts: &Path) -> Result<(u64, u64), String>;

    /// Every mounted graft.
    fn mounts(&self) -> Vec<(RootId, GraftName, PathBuf)>;

    /// Unmounts everything, lazily where busy.
    fn unmount_all(&self);
}

/// Errors that stop the daemon.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServeError {
    /// Another `graftd` holds the lock for this `$GRAFT_HOME`.
    #[error("another graftd is already running for {}", home.display())]
    AlreadyRunning {
        /// The `$GRAFT_HOME`.
        home: PathBuf,
    },

    /// Setting up the run directory, lock or socket failed.
    #[error("{action} {}: {source}", path.display())]
    Io {
        /// What was being attempted.
        action: &'static str,
        /// The path involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
}

/// Creates `dir` with mode `0700`, refusing one that others can access.
///
/// # Errors
///
/// If it cannot be created, or exists with group or other permissions.
pub fn ensure_run_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.is_dir() {
        return Err(io::Error::other(format!(
            "{} is not a directory",
            dir.display()
        )));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::other(format!(
            "{} is accessible by other users (mode {:o}); run `chmod 700` on it",
            dir.display(),
            meta.permissions().mode() & 0o777
        )));
    }
    // The run directory must belong to whoever owns $GRAFT_HOME.
    if let Some(home) = dir.parent()
        && let Ok(home_meta) = std::fs::metadata(home)
        && home_meta.uid() != meta.uid()
    {
        return Err(io::Error::other(format!(
            "{} is owned by another user",
            dir.display()
        )));
    }
    Ok(())
}

struct State {
    home: GraftHome,
    mounter: Arc<dyn Mounter>,
    shutdown: Notify,
}

fn parse_ref(r: &GraftRef) -> Result<(RootId, GraftName), String> {
    let root = r
        .root
        .parse()
        .map_err(|e| format!("bad Root ID {:?}: {e}", r.root))?;
    let graft = r.graft.parse().map_err(|e| format!("{e}"))?;
    Ok((root, graft))
}

/// Mounts one graft after checking its record says it should be mounted.
fn mount_graft(state: &State, root: RootId, name: &GraftName) -> Result<PathBuf, String> {
    let root_dir = state.home.root_dir(&root);
    if !Store::db_path(&root_dir).is_file() {
        return Err(format!("no Root {root} in {}", state.home.path().display()));
    }
    let store = Store::open(&root_dir).map_err(|e| e.to_string())?;
    let record = store.root().map_err(|e| e.to_string())?;
    let graft = store
        .graft(name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no graft named {name}"))?;
    if graft.backend != BackendKind::Fuse {
        return Err(format!(
            "graft {name} uses the {} backend; graftd only mounts fuse grafts",
            graft.backend
        ));
    }
    if !matches!(graft.state, GraftState::Created | GraftState::Live) {
        return Err(format!(
            "graft {name} is {}; only live grafts are mounted",
            graft.state
        ));
    }
    if let Some((_, _, path)) = state
        .mounter
        .mounts()
        .into_iter()
        .find(|(r, g, _)| *r == root && g == name)
    {
        return Ok(path);
    }
    let repo = Repo::discover(&record.path).map_err(|e| e.to_string())?;
    let layers = graft_fs::layer_mounts(&store, &root_dir, graft.id).map_err(|e| e.to_string())?;
    state
        .mounter
        .mount(root, &root_dir.join("mounts"), &repo, &graft, layers)
}

/// Prunes graft `name` of Root `root` through the mounter. The record may
/// already be marked pruned by the CLI.
fn prune_graft(state: &State, root: RootId, name: &GraftName) -> Result<bool, String> {
    let root_dir = state.home.root_dir(&root);
    let store = Store::open(&root_dir).map_err(|e| e.to_string())?;
    let graft = match store.graft(name).map_err(|e| e.to_string())? {
        Some(g) => g,
        None => store
            .pruned_graft(name)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("no graft named {name}"))?,
    };
    if graft.backend != BackendKind::Fuse {
        return Err(format!("graft {name} uses the {} backend", graft.backend));
    }
    state.mounter.prune(root, &root_dir.join("mounts"), &graft)
}

/// Mounts every live fuse graft of every Root. Failures are logged.
fn remount_all(state: &State) -> usize {
    let roots = match graft_store::registry::list_roots(&state.home) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "cannot list Roots; nothing remounted");
            return 0;
        }
    };
    let mut mounted = 0;
    for (record, store) in roots {
        let grafts = match store.grafts() {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(root = %record.id, error = %e, "cannot list grafts");
                continue;
            }
        };
        for g in grafts {
            if g.backend == BackendKind::Fuse && g.state == GraftState::Live {
                match mount_graft(state, record.id, &g.name) {
                    Ok(path) => {
                        tracing::info!(graft = %g.name, path = %path.display(), "remounted");
                        mounted += 1;
                    }
                    Err(e) => tracing::warn!(graft = %g.name, error = %e, "could not remount"),
                }
            }
        }
    }
    mounted
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("internal error: {e}"))?
}

/// A failed call: a JSON-RPC error code and a message.
type Failure = (i64, String);

fn invalid(e: impl std::fmt::Display) -> Failure {
    (INVALID_PARAMS, e.to_string())
}

fn failed(e: impl std::fmt::Display) -> Failure {
    (GRAFT_ERROR, e.to_string())
}

fn value(v: &impl serde::Serialize) -> Result<serde_json::Value, Failure> {
    serde_json::to_value(v).map_err(failed)
}

fn graft_params(p: serde_json::Value) -> Result<(RootId, GraftName), Failure> {
    let r: GraftRef = serde_json::from_value(p).map_err(invalid)?;
    parse_ref(&r).map_err(invalid)
}

async fn mount_like(
    state: &Arc<State>,
    method: &str,
    p: serde_json::Value,
) -> Result<serde_json::Value, Failure> {
    let (root, name) = graft_params(p)?;
    let s = Arc::clone(state);
    match method {
        "mount" => {
            let path = blocking(move || mount_graft(&s, root, &name))
                .await
                .map_err(failed)?;
            value(&Mounted { path })
        }
        "prune" => {
            let lazy = blocking(move || prune_graft(&s, root, &name))
                .await
                .map_err(failed)?;
            value(&Unmounted {
                was_mounted: true,
                lazy,
            })
        }
        _ => {
            let mounts = s.home.root_dir(&root).join("mounts");
            let how = blocking(move || s.mounter.unmount(root, &mounts, &name))
                .await
                .map_err(failed)?;
            value(&Unmounted {
                was_mounted: how.is_some(),
                lazy: how == Some(true),
            })
        }
    }
}

async fn handle(
    state: &Arc<State>,
    method: &str,
    p: serde_json::Value,
) -> Result<serde_json::Value, Failure> {
    match method {
        "ping" => value(&Ping {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            pid: std::process::id(),
        }),
        "mount" | "unmount" | "prune" => mount_like(state, method, p).await,
        "set_actor" => {
            let a: ActorParams = serde_json::from_value(p).map_err(invalid)?;
            let (root, name) = parse_ref(&GraftRef {
                root: a.root,
                graft: a.graft,
            })
            .map_err(invalid)?;
            state
                .mounter
                .set_actor(root, &name, a.actor)
                .map_err(failed)?;
            value(&())
        }
        "gc" => {
            let r: RootRef = serde_json::from_value(p).map_err(invalid)?;
            let root: RootId = r
                .root
                .parse()
                .map_err(|e| invalid(format!("bad Root ID: {e}")))?;
            let s = Arc::clone(state);
            let mounts = s.home.root_dir(&root).join("mounts");
            let (blobs, bytes) = blocking(move || s.mounter.sweep(root, &mounts))
                .await
                .map_err(failed)?;
            value(&Swept { blobs, bytes })
        }
        "mounts" => {
            let list: Vec<MountInfo> = state
                .mounter
                .mounts()
                .into_iter()
                .map(|(root, graft, path)| MountInfo {
                    root: root.to_string(),
                    graft: graft.to_string(),
                    path,
                })
                .collect();
            value(&list)
        }
        "shutdown" => {
            state.shutdown.notify_one();
            value(&())
        }
        other => Err((METHOD_NOT_FOUND, format!("no method {other:?}"))),
    }
}

async fn dispatch(state: &Arc<State>, line: &str) -> Response {
    let value: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Response::err(None, PARSE_ERROR, e.to_string()),
    };
    let request: Request = match serde_json::from_value(value) {
        Ok(r) => r,
        Err(e) => return Response::err(None, INVALID_REQUEST, e.to_string()),
    };
    if request.jsonrpc != JSONRPC {
        return Response::err(Some(request.id), INVALID_REQUEST, "jsonrpc must be \"2.0\"");
    }
    match handle(state, &request.method, request.params).await {
        Ok(v) => Response::ok(request.id, v),
        Err((code, message)) => Response::err(Some(request.id), code, message),
    }
}

async fn connection(state: Arc<State>, stream: UnixStream) {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => return,
            Err(e) => {
                tracing::debug!(error = %e, "connection closed");
                return;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let response = dispatch(&state, &line).await;
        let mut out = match serde_json::to_vec(&response) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "cannot encode a response");
                return;
            }
        };
        out.push(b'\n');
        if write.write_all(&out).await.is_err() {
            return;
        }
    }
}

/// Runs the daemon for `home` until `shutdown`, `SIGINT` or `SIGTERM`.
/// Remounts every live fuse graft first.
///
/// # Errors
///
/// [`ServeError::AlreadyRunning`] if another daemon holds the lock, or if
/// the run directory or socket cannot be set up.
pub async fn serve(home: GraftHome, mounter: Arc<dyn Mounter>) -> Result<(), ServeError> {
    let run = paths::run_dir(&home);
    let io_err = |action, path: &Path| {
        let path = path.to_owned();
        move |source| ServeError::Io {
            action,
            path,
            source,
        }
    };
    ensure_run_dir(&run).map_err(io_err("preparing", &run))?;
    let lock_path = paths::lock_file(&home);
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(io_err("opening", &lock_path))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            return Err(ServeError::AlreadyRunning {
                home: home.path().to_owned(),
            });
        }
        Err(std::fs::TryLockError::Error(e)) => return Err(io_err("locking", &lock_path)(e)),
    }
    // Holding the lock, any existing socket is stale.
    let socket = paths::socket(&home);
    match std::fs::remove_file(&socket) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_err("removing a stale", &socket)(e)),
    }
    let listener = UnixListener::bind(&socket).map_err(io_err("listening on", &socket))?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
            .map_err(io_err("restricting", &socket))?;
    }

    let state = Arc::new(State {
        home,
        mounter,
        shutdown: Notify::new(),
    });
    let s = Arc::clone(&state);
    let remounted = blocking(move || Ok(remount_all(&s))).await.unwrap_or(0);
    tracing::info!(socket = %socket.display(), remounted, "graftd ready");

    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(io_err("handling SIGTERM for", &socket))?;
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    tokio::spawn(connection(Arc::clone(&state), stream));
                }
                Err(e) => tracing::warn!(error = %e, "accept failed"),
            },
            () = state.shutdown.notified() => break,
            _ = tokio::signal::ctrl_c() => break,
            _ = term.recv() => break,
        }
    }
    tracing::info!("graftd stopping; unmounting grafts");
    let _ = std::fs::remove_file(&socket);
    let s = Arc::clone(&state);
    let _ = blocking(move || {
        s.mounter.unmount_all();
        Ok(())
    })
    .await;
    drop(lock);
    Ok(())
}
