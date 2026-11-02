//! The FUSE adapter for [`GraftView`] and the mount lifecycle (Linux).
//!
//! Every callback translates one kernel request into a [`GraftView`] call
//! and its result into a reply; errors become `errno` values and nothing
//! can panic across the boundary. The mount is read-only at the kernel
//! level (`ro`), so writes fail with `EROFS` before reaching Graft.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use fuser::{
    Config, Errno as FuseErrno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags, Generation,
    INodeNo, LockOwner, MountOption, OpenAccMode, OpenFlags, ReplyAttr, ReplyData, ReplyDirectory,
    ReplyEmpty, ReplyEntry, ReplyOpen, ReplyStatfs, ReplyXattr, Request,
};

use crate::view::{Attr, Errno, GraftView, NodeKind};

/// Attributes and entries never change in a read-only graft.
const TTL: Duration = Duration::from_secs(60);

fn errno(e: Errno) -> FuseErrno {
    FuseErrno::from_i32(e.0)
}

fn file_type(kind: NodeKind) -> FileType {
    match kind {
        NodeKind::Directory => FileType::Directory,
        NodeKind::File => FileType::RegularFile,
        NodeKind::Symlink => FileType::Symlink,
    }
}

fn file_attr(a: &Attr) -> FileAttr {
    FileAttr {
        ino: INodeNo(a.ino),
        size: a.size,
        blocks: a.blocks(),
        atime: a.mtime,
        mtime: a.mtime,
        ctime: a.mtime,
        crtime: a.mtime,
        kind: file_type(a.kind),
        perm: a.perm,
        nlink: a.nlink,
        uid: a.uid,
        gid: a.gid,
        rdev: 0,
        blksize: 4096,
        flags: 0,
    }
}

struct FuseFs {
    view: Arc<GraftView>,
}

impl Filesystem for FuseFs {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        match self.view.lookup(parent.0, name) {
            Ok(a) => reply.entry(&TTL, &file_attr(&a), Generation(0)),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        match self.view.getattr(ino.0) {
            Ok(a) => reply.attr(&TTL, &file_attr(&a)),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn readlink(&self, _req: &Request, ino: INodeNo, reply: ReplyData) {
        match self.view.readlink(ino.0) {
            Ok(target) => reply.data(&target),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        if !matches!(flags.acc_mode(), OpenAccMode::O_RDONLY) {
            reply.error(errno(Errno::EROFS));
            return;
        }
        match self.view.getattr(ino.0) {
            // Contents never change, so the page cache may keep them.
            Ok(_) => reply.opened(FileHandle(0), FopenFlags::FOPEN_KEEP_CACHE),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        match self.view.read(ino.0, offset, size) {
            Ok(data) => reply.data(&data),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let entries = match self.view.readdir(ino.0) {
            Ok(entries) => entries,
            Err(e) => {
                reply.error(errno(e));
                return;
            }
        };
        let parent = self.view.parent(ino.0).unwrap_or(ino.0);
        let dots = [(ino.0, "."), (parent, "..")];
        let all = dots
            .iter()
            .map(|(i, n)| (*i, FileType::Directory, OsStr::new(*n)))
            .chain(
                entries
                    .iter()
                    .map(|e| (e.ino, file_type(e.kind), e.name.as_os_str())),
            );
        let skip = usize::try_from(offset).unwrap_or(usize::MAX);
        for (i, (entry_ino, kind, name)) in all.enumerate().skip(skip) {
            // The offset passed back is that of the next entry.
            if reply.add(INodeNo(entry_ino), i as u64 + 1, kind, name) {
                break;
            }
        }
        reply.ok();
    }

    fn statfs(&self, _req: &Request, _ino: INodeNo, reply: ReplyStatfs) {
        let files = self.view.allocated() as u64;
        reply.statfs(0, 0, 0, files, 0, 4096, 255, 4096);
    }

    fn getxattr(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _name: &OsStr,
        _size: u32,
        reply: ReplyXattr,
    ) {
        reply.error(FuseErrno::from_i32(libc::ENODATA));
    }

    fn listxattr(&self, _req: &Request, _ino: INodeNo, size: u32, reply: ReplyXattr) {
        if size == 0 {
            reply.size(0);
        } else {
            reply.data(&[]);
        }
    }

    fn access(&self, _req: &Request, ino: INodeNo, mask: fuser::AccessFlags, reply: ReplyEmpty) {
        if mask.contains(fuser::AccessFlags::W_OK) {
            reply.error(errno(Errno::EROFS));
            return;
        }
        match self.view.getattr(ino.0) {
            Ok(_) => reply.ok(),
            Err(e) => reply.error(errno(e)),
        }
    }
}

/// A mounted graft. Dropping it detaches the mount lazily; prefer
/// [`Mounted::unmount`], which reports whether that was needed.
pub(crate) struct Mounted {
    session: Option<fuser::BackgroundSession>,
    path: PathBuf,
}

impl std::fmt::Debug for Mounted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mounted")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// How an unmount went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmounted {
    /// The mount is gone.
    Clean,
    /// The mount was busy and was detached lazily: processes with open
    /// files keep them until they close them.
    Lazy,
}

fn fusermount() -> std::ffi::OsString {
    if let Some(path) = std::env::var_os("FUSERMOUNT_PATH") {
        return path;
    }
    let on_path = |name: &str| {
        std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
    };
    if on_path("fusermount3") || !on_path("fusermount") {
        "fusermount3".into()
    } else {
        "fusermount".into()
    }
}

/// Runs `fusermount -u [-z] -- path`. `Ok(false)` if it refused because the
/// mount is busy.
fn fusermount_unmount(path: &Path, lazy: bool) -> io::Result<bool> {
    let mut cmd = Command::new(fusermount());
    cmd.arg("-u");
    if lazy {
        cmd.arg("-z");
    }
    let out = cmd.arg("--").arg(path).output()?;
    if out.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("busy") {
        return Ok(false);
    }
    Err(io::Error::other(format!(
        "fusermount -u {}: {}",
        path.display(),
        stderr.trim()
    )))
}

/// Whether `path` is a mount point whose FUSE server is gone (`ENOTCONN`),
/// or any other mount on top of the directory.
fn needs_cleanup(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match std::fs::symlink_metadata(path) {
        Err(e) => e.raw_os_error() == Some(libc::ENOTCONN),
        Ok(meta) => path
            .parent()
            .and_then(|p| std::fs::symlink_metadata(p).ok())
            .is_some_and(|parent| parent.dev() != meta.dev()),
    }
}

/// Mounts `view` read-only at `path` (an existing, empty directory),
/// detaching anything left mounted there by a daemon that died.
pub(crate) fn mount(view: GraftView, path: &Path) -> io::Result<Mounted> {
    if needs_cleanup(path) {
        tracing::warn!(path = %path.display(), "detaching a stale mount");
        fusermount_unmount(path, true)?;
    }
    let mut config = Config::default();
    config.mount_options = vec![
        MountOption::RO,
        MountOption::NoSuid,
        MountOption::NoDev,
        MountOption::NoAtime,
        MountOption::DefaultPermissions,
        MountOption::FSName("graft".to_owned()),
        MountOption::Subtype("graft".to_owned()),
    ];
    config.n_threads = Some(4);
    let fs = FuseFs {
        view: Arc::new(view),
    };
    let session = fuser::spawn_mount(fs, path, &config)?;
    Ok(Mounted {
        session: Some(session),
        path: path.to_owned(),
    })
}

impl Mounted {
    /// The mount point.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Unmounts, lazily if busy. The session thread is reaped in the
    /// background: after a lazy unmount it keeps serving open files.
    pub(crate) fn unmount(mut self) -> io::Result<Unmounted> {
        let how = if fusermount_unmount(&self.path, false)? {
            Unmounted::Clean
        } else {
            tracing::warn!(path = %self.path.display(), "graft is busy; unmounting lazily");
            if !fusermount_unmount(&self.path, true)? {
                return Err(io::Error::other(format!(
                    "{} is busy and could not be detached",
                    self.path.display()
                )));
            }
            Unmounted::Lazy
        };
        if let Some(session) = self.session.take() {
            // Joining first means the session's own unmount-on-drop finds
            // nothing mounted, so it can never detach a later mount here.
            let reaper = std::thread::Builder::new().name("graft-fuse-reaper".to_owned());
            if let Err(e) = reaper.spawn(move || {
                if let Err(e) = session.join() {
                    tracing::warn!(error = %e, "FUSE session ended with an error");
                }
            }) {
                tracing::warn!(error = %e, "could not start a thread to reap the FUSE session");
            }
        }
        Ok(how)
    }
}
