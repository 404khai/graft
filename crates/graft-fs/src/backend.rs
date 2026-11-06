//! The filesystem backend interface and its reflink implementation.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

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

    /// Reading or writing the metadata store failed.
    #[error(transparent)]
    Store(#[from] graft_store::StoreError),

    /// Opening a virtual graft's tree failed.
    #[error(transparent)]
    View(#[from] crate::view::ViewError),

    /// Working out a virtual graft's changes failed.
    #[error(transparent)]
    Changes(#[from] crate::ChangesError),

    /// A directory tree could not be removed because it kept reporting
    /// entries after every retry (something is still writing into it, or
    /// the filesystem's listing missed entries each time).
    #[error(
        "removing {}: directory not empty after {attempts} attempts; still there: {}{}",
        path.display(),
        display_paths(remaining),
        if *more { ", ..." } else { "" }
    )]
    NotEmpty {
        /// The directory being removed.
        path: PathBuf,
        /// How many times removal was attempted.
        attempts: usize,
        /// Some of the entries left behind, relative to `path`.
        remaining: Vec<PathBuf>,
        /// Whether more entries were left than `remaining` lists.
        more: bool,
    },

    /// The graft is not mounted, so its live state is unavailable.
    #[error("graft {name} is not mounted")]
    NotMounted {
        /// The graft.
        name: GraftName,
    },
}

fn display_paths(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return "(nothing listed)".to_owned();
    }
    let shown: Vec<_> = paths.iter().map(|p| p.display().to_string()).collect();
    shown.join(", ")
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

    pub(crate) fn ensure_mounts(&self) -> Result<PathBuf> {
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

/// Pauses before each retry of a removal that found a directory not empty.
/// On APFS, listing a very large directory while its entries are being
/// unlinked can miss some, so one pass of `remove_dir_all` leaves a few
/// behind and `rmdir` fails with `ENOTEMPTY`; a fresh pass lists and removes
/// them. Bounded, so a process that keeps writing into the tree cannot make
/// removal spin forever.
const NOT_EMPTY_RETRY_DELAYS: [Duration; 5] = [
    Duration::from_millis(10),
    Duration::from_millis(50),
    Duration::from_millis(100),
    Duration::from_millis(250),
    Duration::from_millis(500),
];

/// How many leftover entries a final [`FsError::NotEmpty`] names.
const LEFTOVERS_SHOWN: usize = 10;

/// `remove_dir_all`, but also removes read-only directories (Go's module
/// cache, for instance, marks them 0555), and retries when a pass leaves a
/// directory not empty. Never follows symlinks.
fn remove_tree(path: &Path) -> Result<()> {
    remove_tree_with(
        path,
        &NOT_EMPTY_RETRY_DELAYS,
        |p| std::fs::remove_dir_all(p),
        remove_listed,
    )
}

/// [`remove_tree`] with the retry delays, the removal and the sweep between
/// attempts supplied, so tests can simulate a filesystem that reports
/// `ENOTEMPTY`. Only ever calls `remove` and `sweep` on `path`.
fn remove_tree_with(
    path: &Path,
    retry_delays: &[Duration],
    mut remove: impl FnMut(&Path) -> io::Result<()>,
    mut sweep: impl FnMut(&Path),
) -> Result<()> {
    let mut made_writable = false;
    let mut delays = retry_delays.iter();
    let mut attempts = 0;
    loop {
        attempts += 1;
        let err = match remove(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => e,
        };
        match err.kind() {
            io::ErrorKind::PermissionDenied if !made_writable => {
                make_writable(path).map_err(io_err("making writable", path))?;
                made_writable = true;
            }
            io::ErrorKind::DirectoryNotEmpty => {
                let Some(delay) = delays.next() else {
                    let (remaining, more) = leftovers(path, LEFTOVERS_SHOWN);
                    return Err(FsError::NotEmpty {
                        path: path.to_owned(),
                        attempts,
                        remaining,
                        more,
                    });
                };
                tracing::debug!(path = %path.display(), attempts, "directory not empty after removal; sweeping and retrying");
                sweep(path);
                std::thread::sleep(*delay);
            }
            _ => return Err(io_err("removing", path)(err)),
        }
    }
}

/// Removes what is left under `dir` by listing all of it first and only
/// then deleting: files and symlinks, then directories deepest first. On
/// the macOS runner, `remove_dir_all` (which deletes while it lists) kept
/// missing the same entries pass after pass, while a plain listing saw
/// them. Best effort: failures are left for the next attempt to report.
/// Never follows symlinks, and never touches anything outside `dir`.
fn remove_listed(dir: &Path) {
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(entry.path());
                dirs.push(entry.path());
            } else {
                files.push(entry.path());
            }
        }
    }
    for file in &files {
        if let Err(e) = std::fs::remove_file(file) {
            tracing::debug!(path = %file.display(), error = %e, "sweep could not remove a file");
        }
    }
    // A directory is listed after its parent, so reverse order is deepest first.
    for d in dirs.iter().rev() {
        if let Err(e) = std::fs::remove_dir(d) {
            tracing::debug!(path = %d.display(), error = %e, "sweep could not remove a directory");
        }
    }
    tracing::debug!(path = %dir.display(), files = files.len(), dirs = dirs.len(), "swept leftovers");
}

/// Up to `limit` entries still under `dir` (files, symlinks and empty
/// directories, relative to `dir`), and whether there are more. Best
/// effort: unreadable directories are skipped. Never follows symlinks.
fn leftovers(dir: &Path, limit: usize) -> (Vec<PathBuf>, bool) {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        let mut empty = true;
        for entry in entries.flatten() {
            empty = false;
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(path);
            } else if found.len() < limit {
                found.push(path);
            } else {
                return (relative_to(dir, found), true);
            }
        }
        if empty && current != dir {
            if found.len() < limit {
                found.push(current);
            } else {
                return (relative_to(dir, found), true);
            }
        }
    }
    (relative_to(dir, found), false)
}

fn relative_to(dir: &Path, paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .map(|p| p.strip_prefix(dir).map(Path::to_owned).unwrap_or(p))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::Cell;

    const NO_DELAYS: [Duration; 3] = [Duration::ZERO; 3];

    fn not_empty() -> io::Error {
        io::Error::from(io::ErrorKind::DirectoryNotEmpty)
    }

    /// A tree of `dirs` directories with `files` files each.
    fn tree(parent: &Path, dirs: usize, files: usize) -> PathBuf {
        let root = parent.join("graft");
        for d in 0..dirs {
            let dir = root.join(format!("d{d}"));
            std::fs::create_dir_all(&dir).unwrap();
            for f in 0..files {
                std::fs::write(dir.join(format!("f{f}")), b"x").unwrap();
            }
        }
        root
    }

    /// A pass that misses half the files, as an APFS listing raced by
    /// unlinks can, then fails the way `rmdir` does.
    fn remove_half(path: &Path) -> io::Result<()> {
        for dir in std::fs::read_dir(path)? {
            let dir = dir?.path();
            for (i, file) in std::fs::read_dir(&dir)?.enumerate() {
                if i % 2 == 0 {
                    std::fs::remove_file(file?.path())?;
                }
            }
        }
        Err(not_empty())
    }

    #[test]
    fn retries_a_pass_that_leaves_the_directory_not_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tree(tmp.path(), 4, 20);
        let calls = Cell::new(0);
        remove_tree_with(
            &root,
            &NO_DELAYS,
            |p| {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    remove_half(p)
                } else {
                    std::fs::remove_dir_all(p)
                }
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(calls.get(), 2);
        assert!(!root.exists());
        assert!(tmp.path().exists(), "removal went above the graft");
    }

    #[test]
    fn gives_up_after_the_retries_and_names_what_is_left() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tree(tmp.path(), 3, 5);
        std::fs::create_dir(root.join("empty")).unwrap();
        let calls = Cell::new(0);
        let err = remove_tree_with(
            &root,
            &NO_DELAYS,
            |_| {
                calls.set(calls.get() + 1);
                Err(not_empty())
            },
            |_| {},
        )
        .unwrap_err();
        assert_eq!(calls.get(), NO_DELAYS.len() + 1);
        let FsError::NotEmpty {
            path,
            attempts,
            remaining,
            more,
        } = &err
        else {
            panic!("expected NotEmpty, got {err:?}");
        };
        assert_eq!(path, &root);
        assert_eq!(*attempts, NO_DELAYS.len() + 1);
        assert_eq!(remaining.len(), LEFTOVERS_SHOWN);
        assert!(
            *more,
            "16 entries are left but only {LEFTOVERS_SHOWN} shown"
        );
        for p in remaining {
            assert!(p.is_relative(), "{}", p.display());
            assert!(root.join(p).exists(), "{} is not left", p.display());
        }
        let message = err.to_string();
        assert!(
            message.contains("directory not empty after 4 attempts"),
            "{message}"
        );
        assert!(message.ends_with(", ..."), "{message}");
    }

    /// The runner's failure: every removal pass misses the same entries.
    /// The sweep between attempts lists everything first, then deletes it.
    #[test]
    fn sweeps_entries_that_every_removal_pass_misses() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tree(tmp.path(), 4, 30);
        #[cfg(unix)]
        std::os::unix::fs::symlink(tmp.path(), root.join("d2/outside")).unwrap();
        let keep = tmp.path().join("keep.txt");
        std::fs::write(&keep, b"outside the graft").unwrap();
        // Removes everything except `d2`, then fails as rmdir does.
        let blind = |p: &Path| -> io::Result<()> {
            for dir in std::fs::read_dir(p)? {
                let dir = dir?.path();
                if !dir.ends_with("d2") {
                    std::fs::remove_dir_all(&dir)?;
                }
            }
            Err(not_empty())
        };
        let calls = Cell::new(0);
        remove_tree_with(
            &root,
            &NO_DELAYS,
            |p| {
                calls.set(calls.get() + 1);
                if p.join("d2").exists() {
                    blind(p)
                } else {
                    std::fs::remove_dir_all(p)
                }
            },
            remove_listed,
        )
        .unwrap();
        assert!(!root.exists(), "the sweep left {:?}", leftovers(&root, 5));
        assert_eq!(calls.get(), 2);
        assert!(
            keep.exists(),
            "the sweep followed a symlink out of the graft"
        );
    }

    #[test]
    fn lists_every_leftover_when_there_are_few() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tree(tmp.path(), 1, 2);
        std::fs::create_dir(root.join("empty")).unwrap();
        let (mut left, more) = leftovers(&root, LEFTOVERS_SHOWN);
        left.sort();
        assert!(!more);
        assert_eq!(
            left,
            [Path::new("d0/f0"), Path::new("d0/f1"), Path::new("empty")]
        );
    }

    #[test]
    fn does_not_retry_other_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let calls = Cell::new(0);
        let err = remove_tree_with(
            tmp.path(),
            &NO_DELAYS,
            |_| {
                calls.set(calls.get() + 1);
                Err(io::Error::other("disk on fire"))
            },
            |_| {},
        )
        .unwrap_err();
        assert_eq!(calls.get(), 1);
        assert!(matches!(err, FsError::Io { .. }), "{err:?}");
    }

    #[test]
    fn a_missing_tree_is_already_removed() {
        let tmp = tempfile::tempdir().unwrap();
        remove_tree(&tmp.path().join("gone")).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn removes_read_only_directories() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tree(tmp.path(), 2, 3);
        let locked = root.join("d1");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        remove_tree(&root).unwrap();
        assert!(!root.exists());
    }
}
