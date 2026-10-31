//! Cloning files with reflinks, falling back to copies.

use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use graft_root::graft::FileCloner;

use crate::sys;

/// What a [`ReflinkCloner`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CloneReport {
    /// Files created as reflinks (sharing data blocks).
    pub reflinked: u64,
    /// Files created as full copies.
    pub copied: u64,
    /// Why copies were used instead of reflinks, if they were.
    pub fallback: Option<String>,
}

/// Creates files as reflinks of their source. If the filesystem cannot
/// reflink, it switches to plain copies for the rest of its life and
/// records why, so callers can warn once.
#[derive(Debug, Default)]
pub struct ReflinkCloner {
    copy_only: AtomicBool,
    reflinked: AtomicU64,
    copied: AtomicU64,
    fallback: Mutex<Option<String>>,
}

impl ReflinkCloner {
    /// A cloner that tries reflinks first.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A cloner that only copies, because reflinks are known to be
    /// impossible for `reason`.
    #[must_use]
    pub fn copy_only(reason: impl Into<String>) -> Self {
        let cloner = Self::default();
        cloner.fall_back(reason.into());
        cloner
    }

    fn fall_back(&self, reason: String) {
        self.copy_only.store(true, Ordering::Relaxed);
        let mut slot = self
            .fallback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        slot.get_or_insert(reason);
    }

    /// What has been done so far.
    #[must_use]
    pub fn report(&self) -> CloneReport {
        CloneReport {
            reflinked: self.reflinked.load(Ordering::Relaxed),
            copied: self.copied.load(Ordering::Relaxed),
            fallback: self
                .fallback
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
        }
    }
}

/// Copies `src` to the new file `dst`, never following a symlink at `dst`.
fn copy_new(src: &Path, dst: &Path) -> io::Result<()> {
    let mut from = std::fs::File::open(src)?;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut opts, libc::O_NOFOLLOW);
    let mut to = opts.open(dst)?;
    io::copy(&mut from, &mut to)?;
    to.set_permissions(from.metadata()?.permissions())?;
    Ok(())
}

impl FileCloner for ReflinkCloner {
    fn clone_file(&self, src: &Path, dst: &Path) -> io::Result<()> {
        if !self.copy_only.load(Ordering::Relaxed) {
            match sys::reflink(src, dst) {
                Ok(()) => {
                    self.reflinked.fetch_add(1, Ordering::Relaxed);
                    return Ok(());
                }
                Err(e) if sys::is_unsupported(&e) => {
                    let fs = dst
                        .parent()
                        .and_then(|p| crate::filesystem_type(p).ok())
                        .map_or_else(|| "this filesystem".to_owned(), |f| f.name().to_owned());
                    self.fall_back(format!("{fs} does not support reflinks ({e})"));
                }
                Err(e) => return Err(e),
            }
        }
        copy_new(src, dst)?;
        self.copied.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// Recreates the tree at `src` as `dst` (which must not exist), creating
/// every regular file with `cloner` and copying symlinks as symlinks.
/// Directory permissions are applied after their contents exist, so
/// read-only directories (as some package managers create) work. Other file
/// types (sockets, FIFOs) are skipped.
///
/// # Errors
///
/// The first I/O error; `dst` may then be partially populated.
pub fn clone_tree(src: &Path, dst: &Path, cloner: &dyn FileCloner) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if !meta.is_dir() {
        return Err(io::Error::other(format!(
            "{} is not a directory",
            src.display()
        )));
    }
    std::fs::create_dir(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if kind.is_dir() {
            clone_tree(&entry.path(), &to, cloner)?;
        } else if kind.is_file() {
            cloner.clone_file(&entry.path(), &to)?;
        } else if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(entry.path())?, &to)?;
        } else {
            tracing::debug!(path = %entry.path().display(), "skipping special file in layer");
        }
    }
    std::fs::set_permissions(dst, meta.permissions())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_trees_with_symlinks_and_read_only_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("a/b")).unwrap();
        std::fs::write(src.join("a/b/f.js"), "f").unwrap();
        std::fs::write(src.join("top"), "t").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::os::unix::fs::symlink("a/b/f.js", src.join("link")).unwrap();
            std::fs::set_permissions(src.join("a"), std::fs::Permissions::from_mode(0o555))
                .unwrap();
        }
        let dst = dir.path().join("dst");
        let cloner = ReflinkCloner::new();
        clone_tree(&src, &dst, &cloner).unwrap();
        assert_eq!(std::fs::read_to_string(dst.join("a/b/f.js")).unwrap(), "f");
        assert_eq!(std::fs::read_to_string(dst.join("top")).unwrap(), "t");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::read_link(dst.join("link")).unwrap(),
                Path::new("a/b/f.js")
            );
            let mode = std::fs::metadata(dst.join("a"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o555);
            for d in [&src, &dst] {
                std::fs::set_permissions(d.join("a"), std::fs::Permissions::from_mode(0o755))
                    .unwrap();
            }
        }
        // Writes to the clone never reach the source.
        std::fs::write(dst.join("top"), "changed").unwrap();
        assert_eq!(std::fs::read_to_string(src.join("top")).unwrap(), "t");
        assert!(
            clone_tree(&src, &dst, &cloner).is_err(),
            "dst must not exist"
        );
    }

    #[test]
    fn clones_or_copies_with_identical_contents_and_mode() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::write(&src, b"data").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o750)).unwrap();
        }
        let cloner = ReflinkCloner::new();
        cloner.clone_file(&src, &dir.path().join("dst")).unwrap();
        assert_eq!(std::fs::read(dir.path().join("dst")).unwrap(), b"data");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("dst"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o750);
        }
        let report = cloner.report();
        assert_eq!(report.reflinked + report.copied, 1);
        assert_eq!(report.fallback.is_some(), report.copied == 1);
    }

    #[test]
    fn copy_only_never_reflinks_and_explains_why() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::write(&src, b"data").unwrap();
        let cloner = ReflinkCloner::copy_only("different filesystems");
        cloner.clone_file(&src, &dir.path().join("a")).unwrap();
        cloner.clone_file(&src, &dir.path().join("b")).unwrap();
        let report = cloner.report();
        assert_eq!((report.reflinked, report.copied), (0, 2));
        assert_eq!(report.fallback.as_deref(), Some("different filesystems"));
    }

    #[test]
    fn refuses_to_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::write(&src, b"data").unwrap();
        std::fs::write(dir.path().join("dst"), b"keep").unwrap();
        for cloner in [ReflinkCloner::new(), ReflinkCloner::copy_only("test")] {
            let err = cloner
                .clone_file(&src, &dir.path().join("dst"))
                .unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
            assert_eq!(std::fs::read(dir.path().join("dst")).unwrap(), b"keep");
        }
    }
}
