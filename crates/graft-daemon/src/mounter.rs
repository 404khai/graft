//! The platform's [`Mounter`].

use std::path::{Path, PathBuf};

use graft_core::{GraftName, RootId};
use graft_fs::view::LayerMount;
use graft_root::Repo;
use graft_store::GraftRecord;

use crate::server::Mounter;

/// The mounter for this platform: FUSE on Linux, otherwise one that
/// refuses with an explanation.
#[must_use]
pub fn platform() -> std::sync::Arc<dyn Mounter> {
    #[cfg(target_os = "linux")]
    {
        std::sync::Arc::new(fuse::FuseMounter::default())
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::sync::Arc::new(Unsupported)
    }
}

/// Refuses every mount: virtual grafts need Linux until the macOS backend.
#[derive(Debug, Default)]
pub struct Unsupported;

impl Mounter for Unsupported {
    fn mount(
        &self,
        _root: RootId,
        _mounts: &Path,
        _repo: &Repo,
        _graft: &GraftRecord,
        _layers: Vec<LayerMount>,
    ) -> Result<PathBuf, String> {
        Err(format!(
            "fuse grafts need Linux with FUSE; this is {} (use --backend reflink)",
            std::env::consts::OS
        ))
    }

    fn unmount(
        &self,
        _root: RootId,
        _mounts: &Path,
        _graft: &GraftName,
    ) -> Result<Option<bool>, String> {
        Ok(None)
    }

    fn prune(&self, _root: RootId, _mounts: &Path, _graft: &GraftRecord) -> Result<bool, String> {
        Ok(false)
    }

    fn set_actor(
        &self,
        _root: RootId,
        graft: &GraftName,
        _actor: Option<String>,
    ) -> Result<(), String> {
        Err(format!("graft {graft} is not mounted"))
    }

    fn sweep(&self, _root: RootId, _mounts: &Path) -> Result<(u64, u64), String> {
        // Only fuse grafts store blobs, and they need Linux.
        Ok((0, 0))
    }

    fn mounts(&self) -> Vec<(RootId, GraftName, PathBuf)> {
        Vec::new()
    }

    fn unmount_all(&self) {}
}

#[cfg(target_os = "linux")]
mod fuse {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, PoisonError};

    use graft_core::{GraftName, RootId};
    use graft_fs::view::LayerMount;
    use graft_fs::{FsBackend, FuseBackend, GraftSpec, Unmounted};
    use graft_root::Repo;
    use graft_store::GraftRecord;

    use crate::server::Mounter;

    /// One [`FuseBackend`] per Root.
    #[derive(Debug, Default)]
    pub(super) struct FuseMounter {
        roots: Mutex<HashMap<RootId, FuseBackend>>,
    }

    impl Mounter for FuseMounter {
        fn mount(
            &self,
            root: RootId,
            mounts: &Path,
            repo: &Repo,
            graft: &GraftRecord,
            layers: Vec<LayerMount>,
        ) -> Result<PathBuf, String> {
            let mut roots = self.roots.lock().unwrap_or_else(PoisonError::into_inner);
            let backend = roots
                .entry(root)
                .or_insert_with(|| FuseBackend::new(mounts.to_owned()));
            backend
                .mount(
                    repo,
                    &GraftSpec {
                        id: graft.id,
                        name: &graft.name,
                        base: &graft.base_commit,
                    },
                    layers,
                )
                .map_err(|e| e.to_string())
        }

        fn unmount(
            &self,
            root: RootId,
            mounts: &Path,
            graft: &GraftName,
        ) -> Result<Option<bool>, String> {
            let mut roots = self.roots.lock().unwrap_or_else(PoisonError::into_inner);
            let backend = roots
                .entry(root)
                .or_insert_with(|| FuseBackend::new(mounts.to_owned()));
            match backend.unmount(graft) {
                Ok(how) => Ok(how.map(|h| h == Unmounted::Lazy)),
                Err(e) => Err(e.to_string()),
            }
        }

        fn prune(&self, root: RootId, mounts: &Path, graft: &GraftRecord) -> Result<bool, String> {
            let mut roots = self.roots.lock().unwrap_or_else(PoisonError::into_inner);
            let backend = roots
                .entry(root)
                .or_insert_with(|| FuseBackend::new(mounts.to_owned()));
            let lazy = backend
                .unmount(&graft.name)
                .map_err(|e| e.to_string())?
                .is_some_and(|h| h == Unmounted::Lazy);
            backend
                .prune(&graft.name, graft.id)
                .map_err(|e| e.to_string())?;
            Ok(lazy)
        }

        fn set_actor(
            &self,
            root: RootId,
            graft: &GraftName,
            actor: Option<String>,
        ) -> Result<(), String> {
            let roots = self.roots.lock().unwrap_or_else(PoisonError::into_inner);
            roots
                .get(&root)
                .ok_or_else(|| format!("graft {graft} is not mounted"))?
                .set_actor(graft, actor)
                .map_err(|e| e.to_string())
        }

        fn sweep(&self, root: RootId, mounts: &Path) -> Result<(u64, u64), String> {
            let mut roots = self.roots.lock().unwrap_or_else(PoisonError::into_inner);
            roots
                .entry(root)
                .or_insert_with(|| FuseBackend::new(mounts.to_owned()))
                .sweep_blobs()
                .map_err(|e| e.to_string())
        }

        fn mounts(&self) -> Vec<(RootId, GraftName, PathBuf)> {
            let roots = self.roots.lock().unwrap_or_else(PoisonError::into_inner);
            let mut out = Vec::new();
            for (root, backend) in roots.iter() {
                for name in backend.mounted() {
                    let path = backend.path(&name);
                    out.push((*root, name, path));
                }
            }
            out.sort();
            out
        }

        fn unmount_all(&self) {
            let roots = self.roots.lock().unwrap_or_else(PoisonError::into_inner);
            for backend in roots.values() {
                backend.unmount_all();
            }
        }
    }
}
