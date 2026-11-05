//! The NFS adapter for [`GraftView`] and the mount lifecycle (macOS,
//! ADR 0009).
//!
//! Each mounted graft gets its own NFS version 3 server on `127.0.0.1` (from
//! `nfsserve`), mounted with the system's `mount_nfs`. Every NFS procedure
//! becomes a [`GraftView`] call. NFS version 3 has no open or close, so the first
//! write to a file opens a view handle for it, and the handle is released
//! (journaling the file) once the file has been idle for [`IDLE`], or when
//! [`Mounted::sync`] is called.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use nfsserve::nfs::{
    fattr3, fileid3, filename3, ftype3, nfspath3, nfsstat3, nfsstring, nfstime3, sattr3, set_gid3,
    set_mode3, set_mtime, set_size3, set_uid3, specdata3,
};
use nfsserve::tcp::{NFSTcp, NFSTcpListener};
use nfsserve::vfs::{DirEntry as NfsDirEntry, NFSFileSystem, ReadDirResult, VFSCapabilities};

use crate::view::{Attr, Errno, GraftView, Ino, NodeKind, ROOT_INO};
use crate::virtual_backend::Unmounted;
use crate::write::{OpenMode, SetAttr};

/// How long a file must go without writes before it is journaled.
pub(crate) const IDLE: Duration = Duration::from_millis(500);

/// NFS client timeout, in tenths of a second, and retries: a dead server
/// turns into an error after about `timeo * (retrans + 1)`.
const TIMEO: u32 = 50;
const RETRANS: u32 = 3;

fn runtime() -> io::Result<&'static tokio::runtime::Runtime> {
    static RT: OnceLock<Result<tokio::runtime::Runtime, String>> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("graft-nfs")
            .enable_all()
            .build()
            .map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(|e| io::Error::other(e.clone()))
}

fn status(e: Errno) -> nfsstat3 {
    match e.0 {
        libc::EPERM => nfsstat3::NFS3ERR_PERM,
        libc::ENOENT => nfsstat3::NFS3ERR_NOENT,
        libc::EACCES => nfsstat3::NFS3ERR_ACCES,
        libc::EEXIST => nfsstat3::NFS3ERR_EXIST,
        libc::EXDEV => nfsstat3::NFS3ERR_XDEV,
        libc::ENOTDIR => nfsstat3::NFS3ERR_NOTDIR,
        libc::EISDIR => nfsstat3::NFS3ERR_ISDIR,
        libc::EINVAL => nfsstat3::NFS3ERR_INVAL,
        libc::EFBIG => nfsstat3::NFS3ERR_FBIG,
        libc::ENOSPC => nfsstat3::NFS3ERR_NOSPC,
        libc::EROFS => nfsstat3::NFS3ERR_ROFS,
        libc::ENAMETOOLONG => nfsstat3::NFS3ERR_NAMETOOLONG,
        libc::ENOTEMPTY => nfsstat3::NFS3ERR_NOTEMPTY,
        libc::ENOTSUP => nfsstat3::NFS3ERR_NOTSUPP,
        libc::EBADF => nfsstat3::NFS3ERR_BADHANDLE,
        _ => nfsstat3::NFS3ERR_IO,
    }
}

fn time(t: SystemTime) -> nfstime3 {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    nfstime3 {
        seconds: u32::try_from(d.as_secs()).unwrap_or(u32::MAX),
        nseconds: d.subsec_nanos(),
    }
}

fn from_time(t: nfstime3) -> SystemTime {
    UNIX_EPOCH + Duration::new(u64::from(t.seconds), t.nseconds)
}

fn fattr(a: &Attr) -> fattr3 {
    fattr3 {
        ftype: match a.kind {
            NodeKind::Directory => ftype3::NF3DIR,
            NodeKind::File => ftype3::NF3REG,
            NodeKind::Symlink => ftype3::NF3LNK,
        },
        mode: u32::from(a.perm),
        nlink: a.nlink,
        uid: a.uid,
        gid: a.gid,
        size: a.size,
        used: a.blocks() * 512,
        rdev: specdata3::default(),
        fsid: 0,
        fileid: a.ino,
        atime: time(a.mtime),
        mtime: time(a.mtime),
        ctime: time(a.mtime),
    }
}

fn is_appledouble(n: &filename3) -> bool {
    n.0.starts_with(b"._")
}

fn name(n: &filename3) -> &OsStr {
    OsStr::from_bytes(&n.0)
}

/// Runs a blocking view call from an NFS handler.
fn blocking<T>(f: impl FnOnce() -> T) -> T {
    tokio::task::block_in_place(f)
}

/// The NFS file system of one graft.
pub(crate) struct NfsFs {
    view: Arc<GraftView>,
    /// Files with an open write handle: inode -> (handle, last write).
    writers: Mutex<HashMap<Ino, (u64, Instant)>>,
}

impl NfsFs {
    fn writer(&self, ino: Ino) -> Result<u64, Errno> {
        let mut writers = self.writers.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((fh, at)) = writers.get_mut(&ino) {
            *at = Instant::now();
            return Ok(*fh);
        }
        let fh = self.view.open(
            ino,
            OpenMode {
                write: true,
                truncate: false,
            },
        )?;
        writers.insert(ino, (fh, Instant::now()));
        Ok(fh)
    }

    /// Releases (journals) write handles idle for at least `idle`. Returns
    /// the first error; every handle is released regardless.
    pub(crate) fn release_idle(&self, idle: Duration) -> Result<(), Errno> {
        let due: Vec<u64> = {
            let mut writers = self.writers.lock().unwrap_or_else(PoisonError::into_inner);
            let now = Instant::now();
            let due: Vec<Ino> = writers
                .iter()
                .filter(|(_, (_, at))| now.duration_since(*at) >= idle)
                .map(|(ino, _)| *ino)
                .collect();
            due.iter()
                .filter_map(|ino| writers.remove(ino).map(|(fh, _)| fh))
                .collect()
        };
        let mut result = Ok(());
        for fh in due {
            if let Err(e) = self.view.release(fh) {
                tracing::warn!(errno = e.0, "journaling a file written over NFS");
                result = result.and(Err(e));
            }
        }
        result
    }

    fn entry_attr(&self, ino: Ino) -> Result<fattr3, nfsstat3> {
        self.view.getattr(ino).map(|a| fattr(&a)).map_err(status)
    }
}

#[async_trait]
impl NFSFileSystem for NfsFs {
    fn capabilities(&self) -> VFSCapabilities {
        if self.view.is_writable() {
            VFSCapabilities::ReadWrite
        } else {
            VFSCapabilities::ReadOnly
        }
    }

    fn root_dir(&self) -> fileid3 {
        ROOT_INO
    }

    async fn lookup(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {
        blocking(|| match filename.0.as_slice() {
            b"." => Ok(dirid),
            b".." => self.view.parent(dirid),
            _ => self.view.lookup(dirid, name(filename)).map(|a| a.ino),
        })
        .map_err(status)
    }

    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {
        blocking(|| self.entry_attr(id))
    }

    async fn setattr(&self, id: fileid3, setattr: sattr3) -> Result<fattr3, nfsstat3> {
        let set = SetAttr {
            mode: match setattr.mode {
                set_mode3::mode(m) => Some(m),
                set_mode3::Void => None,
            },
            uid: match setattr.uid {
                set_uid3::uid(u) => Some(u),
                set_uid3::Void => None,
            },
            gid: match setattr.gid {
                set_gid3::gid(g) => Some(g),
                set_gid3::Void => None,
            },
            size: match setattr.size {
                set_size3::size(s) => Some(s),
                set_size3::Void => None,
            },
            mtime: match setattr.mtime {
                set_mtime::DONT_CHANGE => None,
                set_mtime::SET_TO_SERVER_TIME => Some(SystemTime::now()),
                set_mtime::SET_TO_CLIENT_TIME(t) => Some(from_time(t)),
            },
        };
        blocking(|| self.view.setattr(id, set).map(|a| fattr(&a))).map_err(status)
    }

    async fn read(
        &self,
        id: fileid3,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        blocking(|| {
            let data = self.view.read(id, offset, count)?;
            let size = self.view.getattr(id)?.size;
            let eof = offset + data.len() as u64 >= size;
            Ok((data, eof))
        })
        .map_err(status)
    }

    async fn write(&self, id: fileid3, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {
        blocking(|| {
            let fh = self.writer(id)?;
            self.view.write(fh, offset, data)?;
            self.view.getattr(id).map(|a| fattr(&a))
        })
        .map_err(status)
    }

    async fn create(
        &self,
        dirid: fileid3,
        filename: &filename3,
        attr: sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        // macOS stores extended attributes in AppleDouble `._name` files on
        // servers without xattrs. Grafts do not support xattrs (as on FUSE),
        // so these are refused rather than journaled as changes (ADR 0009).
        if is_appledouble(filename) {
            return Err(nfsstat3::NFS3ERR_ACCES);
        }
        let mode = match attr.mode {
            set_mode3::mode(m) => m,
            set_mode3::Void => 0o644,
        };
        blocking(|| {
            let (a, fh) = self.view.create(dirid, name(filename), mode, 0)?;
            self.writers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(a.ino, (fh, Instant::now()));
            Ok((a.ino, fattr(&a)))
        })
        .map_err(status)
    }

    async fn create_exclusive(
        &self,
        dirid: fileid3,
        filename: &filename3,
    ) -> Result<fileid3, nfsstat3> {
        self.create(dirid, filename, sattr3::default())
            .await
            .map(|(id, _)| id)
    }

    async fn mkdir(
        &self,
        dirid: fileid3,
        dirname: &filename3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        // nfsserve drops MKDIR's attributes (ADR 0009).
        blocking(|| self.view.mkdir(dirid, name(dirname), 0o755, 0))
            .map(|a| (a.ino, fattr(&a)))
            .map_err(status)
    }

    async fn remove(&self, dirid: fileid3, filename: &filename3) -> Result<(), nfsstat3> {
        // nfsserve sends REMOVE and RMDIR here alike.
        blocking(|| {
            let a = self.view.lookup(dirid, name(filename))?;
            if a.kind == NodeKind::Directory {
                self.view.rmdir(dirid, name(filename))
            } else {
                self.view.unlink(dirid, name(filename))
            }
        })
        .map_err(status)
    }

    async fn rename(
        &self,
        from_dirid: fileid3,
        from_filename: &filename3,
        to_dirid: fileid3,
        to_filename: &filename3,
    ) -> Result<(), nfsstat3> {
        blocking(|| {
            self.view.rename(
                from_dirid,
                name(from_filename),
                to_dirid,
                name(to_filename),
                false,
            )
        })
        .map_err(status)
    }

    async fn readdir(
        &self,
        dirid: fileid3,
        start_after: fileid3,
        max_entries: usize,
    ) -> Result<ReadDirResult, nfsstat3> {
        blocking(|| {
            let all = self.view.readdir(dirid).map_err(status)?;
            let start = if start_after == 0 {
                0
            } else {
                all.iter()
                    .position(|e| e.ino == start_after)
                    .map(|i| i + 1)
                    .ok_or(nfsstat3::NFS3ERR_BAD_COOKIE)?
            };
            let mut entries = Vec::new();
            for e in all.iter().skip(start).take(max_entries) {
                // An entry removed meanwhile is skipped, not an error.
                if let Ok(attr) = self.entry_attr(e.ino) {
                    entries.push(NfsDirEntry {
                        fileid: e.ino,
                        name: nfsstring(e.name.as_bytes().to_vec()),
                        attr,
                    });
                }
            }
            Ok(ReadDirResult {
                end: start + max_entries >= all.len(),
                entries,
            })
        })
    }

    async fn symlink(
        &self,
        dirid: fileid3,
        linkname: &filename3,
        symlink: &nfspath3,
        _attr: &sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        blocking(|| {
            self.view.symlink(
                dirid,
                name(linkname),
                Path::new(OsStr::from_bytes(&symlink.0)),
            )
        })
        .map(|a| (a.ino, fattr(&a)))
        .map_err(status)
    }

    async fn readlink(&self, id: fileid3) -> Result<nfspath3, nfsstat3> {
        blocking(|| self.view.readlink(id))
            .map(nfsstring)
            .map_err(status)
    }
}

/// Whether `path` is currently a mount point, from the mount table (never
/// touching the path, which could hang on a dead server).
fn is_mounted(path: &Path) -> bool {
    let Ok(out) = Command::new("/sbin/mount").output() else {
        return false;
    };
    let needle = format!(" on {} (", path.display());
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|l| l.contains(&needle))
}

/// Runs `umount [-f] path`; `Ok(false)` if it is busy.
fn umount(path: &Path, force: bool) -> io::Result<bool> {
    let mut cmd = Command::new("/sbin/umount");
    if force {
        cmd.arg("-f");
    }
    let out = cmd.arg(path).output()?;
    if out.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("busy") {
        return Ok(false);
    }
    Err(io::Error::other(format!(
        "umount {}: {}",
        path.display(),
        stderr.trim()
    )))
}

/// Forcibly detaches a mount at `path` left by a daemon that died.
pub(crate) fn detach_stale(path: &Path) -> io::Result<Option<Unmounted>> {
    if !is_mounted(path) {
        return Ok(None);
    }
    tracing::warn!(path = %path.display(), "detaching a stale mount");
    umount(path, true)?;
    Ok(Some(Unmounted::Lazy))
}

/// A mounted graft.
pub(crate) struct Mounted {
    path: PathBuf,
    view: Arc<GraftView>,
    fs: Arc<NfsFs>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl std::fmt::Debug for Mounted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mounted")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// Serves `view` over NFS and mounts it at `path` (an existing, empty
/// directory).
pub(crate) fn mount(view: Arc<GraftView>, path: &Path) -> io::Result<Mounted> {
    detach_stale(path)?;
    let rt = runtime()?;
    let fs = Arc::new(NfsFs {
        view: Arc::clone(&view),
        writers: Mutex::new(HashMap::new()),
    });
    let listener = rt.block_on(NFSTcpListener::bind(
        "127.0.0.1:0",
        SharedFs(Arc::clone(&fs)),
    ))?;
    let port = listener.get_listen_port();
    let server = rt.spawn(async move {
        if let Err(e) = listener.handle_forever().await {
            tracing::warn!(error = %e, "NFS server stopped");
        }
    });
    let flush_fs = Arc::clone(&fs);
    let flusher = rt.spawn(async move {
        let mut tick = tokio::time::interval(IDLE / 2);
        loop {
            tick.tick().await;
            let fs = Arc::clone(&flush_fs);
            let _ = tokio::task::spawn_blocking(move || fs.release_idle(IDLE)).await;
        }
    });
    let options = format!(
        "vers=3,tcp,port={port},mountport={port},locallocks,soft,timeo={TIMEO},retrans={RETRANS},nobrowse"
    );
    let out = Command::new("/sbin/mount_nfs")
        .args(["-o", &options, "localhost:/"])
        .arg(path)
        .output();
    let failed = match out {
        Ok(o) if o.status.success() => None,
        Ok(o) => Some(io::Error::other(format!(
            "mount_nfs {}: {}",
            path.display(),
            String::from_utf8_lossy(&o.stderr).trim()
        ))),
        Err(e) => Some(e),
    };
    if let Some(e) = failed {
        server.abort();
        flusher.abort();
        return Err(e);
    }
    Ok(Mounted {
        path: path.to_owned(),
        view,
        fs,
        tasks: vec![server, flusher],
    })
}

/// `nfsserve` takes its file system by value; this shares ours.
struct SharedFs(Arc<NfsFs>);

#[async_trait]
impl NFSFileSystem for SharedFs {
    fn capabilities(&self) -> VFSCapabilities {
        self.0.capabilities()
    }
    fn root_dir(&self) -> fileid3 {
        self.0.root_dir()
    }
    async fn lookup(&self, d: fileid3, f: &filename3) -> Result<fileid3, nfsstat3> {
        self.0.lookup(d, f).await
    }
    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {
        self.0.getattr(id).await
    }
    async fn setattr(&self, id: fileid3, a: sattr3) -> Result<fattr3, nfsstat3> {
        self.0.setattr(id, a).await
    }
    async fn read(&self, id: fileid3, o: u64, c: u32) -> Result<(Vec<u8>, bool), nfsstat3> {
        self.0.read(id, o, c).await
    }
    async fn write(&self, id: fileid3, o: u64, d: &[u8]) -> Result<fattr3, nfsstat3> {
        self.0.write(id, o, d).await
    }
    async fn create(
        &self,
        d: fileid3,
        f: &filename3,
        a: sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        self.0.create(d, f, a).await
    }
    async fn create_exclusive(&self, d: fileid3, f: &filename3) -> Result<fileid3, nfsstat3> {
        self.0.create_exclusive(d, f).await
    }
    async fn mkdir(&self, d: fileid3, f: &filename3) -> Result<(fileid3, fattr3), nfsstat3> {
        self.0.mkdir(d, f).await
    }
    async fn remove(&self, d: fileid3, f: &filename3) -> Result<(), nfsstat3> {
        self.0.remove(d, f).await
    }
    async fn rename(
        &self,
        fd: fileid3,
        ff: &filename3,
        td: fileid3,
        tf: &filename3,
    ) -> Result<(), nfsstat3> {
        self.0.rename(fd, ff, td, tf).await
    }
    async fn readdir(
        &self,
        d: fileid3,
        after: fileid3,
        max: usize,
    ) -> Result<ReadDirResult, nfsstat3> {
        self.0.readdir(d, after, max).await
    }
    async fn symlink(
        &self,
        d: fileid3,
        l: &filename3,
        t: &nfspath3,
        a: &sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        self.0.symlink(d, l, t, a).await
    }
    async fn readlink(&self, id: fileid3) -> Result<nfspath3, nfsstat3> {
        self.0.readlink(id).await
    }
}

impl Mounted {
    /// The view it serves.
    pub(crate) fn view(&self) -> &Arc<GraftView> {
        &self.view
    }

    /// The mount point.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Journals every file written over NFS so far.
    pub(crate) fn sync(&self) -> Result<(), Errno> {
        self.fs.release_idle(Duration::ZERO)
    }

    /// Journals pending writes, unmounts (forcibly if busy) and stops the
    /// server.
    pub(crate) fn unmount(self) -> io::Result<Unmounted> {
        if let Err(e) = self.sync() {
            tracing::warn!(errno = e.0, "journaling before unmount");
        }
        let how = if umount(&self.path, false)? {
            Unmounted::Clean
        } else {
            tracing::warn!(path = %self.path.display(), "graft is busy; unmounting forcibly");
            if !umount(&self.path, true)? {
                return Err(io::Error::other(format!(
                    "{} is busy and could not be unmounted",
                    self.path.display()
                )));
            }
            Unmounted::Lazy
        };
        for t in &self.tasks {
            t.abort();
        }
        Ok(how)
    }
}
