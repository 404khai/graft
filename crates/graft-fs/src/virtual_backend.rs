//! The FUSE backend: grafts as read-only virtual trees (ADR 0006).

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use graft_core::{CommitId, GraftId, GraftName};
use graft_root::Repo;
use graft_root::graft::{Change, CheckoutStats};
use graft_root::tree::ObjectStore;

use crate::backend::{CreateReport, FsBackend, FsError, GraftSpec, ReflinkBackend, Result};
use crate::clone::CloneReport;
use crate::view::{GraftView, LayerMount};

pub use crate::fuse::Unmounted;

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
pub struct FuseBackend {
    dirs: ReflinkBackend,
    mounted: Mutex<HashMap<GraftName, crate::fuse::Mounted>>,
}

impl FuseBackend {
    /// A backend mounting grafts under `mounts`.
    #[must_use]
    pub fn new(mounts: PathBuf) -> Self {
        Self {
            dirs: ReflinkBackend::new(mounts),
            mounted: Mutex::new(HashMap::new()),
        }
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
            let view = GraftView::new(
                ObjectStore::open(root),
                spec.base,
                layers,
                owner.uid(),
                owner.gid(),
            )?;
            crate::fuse::mount(view, &target).map_err(io_err("mounting", &target))
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
            return crate::fuse::detach_stale(&path).map_err(io_err("detaching", &path));
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

impl FsBackend for FuseBackend {
    fn kind(&self) -> &'static str {
        "fuse"
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

    fn diff(&self, _name: &GraftName, _base: &CommitId) -> Result<Vec<Change>> {
        // Read-only until the delta store: nothing can have changed.
        Ok(Vec::new())
    }

    fn prune(&self, name: &GraftName, _id: GraftId) -> Result<()> {
        self.unmount(name)?;
        self.remove_mount_point(name)
    }

    fn disk_usage(&self, _name: &GraftName) -> Result<u64> {
        Ok(0)
    }
}

impl Drop for FuseBackend {
    fn drop(&mut self) {
        self.unmount_all();
    }
}
