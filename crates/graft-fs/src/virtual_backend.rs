//! The FUSE backend: grafts as read-only virtual trees (ADR 0006).

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use graft_core::{BackendKind, CommitId, GraftId, GraftName, Timestamp};
use graft_root::Repo;
use graft_root::graft::{Change, ChangeKind, CheckoutStats, FileDiff, GraftRepo, branch_name};
use graft_root::tree::ObjectStore;
use graft_store::blobs::BlobStore;
use graft_store::{RootRecord, Store};

use crate::backend::{CreateReport, FsBackend, FsError, GraftSpec, ReflinkBackend, Result};
use crate::clone::CloneReport;
use crate::view::{GitInit, GraftView, Journal, LayerMount};

#[cfg(target_os = "linux")]
use crate::fuse as mechanism;
#[cfg(target_os = "macos")]
use crate::nfs as mechanism;

/// How an unmount went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmounted {
    /// The mount is gone.
    Clean,
    /// The mount was busy and was detached anyway: lazily on Linux, so
    /// processes with open files keep them until they close them; forcibly
    /// on macOS, so they get errors.
    Lazy,
}

/// The virtual backend kind this platform mounts.
#[cfg(target_os = "linux")]
const KIND: BackendKind = BackendKind::Fuse;
#[cfg(target_os = "macos")]
const KIND: BackendKind = BackendKind::Nfs;

fn io_err<'a>(action: &'static str, path: &'a Path) -> impl FnOnce(io::Error) -> FsError + 'a {
    move |source| FsError::Io {
        action,
        path: path.to_owned(),
        source,
    }
}

/// Grafts served lazily from Root's object store over FUSE. The backend
/// owns its mounts: they live as long as it does (in `graftd`).
#[derive(Debug)]
pub struct VirtualBackend {
    dirs: ReflinkBackend,
    /// `$GRAFT_HOME/roots/<root-id>`: the store, blobs and per-graft state.
    root_dir: PathBuf,
    mounted: Mutex<HashMap<GraftName, mechanism::Mounted>>,
}

impl VirtualBackend {
    /// A backend mounting grafts under `mounts`
    /// (`$GRAFT_HOME/roots/<root-id>/mounts`).
    #[must_use]
    pub fn new(mounts: PathBuf) -> Self {
        let root_dir = mounts.parent().map(Path::to_owned).unwrap_or_default();
        Self {
            dirs: ReflinkBackend::new(mounts),
            root_dir,
            mounted: Mutex::new(HashMap::new()),
        }
    }

    /// The Root's blob store.
    #[must_use]
    pub fn blobs(&self) -> BlobStore {
        BlobStore::new(self.root_dir.join("blobs"))
    }

    /// Graft `id`'s private directory: staging files and `.git`.
    #[must_use]
    pub fn graft_dir(&self, id: GraftId) -> PathBuf {
        self.root_dir.join("grafts").join(id.to_string())
    }

    fn view(&self, name: &GraftName) -> Option<Arc<GraftView>> {
        self.mounted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(name)
            .map(|m| Arc::clone(m.view()))
    }

    /// Journals every write the graft has received so far (NFS defers
    /// journaling until a file goes idle; FUSE journals on close).
    ///
    /// # Errors
    ///
    /// [`FsError::NotMounted`], or the errno of a failed journal write.
    pub fn sync(&self, name: &GraftName) -> Result<()> {
        let map = self.mounted.lock().unwrap_or_else(PoisonError::into_inner);
        let m = map
            .get(name)
            .ok_or_else(|| FsError::NotMounted { name: name.clone() })?;
        m.sync().map_err(|e| FsError::Io {
            action: "journaling writes in",
            path: m.path().to_owned(),
            source: io::Error::from_raw_os_error(e.0),
        })
    }

    /// Sets the actor recorded on the graft's following ops.
    ///
    /// # Errors
    ///
    /// [`FsError::NotMounted`].
    pub fn set_actor(&self, name: &GraftName, actor: Option<String>) -> Result<()> {
        self.view(name)
            .ok_or_else(|| FsError::NotMounted { name: name.clone() })?
            .set_actor(actor);
        Ok(())
    }

    /// What graft `name` changed against its base, from its delta, with
    /// contents, hiding added paths its ignore rules exclude (as git
    /// status does).
    ///
    /// # Errors
    ///
    /// [`FsError::NotMounted`], plus read and git errors.
    pub fn diffs(&self, name: &GraftName) -> Result<Vec<FileDiff>> {
        self.sync(name)?;
        let view = self
            .view(name)
            .ok_or_else(|| FsError::NotMounted { name: name.clone() })?;
        let diffs = view.diffs()?;
        let added: Vec<PathBuf> = diffs
            .iter()
            .filter(|d| d.change.kind == ChangeKind::Added)
            .map(|d| d.change.path.clone())
            .collect();
        if added.is_empty() {
            return Ok(diffs);
        }
        let ignored = GraftRepo::open(&self.path(name))?.ignored(&added)?;
        Ok(crate::without_ignored(diffs, &ignored))
    }

    /// Deletes blobs no graft of this Root references, holding every
    /// mounted graft's write lock so none is being stored meanwhile.
    /// Returns blobs and bytes removed.
    ///
    /// # Errors
    ///
    /// Store and I/O errors.
    pub fn sweep_blobs(&self) -> Result<(u64, u64)> {
        let views: Vec<Arc<GraftView>> = self
            .mounted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|m| Arc::clone(m.view()))
            .collect();
        let _locks: Vec<_> = views.iter().filter_map(|v| v.write_lock()).collect();
        let keep: HashSet<_> = Store::open(&self.root_dir)?.referenced_blobs()?;
        Ok(self.blobs().sweep(&keep)?)
    }

    /// Names of the grafts this backend has mounted.
    #[must_use]
    pub fn mounted(&self) -> Vec<GraftName> {
        let map = self.mounted.lock().unwrap_or_else(PoisonError::into_inner);
        let mut names: Vec<_> = map.keys().cloned().collect();
        names.sort();
        names
    }

    /// Mounts the graft `spec` at its path, with `layers` overlaid. The
    /// mount point is created if missing and must be empty.
    ///
    /// # Errors
    ///
    /// [`FsError::AlreadyExists`] if it is already mounted, plus I/O, git
    /// and mount errors. On error no mount point is left behind that this
    /// call created.
    pub fn mount(
        &self,
        root: &Repo,
        spec: &GraftSpec<'_>,
        layers: Vec<LayerMount>,
    ) -> Result<PathBuf> {
        let mounts = self.dirs.ensure_mounts()?;
        let target = self.path(spec.name);
        let mut map = self.mounted.lock().unwrap_or_else(PoisonError::into_inner);
        if map.contains_key(spec.name) {
            return Err(FsError::AlreadyExists { path: target });
        }
        let created = match std::fs::create_dir(&target) {
            Ok(()) => true,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => false,
            Err(e) => return Err(io_err("creating", &target)(e)),
        };
        let owner = std::fs::metadata(&mounts).map_err(io_err("inspecting", &mounts))?;
        let mounted = (|| {
            use std::os::unix::fs::MetadataExt;
            self.ensure_record(root, spec)?;
            let objects = ObjectStore::open(root);
            let journal = Journal {
                store: Store::open(&self.root_dir)?,
                blobs: self.blobs(),
                graft: spec.id,
                dir: self.graft_dir(spec.id),
                git_init: Some(git_init(&objects, spec, &layers)),
                git_in_background: cfg!(target_os = "macos"),
            };
            let view = GraftView::writable(
                objects,
                spec.base,
                layers,
                owner.uid(),
                owner.gid(),
                journal,
            )?;
            mechanism::mount(Arc::new(view), &target).map_err(io_err("mounting", &target))
        })();
        match mounted {
            Ok(m) => {
                map.insert(spec.name.clone(), m);
                Ok(target)
            }
            Err(e) => {
                if created {
                    let _ = std::fs::remove_dir(&target);
                }
                Err(e)
            }
        }
    }

    /// Unmounts the graft, lazily if a process holds files open. A stale
    /// mount this backend does not own (its server died) is detached
    /// lazily. `None` if nothing was mounted.
    ///
    /// # Errors
    ///
    /// If `fusermount` fails.
    pub fn unmount(&self, name: &GraftName) -> Result<Option<Unmounted>> {
        let taken = self
            .mounted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(name);
        let Some(m) = taken else {
            let path = self.path(name);
            return mechanism::detach_stale(&path).map_err(io_err("detaching", &path));
        };
        let path = m.path().to_owned();
        m.unmount().map(Some).map_err(io_err("unmounting", &path))
    }

    /// Unmounts every graft (lazily if busy).
    pub fn unmount_all(&self) {
        for name in self.mounted() {
            if let Err(e) = self.unmount(&name) {
                tracing::warn!(graft = %name, error = %e, "could not unmount");
            }
        }
    }

    /// Removes the empty mount point of an unmounted graft. Never recursive.
    ///
    /// # Errors
    ///
    /// If the directory is not empty (still mounted, or not a mount point).
    pub fn remove_mount_point(&self, name: &GraftName) -> Result<()> {
        let path = self.path(name);
        match std::fs::remove_dir(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_err("removing the mount point", &path)(e)),
        }
    }
}

/// Creates a virtual graft's `.git` on first use (ADR 0008).
fn git_init(objects: &ObjectStore, spec: &GraftSpec<'_>, layers: &[LayerMount]) -> GitInit {
    let objects = objects.clone();
    let branch = branch_name(spec.name);
    let base = spec.base.clone();
    let layer_paths: Vec<PathBuf> = layers.iter().map(|l| l.path.clone()).collect();
    Box::new(move |dir: &Path| {
        let paths: Vec<&Path> = layer_paths.iter().map(PathBuf::as_path).collect();
        graft_root::graft::init_virtual_git(&objects.root(), dir, &branch, &base, &paths)
            .map_err(|e| e.to_string())
    })
}

impl VirtualBackend {
    /// Records the Root and graft in the store if they are not there yet,
    /// for grafts mounted without the CLI (tests and [`FsBackend::create`]).
    /// A no-op in graftd, which only mounts recorded grafts.
    fn ensure_record(&self, root: &Repo, spec: &GraftSpec<'_>) -> Result<()> {
        let mut store = if Store::db_path(&self.root_dir).is_file() {
            Store::open(&self.root_dir)?
        } else {
            let (id, first_commit) = root.derive_root_id()?;
            let record = RootRecord {
                id,
                path: root.workdir().to_owned(),
                first_commit,
                registered_at: Timestamp::now(),
            };
            Store::create(&self.root_dir, &record)?.0
        };
        if store.graft_by_id(spec.id)?.is_none() {
            store.create_graft_with_id(spec.id, spec.name, spec.base, KIND)?;
        }
        Ok(())
    }
}

impl FsBackend for VirtualBackend {
    fn kind(&self) -> &'static str {
        KIND.as_str()
    }

    fn create(&self, root: &Repo, spec: &GraftSpec<'_>) -> Result<CreateReport> {
        let target = self.path(spec.name);
        if std::fs::symlink_metadata(&target).is_ok() {
            return Err(FsError::AlreadyExists { path: target });
        }
        let path = self.mount(root, spec, Vec::new())?;
        Ok(CreateReport {
            path,
            checkout: CheckoutStats::default(),
            clone: CloneReport::default(),
        })
    }

    fn path(&self, name: &GraftName) -> PathBuf {
        self.dirs.path(name)
    }

    fn diff(&self, name: &GraftName, _base: &CommitId) -> Result<Vec<Change>> {
        Ok(self.diffs(name)?.into_iter().map(|d| d.change).collect())
    }

    fn prune(&self, name: &GraftName, id: GraftId) -> Result<()> {
        self.unmount(name)?;
        self.remove_mount_point(name)?;
        let dir = self.graft_dir(id);
        if dir.exists() {
            let grafts = self.root_dir.join("grafts");
            let parent = dir.parent().and_then(|p| p.canonicalize().ok());
            if parent != grafts.canonicalize().ok() {
                return Err(FsError::OutsideMounts {
                    path: dir,
                    mounts: grafts,
                });
            }
            std::fs::remove_dir_all(&dir).map_err(io_err("removing", &dir))?;
        }
        if Store::db_path(&self.root_dir).is_file() {
            Store::open(&self.root_dir)?.clear_delta(id)?;
            self.sweep_blobs()?;
        }
        Ok(())
    }

    fn disk_usage(&self, name: &GraftName) -> Result<u64> {
        let store = Store::open(&self.root_dir)?;
        match store.graft(name)? {
            Some(record) => crate::virtual_disk_usage(&store, &self.root_dir, record.id),
            None => Ok(0),
        }
    }
}

impl Drop for VirtualBackend {
    fn drop(&mut self) {
        self.unmount_all();
    }
}
