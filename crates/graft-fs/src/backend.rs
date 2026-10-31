//! The filesystem backend interface and its reflink implementation.

use std::io;
use std::path::{Path, PathBuf};

use graft_core::{CommitId, GraftId, GraftName};
use graft_root::Repo;
use graft_root::graft::{Change, CheckoutStats, GraftRepo, branch_name, create_worktree};

use crate::clone::{CloneReport, ReflinkCloner};

/// Errors from a filesystem backend.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FsError {
    /// A filesystem operation failed.
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

    /// The graft's directory already exists, so creating it would overwrite
    /// something.
    #[error("{} already exists; remove it or choose another name", path.display())]
    AlreadyExists {
        /// The existing path.
        path: PathBuf,
    },

    /// A path that should be inside the mounts directory is not.
    #[error("refusing to touch {}: it is not inside {}", path.display(), mounts.display())]
    OutsideMounts {
        /// The offending path.
        path: PathBuf,
        /// The mounts directory.
        mounts: PathBuf,
    },

    /// Reading or writing git state failed.
    #[error(transparent)]
    Root(#[from] graft_root::RootError),
}

/// Result alias using [`FsError`].
pub type Result<T, E = FsError> = std::result::Result<T, E>;

fn io_err<'a>(action: &'static str, path: &'a Path) -> impl FnOnce(io::Error) -> FsError + 'a {
    move |source| FsError::Io {
        action,
        path: path.to_owned(),
        source,
    }
}

/// What a backend needs to know to create a graft.
#[derive(Debug, Clone, Copy)]
pub struct GraftSpec<'a> {
    /// The graft's ID (used for temporary paths).
    pub id: GraftId,
    /// The graft's name (its directory name).
    pub name: &'a GraftName,
    /// The commit to check out.
    pub base: &'a CommitId,
}

/// What creating a graft did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateReport {
    /// The graft's working tree.
    pub path: PathBuf,
    /// How each file was produced.
    pub checkout: CheckoutStats,
    /// Reflinks versus copies, and why copies were needed.
    pub clone: CloneReport,
}

/// How grafts are materialised on disk. Implemented by the reflink backend
/// now, and by FUSE and NFS backends in later phases.
pub trait FsBackend {
    /// A short name for diagnostics, e.g. `"reflink"`.
    fn kind(&self) -> &'static str;

    /// Creates the graft's working tree for `spec` from `root`.
    ///
    /// # Errors
    ///
    /// [`FsError::AlreadyExists`] if the graft's path is taken, plus I/O
    /// and git errors. On error nothing is left behind.
    fn create(&self, root: &Repo, spec: &GraftSpec<'_>) -> Result<CreateReport>;

    /// Where the graft named `name` lives (whether or not it exists).
    fn path(&self, name: &GraftName) -> PathBuf;

    /// What changed in the graft relative to `base`.
    ///
    /// # Errors
    ///
    /// I/O and git errors.
    fn diff(&self, name: &GraftName, base: &CommitId) -> Result<Vec<Change>>;

    /// Removes everything the backend stored for the graft. Succeeds if
    /// there is nothing left to remove.
    ///
    /// # Errors
    ///
    /// I/O errors, or [`FsError::OutsideMounts`] if a path would escape.
    fn prune(&self, name: &GraftName, id: GraftId) -> Result<()>;

    /// Bytes of disk the graft uses that are not shared with Root.
    ///
    /// # Errors
    ///
    /// I/O errors.
    fn disk_usage(&self, name: &GraftName) -> Result<u64>;
}

/// Grafts as real working trees whose unchanged files are reflinks of
/// Root's (ADR 0003). Falls back to copies, with a reason, when the
/// filesystem cannot reflink or the mounts directory is on another
/// filesystem.
#[derive(Debug, Clone)]
pub struct ReflinkBackend {
    mounts: PathBuf,
}

impl ReflinkBackend {
    /// A backend keeping grafts under `mounts` (`$GRAFT_HOME/roots/<id>/mounts`).
    #[must_use]
    pub fn new(mounts: PathBuf) -> Self {
        Self { mounts }
    }

    /// The mounts directory.
    #[must_use]
    pub fn mounts(&self) -> &Path {
        &self.mounts
    }

    fn tmp_path(&self, id: GraftId) -> PathBuf {
        self.mounts.join(format!(".tmp-{id}"))
    }

    fn ensure_mounts(&self) -> Result<PathBuf> {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder
            .create(&self.mounts)
            .map_err(io_err("creating", &self.mounts))?;
        self.checked_mounts()
    }

    /// The canonical mounts directory, refusing it if it is a symlink: a
    /// mounts directory swapped for a link to Root must never make prune
    /// delete Root's files.
    fn checked_mounts(&self) -> Result<PathBuf> {
        let meta =
            std::fs::symlink_metadata(&self.mounts).map_err(io_err("inspecting", &self.mounts))?;
        if !meta.is_dir() {
            return Err(FsError::OutsideMounts {
                path: self.mounts.clone(),
                mounts: self.mounts.clone(),
            });
        }
        self.mounts
            .canonicalize()
            .map_err(io_err("resolving", &self.mounts))
    }

    /// Removes `path` if it exists, after checking it is a direct child of
    /// the mounts directory. Never follows a symlink.
    fn remove_child(&self, path: &Path) -> Result<()> {
        let meta = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // Still refuse a bad mounts directory, so callers find out.
                if std::fs::symlink_metadata(&self.mounts).is_ok() {
                    self.checked_mounts()?;
                }
                return Ok(());
            }
            Err(e) => return Err(io_err("inspecting", path)(e)),
        };
        let mounts = self.checked_mounts()?;
        let parent = path
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .unwrap_or_default();
        if parent != mounts || path.file_name().is_none() {
            return Err(FsError::OutsideMounts {
                path: path.to_owned(),
                mounts,
            });
        }
        if meta.is_dir() {
            remove_tree(path)
        } else {
            std::fs::remove_file(path).map_err(io_err("removing", path))
        }
    }
}

/// `remove_dir_all`, but also removes read-only directories (Go's module
/// cache, for instance, marks them 0555). Never follows symlinks.
fn remove_tree(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {}
        Err(e) => return Err(io_err("removing", path)(e)),
    }
    make_writable(path).map_err(io_err("making writable", path))?;
    std::fs::remove_dir_all(path).map_err(io_err("removing", path))
}

fn make_writable(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(dir)?;
        if !meta.is_dir() {
            return Ok(());
        }
        let mode = meta.permissions().mode();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode | 0o700))?;
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                make_writable(&entry.path())?;
            }
        }
    }
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

impl FsBackend for ReflinkBackend {
    fn kind(&self) -> &'static str {
        "reflink"
    }

    fn create(&self, root: &Repo, spec: &GraftSpec<'_>) -> Result<CreateReport> {
        let mounts = self.ensure_mounts()?;
        let target = self.path(spec.name);
        if std::fs::symlink_metadata(&target).is_ok() {
            return Err(FsError::AlreadyExists { path: target });
        }
        let tmp = self.tmp_path(spec.id);
        self.remove_child(&tmp)?;

        let cloner = match crate::same_filesystem(root.workdir(), &mounts) {
            Ok(true) => ReflinkCloner::new(),
            Ok(false) => ReflinkCloner::copy_only(format!(
                "{} and {} are on different filesystems",
                root.workdir().display(),
                mounts.display()
            )),
            Err(e) => ReflinkCloner::copy_only(format!("cannot compare filesystems: {e}")),
        };

        let built = create_worktree(root, &tmp, &branch_name(spec.name), spec.base, &cloner)
            .map_err(FsError::from)
            .and_then(|checkout| {
                std::fs::rename(&tmp, &target).map_err(io_err("renaming", &tmp))?;
                Ok(checkout)
            });
        match built {
            Ok(checkout) => Ok(CreateReport {
                path: target,
                checkout,
                clone: cloner.report(),
            }),
            Err(e) => {
                if let Err(cleanup) = self.remove_child(&tmp) {
                    tracing::warn!(path = %tmp.display(), error = %cleanup, "could not clean up a failed graft");
                }
                Err(e)
            }
        }
    }

    fn path(&self, name: &GraftName) -> PathBuf {
        self.mounts.join(name.as_str())
    }

    fn diff(&self, name: &GraftName, base: &CommitId) -> Result<Vec<Change>> {
        Ok(GraftRepo::open(&self.path(name))?.changes_since(base)?)
    }

    fn prune(&self, name: &GraftName, id: GraftId) -> Result<()> {
        self.remove_child(&self.tmp_path(id))?;
        self.remove_child(&self.path(name))
    }

    fn disk_usage(&self, name: &GraftName) -> Result<u64> {
        let path = self.path(name);
        crate::unshared_bytes(&path).map_err(io_err("measuring", &path))
    }
}
