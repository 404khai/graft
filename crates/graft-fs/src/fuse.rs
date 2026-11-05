//! The FUSE adapter for [`GraftView`] and the mount lifecycle (Linux).
//!
//! Every callback translates one kernel request into a [`GraftView`] call
//! and its result into a reply; errors become `errno` values and nothing
//! can panic across the boundary. A read-only view is also mounted `ro`, so
//! the kernel refuses writes before they reach Graft.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use fuser::{
    AccessFlags, Config, Errno as FuseErrno, FileAttr, FileHandle, FileType, Filesystem,
    FopenFlags, Generation, INodeNo, LockOwner, MountOption, OpenFlags, RenameFlags, ReplyAttr,
    ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyOpen, ReplyStatfs,
    ReplyWrite, ReplyXattr, Request, TimeOrNow, WriteFlags,
};

use crate::view::{Attr, Errno, GraftView, NodeKind};
use crate::virtual_backend::Unmounted;
use crate::write::{OpenMode, SetAttr};

/// How long the kernel may cache entries and attributes. Every change to a
/// graft arrives through this mount, so the kernel's caches stay coherent
/// (ADR 0008).
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

fn entry(reply: ReplyEntry, r: Result<Attr, Errno>) {
    match r {
        Ok(a) => reply.entry(&TTL, &file_attr(&a), Generation(0)),
        Err(e) => reply.error(errno(e)),
    }
}

fn empty(reply: ReplyEmpty, r: Result<(), Errno>) {
    match r {
        Ok(()) => reply.ok(),
        Err(e) => reply.error(errno(e)),
    }
}

fn when(t: TimeOrNow) -> SystemTime {
    match t {
        TimeOrNow::SpecificTime(t) => t,
        TimeOrNow::Now => SystemTime::now(),
    }
}

struct FuseFs {
    view: Arc<GraftView>,
}

impl Filesystem for FuseFs {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        entry(reply, self.view.lookup(parent.0, name));
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        match self.view.getattr(ino.0) {
            Ok(a) => reply.attr(&TTL, &file_attr(&a)),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn setattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<FileHandle>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<fuser::BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        let set = SetAttr {
            mode,
            uid,
            gid,
            size,
            mtime: mtime.map(when),
        };
        match self.view.setattr(ino.0, set) {
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

    fn mknod(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        mode: u32,
        umask: u32,
        _rdev: u32,
        reply: ReplyEntry,
    ) {
        entry(reply, self.view.mknod(parent.0, name, mode, umask));
    }

    fn mkdir(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        mode: u32,
        umask: u32,
        reply: ReplyEntry,
    ) {
        entry(reply, self.view.mkdir(parent.0, name, mode, umask));
    }

    fn unlink(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        empty(reply, self.view.unlink(parent.0, name));
    }

    fn rmdir(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        empty(reply, self.view.rmdir(parent.0, name));
    }

    fn symlink(
        &self,
        _req: &Request,
        parent: INodeNo,
        link_name: &OsStr,
        target: &Path,
        reply: ReplyEntry,
    ) {
        entry(reply, self.view.symlink(parent.0, link_name, target));
    }

    fn rename(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
        flags: RenameFlags,
        reply: ReplyEmpty,
    ) {
        if flags.intersects(RenameFlags::RENAME_EXCHANGE | RenameFlags::RENAME_WHITEOUT) {
            reply.error(errno(Errno::EINVAL));
            return;
        }
        let noreplace = flags.contains(RenameFlags::RENAME_NOREPLACE);
        empty(
            reply,
            self.view
                .rename(parent.0, name, newparent.0, newname, noreplace),
        );
    }

    fn link(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _newparent: INodeNo,
        _newname: &OsStr,
        reply: ReplyEntry,
    ) {
        // Hard links are not supported (ADR 0008); cargo and npm fall back
        // to copying.
        reply.error(errno(Errno::EPERM));
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        let mode = OpenMode::from_flags(flags.0);
        match self.view.open(ino.0, mode) {
            Ok(fh) => reply.opened(FileHandle(fh), FopenFlags::empty()),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn read(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        match self.view.read_handle(fh.0, offset, size) {
            Ok(data) => reply.data(&data),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn write(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        match self.view.write(fh.0, offset, data) {
            Ok(n) => reply.written(u32::try_from(n).unwrap_or(u32::MAX)),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn flush(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _lock_owner: LockOwner,
        reply: ReplyEmpty,
    ) {
        empty(reply, self.view.flush(fh.0));
    }

    fn release(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        empty(reply, self.view.release(fh.0));
    }

    fn fsync(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _datasync: bool,
        reply: ReplyEmpty,
    ) {
        empty(reply, self.view.fsync(fh.0));
    }

    fn opendir(&self, _req: &Request, ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        match self.view.opendir(ino.0) {
            Ok(fh) => reply.opened(FileHandle(fh), FopenFlags::empty()),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let entries = match self.view.dir_entries(fh.0) {
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

    fn releasedir(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _flags: OpenFlags,
        reply: ReplyEmpty,
    ) {
        self.view.releasedir(fh.0);
        reply.ok();
    }

    fn create(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        mode: u32,
        umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        match self.view.create(parent.0, name, mode, umask) {
            Ok((a, fh)) => reply.created(
                &TTL,
                &file_attr(&a),
                Generation(0),
                FileHandle(fh),
                FopenFlags::empty(),
            ),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn statfs(&self, _req: &Request, _ino: INodeNo, reply: ReplyStatfs) {
        let files = self.view.allocated() as u64;
        let free = self.view.free_bytes() / 4096;
        reply.statfs(
            free,
            free,
            free,
            files,
            u64::from(u32::MAX),
            4096,
            255,
            4096,
        );
    }

    fn setxattr(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _name: &OsStr,
        _value: &[u8],
        _flags: i32,
        _position: u32,
        reply: ReplyEmpty,
    ) {
        reply.error(errno(Errno::ENOTSUP));
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

    fn access(&self, _req: &Request, ino: INodeNo, mask: AccessFlags, reply: ReplyEmpty) {
        if mask.contains(AccessFlags::W_OK) && !self.view.is_writable() {
            reply.error(errno(Errno::EROFS));
            return;
        }
        // Permission bits are checked by the kernel (`default_permissions`).
        empty(reply, self.view.getattr(ino.0).map(|_| ()));
    }
}

/// A mounted graft. Dropping it detaches the mount lazily; prefer
/// [`Mounted::unmount`], which reports whether that was needed.
pub(crate) struct Mounted {
    session: Option<fuser::BackgroundSession>,
    path: PathBuf,
    view: Arc<GraftView>,
}

impl std::fmt::Debug for Mounted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mounted")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
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

/// Lazily detaches a mount at `path` that no live session serves.
pub(crate) fn detach_stale(path: &Path) -> io::Result<Option<Unmounted>> {
    if !needs_cleanup(path) {
        return Ok(None);
    }
    tracing::warn!(path = %path.display(), "detaching a stale mount");
    fusermount_unmount(path, true)?;
    Ok(Some(Unmounted::Lazy))
}

/// Mounts `view` read-only at `path` (an existing, empty directory),
/// detaching anything left mounted there by a daemon that died.
pub(crate) fn mount(view: Arc<GraftView>, path: &Path) -> io::Result<Mounted> {
    if needs_cleanup(path) {
        tracing::warn!(path = %path.display(), "detaching a stale mount");
        fusermount_unmount(path, true)?;
    }
    let mut config = Config::default();
    let rw = if view.is_writable() {
        MountOption::RW
    } else {
        MountOption::RO
    };
    config.mount_options = vec![
        rw,
        MountOption::NoSuid,
        MountOption::NoDev,
        MountOption::NoAtime,
        MountOption::DefaultPermissions,
        MountOption::FSName("graft".to_owned()),
        MountOption::Subtype("graft".to_owned()),
    ];
    config.n_threads = Some(4);
    let fs = FuseFs {
        view: Arc::clone(&view),
    };
    let session = fuser::spawn_mount(fs, path, &config)?;
    Ok(Mounted {
        session: Some(session),
        path: path.to_owned(),
        view,
    })
}

impl Mounted {
    /// The view it serves.
    pub(crate) fn view(&self) -> &Arc<GraftView> {
        &self.view
    }

    /// Journals pending writes. The kernel flushes FUSE files on close, so
    /// there is nothing to do.
    #[allow(clippy::unused_self, clippy::unnecessary_wraps)] // Same API as the NFS mount.
    pub(crate) fn sync(&self) -> Result<(), Errno> {
        Ok(())
    }

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
