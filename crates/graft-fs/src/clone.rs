//! Cloning files with reflinks, falling back to copies.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

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
/// Directories are created in one walk; files are then cloned on several
/// threads, because each reflink is a separate syscall that may also have to
/// write back the source's dirty data first (Phase 8 profile:
/// `bench/results/profiles/`).
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
    let mut plan = Plan::default();
    plan.dirs.push((dst.to_owned(), meta.permissions()));
    std::fs::create_dir(dst)?;
    walk(src, dst, &mut plan)?;
    clone_files(&plan.files, cloner)?;
    // Deepest first, so a read-only parent is locked only after its
    // children are complete.
    for (dir, permissions) in plan.dirs.into_iter().rev() {
        std::fs::set_permissions(&dir, permissions)?;
    }
    Ok(())
}

#[derive(Default)]
struct Plan {
    /// Every directory created, parents before children, with the
    /// permissions to apply once it is populated.
    dirs: Vec<(PathBuf, std::fs::Permissions)>,
    /// Regular files to clone: (source, destination).
    files: Vec<(PathBuf, PathBuf)>,
}

/// Creates `dst`'s subdirectories and symlinks from `src`, and lists its
/// files. Never follows a symlink.
fn walk(src: &Path, dst: &Path, plan: &mut Plan) -> io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if kind.is_dir() {
            std::fs::create_dir(&to)?;
            plan.dirs.push((
                to.clone(),
                std::fs::symlink_metadata(entry.path())?.permissions(),
            ));
            walk(&entry.path(), &to, plan)?;
        } else if kind.is_file() {
            plan.files.push((entry.path(), to));
        } else if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(entry.path())?, &to)?;
        } else {
            tracing::debug!(path = %entry.path().display(), "skipping special file in layer");
        }
    }
    Ok(())
}

/// Files per worker below which another thread is not worth starting.
const FILES_PER_WORKER: usize = 64;

/// Clones every `(source, destination)` with `cloner` on up to 8 threads,
/// stopping at the first error.
fn clone_files(files: &[(PathBuf, PathBuf)], cloner: &dyn FileCloner) -> io::Result<()> {
    let workers = std::thread::available_parallelism()
        .map_or(4, std::num::NonZero::get)
        .min(8)
        .min(files.len().div_ceil(FILES_PER_WORKER).max(1));
    if workers == 1 {
        return files
            .iter()
            .try_for_each(|(from, to)| cloner.clone_file(from, to));
    }
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let first_error: Mutex<Option<io::Error>> = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while !failed.load(Ordering::Relaxed) {
                    let Some((from, to)) = files.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        return;
                    };
                    if let Err(e) = cloner.clone_file(from, to) {
                        failed.store(true, Ordering::Relaxed);
                        first_error
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .get_or_insert(e);
                        return;
                    }
                }
            });
        }
    });
    match first_error
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    {
        Some(e) => Err(e),
        None => Ok(()),
    }
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

    /// Every file in `dir`, relative, with its contents and mode.
    fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>, u32)> {
        use std::os::unix::fs::PermissionsExt;
        let mut out = Vec::new();
        let mut stack = vec![dir.to_owned()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                let meta = std::fs::symlink_metadata(&p).unwrap();
                let rel = p.strip_prefix(dir).unwrap().to_owned();
                let mode = meta.permissions().mode();
                if meta.is_dir() {
                    out.push((rel, Vec::new(), mode));
                    stack.push(p);
                } else if meta.is_symlink() {
                    let target = std::fs::read_link(&p).unwrap();
                    out.push((rel, target.into_os_string().into_encoded_bytes(), mode));
                } else {
                    out.push((rel, std::fs::read(&p).unwrap(), mode));
                }
            }
        }
        out.sort();
        out
    }

    #[cfg(unix)]
    #[test]
    fn clones_many_files_in_parallel_exactly() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        for d in 0..40 {
            let dir = src.join(format!("pkg{d}/lib"));
            std::fs::create_dir_all(&dir).unwrap();
            for f in 0..50 {
                std::fs::write(dir.join(format!("{f}.js")), format!("{d}/{f}")).unwrap();
            }
            std::os::unix::fs::symlink("lib/0.js", src.join(format!("pkg{d}/main.js"))).unwrap();
        }
        let exe = src.join("pkg1/lib/1.js");
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        // A read-only directory inside a read-only directory.
        for dir in ["pkg2/lib", "pkg2"] {
            std::fs::set_permissions(src.join(dir), std::fs::Permissions::from_mode(0o555))
                .unwrap();
        }
        let dst = tmp.path().join("dst");
        let cloner = ReflinkCloner::new();
        clone_tree(&src, &dst, &cloner).unwrap();
        assert_eq!(snapshot(&src), snapshot(&dst));
        let r = cloner.report();
        assert_eq!(r.reflinked + r.copied, 2000);
        for dir in [&src, &dst] {
            for d in ["pkg2", "pkg2/lib"] {
                std::fs::set_permissions(dir.join(d), std::fs::Permissions::from_mode(0o755))
                    .unwrap();
            }
        }
    }

    /// Fails on one file; every worker stops and the error is returned.
    struct FailsOn(PathBuf, AtomicU64);

    impl FileCloner for FailsOn {
        fn clone_file(&self, src: &Path, dst: &Path) -> io::Result<()> {
            self.1.fetch_add(1, Ordering::Relaxed);
            if src == self.0 {
                return Err(io::Error::other("injected"));
            }
            copy_new(src, dst)
        }
    }

    #[test]
    fn stops_at_the_first_error_while_cloning_in_parallel() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir(&src).unwrap();
        for f in 0..5000 {
            std::fs::write(src.join(format!("{f}")), b"x").unwrap();
        }
        let first = std::fs::read_dir(&src)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let cloner = FailsOn(first, AtomicU64::new(0));
        let err = clone_tree(&src, &tmp.path().join("dst"), &cloner).unwrap_err();
        assert_eq!(err.to_string(), "injected");
        assert!(
            cloner.1.load(Ordering::Relaxed) < 5000,
            "workers did not stop"
        );
    }
}
