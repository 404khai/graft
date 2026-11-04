//! The tree a virtual graft serves: its delta over the lower tree (ADRs
//! 0006 and 0008).
//!
//! [`GraftView`] is addressed by inode numbers like a FUSE filesystem but has
//! no FUSE dependency, so it is tested directly on every platform. Inodes
//! are allocated per path the first time it is looked up, follow the path
//! through renames, and are never reused while the view lives.
//!
//! A view made with [`GraftView::new`] is read-only. One made with
//! [`GraftView::writable`] journals every change through the graft's store
//! (see the `write` module) and serves `.git` from a real directory.

use std::collections::{BTreeMap, HashMap};
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use graft_core::{CommitId, GraftId};
use graft_root::tree::ObjectStore;
use graft_store::blobs::BlobStore;
use graft_store::delta::{Content, Delta, Node, RelPath, WriteKind, join, parent};
use graft_store::{Store, StoreError};

use crate::lower::{Contents, LowerTree, child, eio, lock, valid_layer_path};

pub use crate::lower::DEFAULT_CACHE_BYTES;

/// An inode number. [`ROOT_INO`] is the graft's top directory.
pub type Ino = u64;

/// The inode of the graft's top directory.
pub const ROOT_INO: Ino = 1;

/// An `errno` value, for returning to the kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Errno(pub i32);

impl Errno {
    /// Operation not permitted.
    pub const EPERM: Self = Self(libc::EPERM);
    /// No such file or directory.
    pub const ENOENT: Self = Self(libc::ENOENT);
    /// I/O error: the cause is logged.
    pub const EIO: Self = Self(libc::EIO);
    /// Bad file handle.
    pub const EBADF: Self = Self(libc::EBADF);
    /// Already exists.
    pub const EEXIST: Self = Self(libc::EEXIST);
    /// Cross-device link (between `.git` and the rest of the graft).
    pub const EXDEV: Self = Self(libc::EXDEV);
    /// Not a directory.
    pub const ENOTDIR: Self = Self(libc::ENOTDIR);
    /// Is a directory.
    pub const EISDIR: Self = Self(libc::EISDIR);
    /// Invalid argument (e.g. readlink of a non-link).
    pub const EINVAL: Self = Self(libc::EINVAL);
    /// Read-only filesystem.
    pub const EROFS: Self = Self(libc::EROFS);
    /// Name too long.
    pub const ENAMETOOLONG: Self = Self(libc::ENAMETOOLONG);
    /// Directory not empty.
    pub const ENOTEMPTY: Self = Self(libc::ENOTEMPTY);
    /// Not supported.
    pub const ENOTSUP: Self = Self(libc::ENOTSUP);
}

impl From<&io::Error> for Errno {
    fn from(e: &io::Error) -> Self {
        Self(e.raw_os_error().unwrap_or(libc::EIO))
    }
}

impl From<io::Error> for Errno {
    fn from(e: io::Error) -> Self {
        Self::from(&e)
    }
}

/// What an inode is, as far as `stat` is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A directory.
    Directory,
    /// A regular file.
    File,
    /// A symbolic link.
    Symlink,
}

/// Attributes of an inode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attr {
    /// Inode number.
    pub ino: Ino,
    /// What it is.
    pub kind: NodeKind,
    /// Size in bytes (for a symlink, the length of its target).
    pub size: u64,
    /// Permission bits (`0o644`, `0o755`, ...).
    pub perm: u16,
    /// Hard link count. Directories report 1, meaning "unknown", so tools
    /// such as `find` do not infer subdirectory counts from it.
    pub nlink: u32,
    /// Modification time.
    pub mtime: SystemTime,
    /// Owner.
    pub uid: u32,
    /// Group.
    pub gid: u32,
}

impl Attr {
    /// Allocated size in 512-byte blocks.
    #[must_use]
    pub fn blocks(&self) -> u64 {
        self.size.div_ceil(512)
    }
}

/// One entry of a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// The entry's inode.
    pub ino: Ino,
    /// What it is.
    pub kind: NodeKind,
    /// Its name.
    pub name: OsString,
}

/// A layer to overlay: `source` (a directory in the layer cache) appears at
/// `path` (relative) in the graft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerMount {
    /// Where the layer appears, relative to the graft's top.
    pub path: PathBuf,
    /// The layer's directory in the cache.
    pub source: PathBuf,
}

/// Creates a graft's `.git` directory the first time it is needed.
pub type GitInit = Box<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

/// Where a writable view keeps its state.
pub struct Journal {
    /// The Root's metadata store.
    pub store: Store,
    /// The Root's blob store.
    pub blobs: BlobStore,
    /// The graft.
    pub graft: GraftId,
    /// The graft's private directory (`grafts/<graft-id>/`): staging files
    /// go in `work/`, git state in `git/`.
    pub dir: PathBuf,
    /// Creates `.git` (in `dir/git`) on first use. `None` means the graft
    /// has no `.git`.
    pub git_init: Option<GitInit>,
}

impl std::fmt::Debug for Journal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Journal")
            .field("graft", &self.graft)
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

/// A file open for writing: its contents live in a staging file until they
/// are journaled (ADR 0008).
#[derive(Debug)]
pub(crate) struct StagedFile {
    pub(crate) id: u64,
    pub(crate) work: PathBuf,
    pub(crate) file: std::fs::File,
    /// Written since last journaled.
    pub(crate) dirty: AtomicBool,
    /// How the next journaled write is described.
    pub(crate) kind: Mutex<WriteKind>,
    /// Open handles using it.
    pub(crate) opens: AtomicUsize,
    /// Its path was deleted: nothing more is journaled.
    pub(crate) orphan: AtomicBool,
}

/// What an open handle reads from.
#[derive(Debug)]
pub(crate) enum Src {
    /// Lower contents held in memory.
    Bytes(Arc<Vec<u8>>),
    /// A blob, a layer file, or a file under `.git`.
    File(std::fs::File),
    /// A staging file.
    Staged(Arc<StagedFile>),
    /// A directory listing, snapshotted when opened.
    Dir(Vec<DirEntry>),
}

#[derive(Debug)]
pub(crate) struct Handle {
    pub(crate) ino: Ino,
    pub(crate) src: Src,
    pub(crate) write: bool,
}

/// Inode numbers by path and back.
#[derive(Debug, Default)]
pub(crate) struct Inodes {
    by_path: BTreeMap<RelPath, Ino>,
    by_ino: HashMap<Ino, RelPath>,
    /// Inodes whose path is gone but which are still open: their last
    /// attributes, for `fstat`.
    orphans: HashMap<Ino, Attr>,
    next: Ino,
}

impl Inodes {
    fn new() -> Self {
        let mut i = Self {
            next: ROOT_INO + 1,
            ..Self::default()
        };
        i.by_path.insert(Vec::new(), ROOT_INO);
        i.by_ino.insert(ROOT_INO, Vec::new());
        i
    }

    pub(crate) fn ino_for(&mut self, path: &[u8]) -> Ino {
        if let Some(i) = self.by_path.get(path) {
            return *i;
        }
        let ino = self.next;
        self.next += 1;
        self.by_path.insert(path.to_vec(), ino);
        self.by_ino.insert(ino, path.to_vec());
        ino
    }

    pub(crate) fn path_of(&self, ino: Ino) -> Option<&RelPath> {
        self.by_ino.get(&ino)
    }

    /// Inodes at or below `dir`.
    pub(crate) fn within(&self, dir: &[u8]) -> Vec<(RelPath, Ino)> {
        let mut out: Vec<(RelPath, Ino)> = Vec::new();
        if let Some(i) = self.by_path.get(dir) {
            out.push((dir.to_vec(), *i));
        }
        let mut start = dir.to_vec();
        start.push(b'/');
        for (p, i) in self.by_path.range(start.clone()..) {
            if !p.starts_with(&start) {
                break;
            }
            out.push((p.clone(), *i));
        }
        out
    }

    /// Forgets every path at or below `dir`.
    pub(crate) fn remove(&mut self, dir: &[u8]) -> Vec<Ino> {
        let gone = self.within(dir);
        for (p, i) in &gone {
            self.by_path.remove(p);
            self.by_ino.remove(i);
        }
        gone.into_iter().map(|(_, i)| i).collect()
    }

    /// Moves every path at or below `from` to below `to`.
    pub(crate) fn rename(&mut self, from: &[u8], to: &[u8]) {
        let moved = self.within(from);
        for (p, _) in &moved {
            self.by_path.remove(p);
        }
        for (p, i) in moved {
            let mut dest = to.to_vec();
            dest.extend_from_slice(&p[from.len()..]);
            self.by_path.insert(dest.clone(), i);
            self.by_ino.insert(i, dest);
        }
    }

    pub(crate) fn orphan(&mut self, ino: Ino, attr: Attr) {
        self.orphans.insert(ino, attr);
    }

    pub(crate) fn forget_orphan(&mut self, ino: Ino) {
        self.orphans.remove(&ino);
    }

    fn len(&self) -> usize {
        self.by_path.len()
    }
}

/// Mutable state of a writable view, behind its write lock.
pub(crate) struct Writer {
    pub(crate) store: Store,
    pub(crate) graft: GraftId,
    pub(crate) work: PathBuf,
    pub(crate) next_staged: u64,
    pub(crate) actor: Option<String>,
}

impl Writer {
    /// Forgets staging row `id`, logging failures (it is only bookkeeping).
    pub(crate) fn unstage(&mut self, id: u64) {
        let graft = self.graft;
        if let Err(e) = self.store.unstage(graft, id) {
            tracing::warn!(error = %e, "could not forget a staging file");
        }
    }
}

/// The write lock of a view, from [`GraftView::write_lock`].
pub struct WriteLock<'a>(#[allow(dead_code)] MutexGuard<'a, Writer>);

impl std::fmt::Debug for WriteLock<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WriteLock")
    }
}

/// The `.git` of a writable view: a real directory, created on first use.
pub(crate) struct GitDir {
    pub(crate) dir: PathBuf,
    init: GitInit,
    ready: OnceLock<Result<(), String>>,
}

impl GitDir {
    /// Creates the directory if it was not yet.
    pub(crate) fn ensure(&self) -> Result<&Path, Errno> {
        self.ready
            .get_or_init(|| {
                if self.dir.join("HEAD").is_file() {
                    return Ok(());
                }
                // A half-built directory from a crash is rebuilt.
                if self.dir.exists() {
                    std::fs::remove_dir_all(&self.dir).map_err(|e| e.to_string())?;
                }
                (self.init)(&self.dir)
            })
            .as_ref()
            .map_err(|e| eio("creating the graft's .git", e))?;
        Ok(&self.dir)
    }
}

/// The tree of a virtual graft.
pub struct GraftView {
    pub(crate) lower: LowerTree,
    pub(crate) time: SystemTime,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) delta: RwLock<Delta>,
    pub(crate) inodes: Mutex<Inodes>,
    pub(crate) staged: Mutex<HashMap<Ino, Arc<StagedFile>>>,
    pub(crate) handles: Mutex<HashMap<u64, Arc<Handle>>>,
    pub(crate) next_fh: AtomicU64,
    pub(crate) writer: Option<Mutex<Writer>>,
    /// The blob store, outside the write lock so reads never wait on it.
    pub(crate) blobs: Option<BlobStore>,
    pub(crate) git: Option<GitDir>,
}

impl std::fmt::Debug for GraftView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraftView")
            .field("lower", &self.lower)
            .field("writable", &self.writer.is_some())
            .finish_non_exhaustive()
    }
}

/// Errors creating a view.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ViewError {
    /// Reading the base commit failed, or a layer path is unsafe.
    #[error(transparent)]
    Root(#[from] graft_root::RootError),
    /// Reading or recovering the delta failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Preparing the graft's directory failed.
    #[error("preparing {}: {source}", path.display())]
    Io {
        /// The path.
        path: PathBuf,
        /// The error.
        #[source]
        source: io::Error,
    },
}

pub(crate) fn millis_to_time(ms: i64) -> SystemTime {
    match u64::try_from(ms) {
        Ok(ms) => UNIX_EPOCH + Duration::from_millis(ms),
        Err(_) => UNIX_EPOCH,
    }
}

pub(crate) fn os(bytes: &[u8]) -> OsString {
    graft_core::path_from_bytes(bytes.to_vec()).into_os_string()
}

impl GraftView {
    fn build(
        objects: ObjectStore,
        base: &CommitId,
        layers: Vec<LayerMount>,
        uid: u32,
        gid: u32,
    ) -> Result<Self, graft_root::RootError> {
        for layer in &layers {
            if !valid_layer_path(&layer.path) {
                return Err(graft_root::RootError::UnsafePath {
                    path: layer.path.display().to_string(),
                    reason: "a layer path must be relative, without `..` or `.git`".to_owned(),
                });
            }
        }
        let root = objects.commit_tree(base)?;
        let secs = u64::try_from(root.time).unwrap_or(0);
        Ok(Self {
            lower: LowerTree::new(objects, root.tree, layers),
            time: UNIX_EPOCH + Duration::from_secs(secs),
            uid,
            gid,
            delta: RwLock::new(Delta::default()),
            inodes: Mutex::new(Inodes::new()),
            staged: Mutex::new(HashMap::new()),
            handles: Mutex::new(HashMap::new()),
            next_fh: AtomicU64::new(1),
            writer: None,
            blobs: None,
            git: None,
        })
    }

    /// A read-only view of `base` from `objects`, with `layers` overlaid,
    /// owned by `uid`:`gid`.
    ///
    /// # Errors
    ///
    /// If the base commit cannot be read, or a layer path is not a plain
    /// relative path.
    pub fn new(
        objects: ObjectStore,
        base: &CommitId,
        layers: Vec<LayerMount>,
        uid: u32,
        gid: u32,
    ) -> Result<Self, graft_root::RootError> {
        Self::build(objects, base, layers, uid, gid)
    }

    /// A writable view: the graft's delta from `journal.store` over the
    /// lower tree. Files left staged by a crash are journaled first.
    ///
    /// # Errors
    ///
    /// As for [`GraftView::new`], plus store errors and failures to prepare
    /// the graft's directory.
    pub fn writable(
        objects: ObjectStore,
        base: &CommitId,
        layers: Vec<LayerMount>,
        uid: u32,
        gid: u32,
        journal: Journal,
    ) -> Result<Self, ViewError> {
        let mut view = Self::build(objects, base, layers, uid, gid)?;
        let work = journal.dir.join("work");
        crate::write::private_dir(&work).map_err(|source| ViewError::Io {
            path: work.clone(),
            source,
        })?;
        *view.delta.get_mut().unwrap_or_else(PoisonError::into_inner) =
            journal.store.delta(journal.graft)?;
        view.git = journal.git_init.map(|init| GitDir {
            dir: journal.dir.join("git"),
            init,
            ready: OnceLock::new(),
        });
        view.blobs = Some(journal.blobs);
        view.writer = Some(Mutex::new(Writer {
            store: journal.store,
            graft: journal.graft,
            work,
            next_staged: 1,
            actor: None,
        }));
        view.recover()?;
        Ok(view)
    }

    /// Whether changes are accepted.
    #[must_use]
    pub fn is_writable(&self) -> bool {
        self.writer.is_some()
    }

    /// How many inodes have been allocated so far.
    #[must_use]
    pub fn allocated(&self) -> usize {
        lock(&self.inodes).len()
    }

    /// Holds the write lock until dropped: no change can be journaled and
    /// no blob stored meanwhile. `None` for a read-only view.
    #[must_use]
    pub fn write_lock(&self) -> Option<WriteLock<'_>> {
        self.writer.as_ref().map(|w| WriteLock(lock(w)))
    }

    /// What the graft changed against its base, from the delta, with
    /// contents. Ignore rules are not applied (see
    /// [`crate::without_ignored`]).
    ///
    /// # Errors
    ///
    /// If a file's contents cannot be read.
    pub fn diffs(&self) -> Result<Vec<graft_root::graft::FileDiff>, crate::ChangesError> {
        let blobs = self
            .blobs
            .clone()
            .unwrap_or_else(|| BlobStore::new(PathBuf::new()));
        let delta = self.delta();
        crate::changes::diffs_over(&self.lower, &delta, &blobs)
    }

    /// The lower tree, for replaying an op log against it.
    #[must_use]
    pub fn lower(&self) -> &dyn graft_store::delta::Lower {
        &self.lower
    }

    /// A copy of the current delta.
    #[must_use]
    pub fn delta(&self) -> Delta {
        self.delta
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn path_of(&self, ino: Ino) -> Result<RelPath, Errno> {
        lock(&self.inodes)
            .path_of(ino)
            .cloned()
            .ok_or(Errno::ENOENT)
    }

    /// The part of `path` below `.git`, if `path` is in the graft's git
    /// directory.
    pub(crate) fn git_rel<'a>(&self, path: &'a [u8]) -> Option<&'a [u8]> {
        self.git.as_ref()?;
        if path == b".git" {
            Some(&[])
        } else {
            path.strip_prefix(b".git/")
        }
    }

    /// The real path under `.git` for `rel`, creating `.git` if needed.
    pub(crate) fn git_path(&self, rel: &[u8]) -> Result<PathBuf, Errno> {
        let git = self.git.as_ref().ok_or(Errno::ENOENT)?;
        let dir = git.ensure()?;
        Ok(if rel.is_empty() {
            dir.to_owned()
        } else {
            dir.join(graft_core::path_from_bytes(rel.to_vec()))
        })
    }

    pub(crate) fn resolve_node(&self, path: &[u8]) -> Option<Node> {
        self.delta
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .resolve(path, &self.lower)
    }

    fn real_attr(&self, ino: Ino, path: &Path) -> Result<Attr, Errno> {
        let meta = std::fs::symlink_metadata(path)?;
        let kind = if meta.is_dir() {
            NodeKind::Directory
        } else if meta.file_type().is_symlink() {
            NodeKind::Symlink
        } else {
            NodeKind::File
        };
        #[cfg(unix)]
        let perm = {
            use std::os::unix::fs::PermissionsExt;
            u16::try_from(meta.permissions().mode() & 0o7777).unwrap_or(0o644)
        };
        #[cfg(not(unix))]
        let perm = 0o644;
        Ok(Attr {
            ino,
            kind,
            size: meta.len(),
            perm,
            nlink: 1,
            mtime: meta.modified().unwrap_or(self.time),
            uid: self.uid,
            gid: self.gid,
        })
    }

    /// Attributes of the node at `path`, whose inode is `ino`.
    pub(crate) fn attr_at(&self, ino: Ino, path: &[u8]) -> Result<Attr, Errno> {
        if let Some(rel) = self.git_rel(path) {
            return self.real_attr(ino, &self.git_path(rel)?);
        }
        let node = self.resolve_node(path).ok_or(Errno::ENOENT)?;
        let attr = |kind, size, perm: u32, mtime: Option<i64>| Attr {
            ino,
            kind,
            size,
            perm: u16::try_from(perm & 0o7777).unwrap_or(0o644),
            nlink: 1,
            mtime: mtime.map_or(self.time, millis_to_time),
            uid: self.uid,
            gid: self.gid,
        };
        Ok(match node {
            Node::Dir { mode, mtime } => {
                let mut a = attr(NodeKind::Directory, 0, mode, mtime);
                if mtime.is_none()
                    && let Some(t) = self.lower.real_mtime(path)
                {
                    a.mtime = t;
                }
                a
            }
            Node::Symlink { target, mtime } => {
                attr(NodeKind::Symlink, target.len() as u64, 0o777, mtime)
            }
            Node::File {
                content,
                mode,
                mtime,
            } => {
                if let Some(s) = lock(&self.staged).get(&ino).cloned() {
                    let meta = s.file.metadata()?;
                    let mut a = attr(NodeKind::File, meta.len(), mode, mtime);
                    if s.dirty.load(Ordering::Acquire) {
                        a.mtime = meta.modified().unwrap_or(a.mtime);
                    }
                    return Ok(a);
                }
                let size = match &content {
                    Content::Blob(h) => std::fs::metadata(self.blobs_path(h)?)?.len(),
                    Content::Lower(p) => self.lower.size(p)?,
                };
                let mut a = attr(NodeKind::File, size, mode, mtime);
                if mtime.is_none()
                    && let Content::Lower(p) = &content
                    && let Some(t) = self.lower.real_mtime(p)
                {
                    a.mtime = t;
                }
                a
            }
        })
    }

    pub(crate) fn blobs_path(&self, h: &graft_core::BlobHash) -> Result<PathBuf, Errno> {
        Ok(self.blobs.as_ref().ok_or(Errno::EIO)?.path(h))
    }

    /// Attributes of inode `ino`.
    ///
    /// # Errors
    ///
    /// `ENOENT` for an unknown inode, `EIO` if git objects cannot be read,
    /// or the error from `lstat` for a real file.
    pub fn getattr(&self, ino: Ino) -> Result<Attr, Errno> {
        let path = {
            let inodes = lock(&self.inodes);
            match inodes.path_of(ino) {
                Some(p) => p.clone(),
                None => return inodes.orphans.get(&ino).copied().ok_or(Errno::ENOENT),
            }
        };
        self.attr_at(ino, &path)
    }

    /// Looks up `name` in directory `parent`.
    ///
    /// # Errors
    ///
    /// `ENOENT` if there is no such entry, `ENOTDIR` if `parent` is not a
    /// directory, and the errors of [`GraftView::getattr`].
    pub fn lookup(&self, parent: Ino, name: &OsStr) -> Result<Attr, Errno> {
        let dir = self.path_of(parent)?;
        if self.git_rel(&dir).is_none() {
            match self.resolve_node(&dir) {
                Some(n) if n.is_dir() => {}
                Some(_) => return Err(Errno::ENOTDIR),
                None => return Err(Errno::ENOENT),
            }
        }
        let path = child(&dir, name);
        if self.git_rel(&path).is_none() && self.resolve_node(&path).is_none() {
            return Err(Errno::ENOENT);
        }
        if let Some(rel) = self.git_rel(&path) {
            std::fs::symlink_metadata(self.git_path(rel)?)?;
        }
        let ino = lock(&self.inodes).ino_for(&path);
        self.attr_at(ino, &path)
    }

    /// The parent of directory `ino` (the top is its own parent).
    ///
    /// # Errors
    ///
    /// `ENOENT` for an unknown inode.
    pub fn parent(&self, ino: Ino) -> Result<Ino, Errno> {
        let path = self.path_of(ino)?;
        if path.is_empty() {
            return Ok(ROOT_INO);
        }
        Ok(lock(&self.inodes).ino_for(parent(&path)))
    }

    /// The entries of directory `ino`, sorted by name, without `.` and `..`.
    ///
    /// # Errors
    ///
    /// `ENOTDIR` if it is not a directory, plus read errors.
    pub fn readdir(&self, ino: Ino) -> Result<Vec<DirEntry>, Errno> {
        let dir = self.path_of(ino)?;
        if let Some(rel) = self.git_rel(&dir) {
            return self.real_readdir(&dir, &self.git_path(rel)?);
        }
        let names = {
            let delta = self.delta.read().unwrap_or_else(PoisonError::into_inner);
            match delta.resolve(&dir, &self.lower) {
                Some(n) if n.is_dir() => {}
                Some(_) => return Err(Errno::ENOTDIR),
                None => return Err(Errno::ENOENT),
            }
            delta.children(&dir, &self.lower)
        };
        let mut out = Vec::with_capacity(names.len() + 1);
        if dir.is_empty() && self.git.is_some() {
            let ino = lock(&self.inodes).ino_for(b".git");
            out.push(DirEntry {
                ino,
                kind: NodeKind::Directory,
                name: OsString::from(".git"),
            });
        }
        for name in names {
            let path = join(&dir, &name);
            let Some(node) = self.resolve_node(&path) else {
                continue;
            };
            let ino = lock(&self.inodes).ino_for(&path);
            out.push(DirEntry {
                ino,
                kind: match node {
                    Node::Dir { .. } => NodeKind::Directory,
                    Node::File { .. } => NodeKind::File,
                    Node::Symlink { .. } => NodeKind::Symlink,
                },
                name: os(&name),
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    fn real_readdir(&self, dir: &[u8], real: &Path) -> Result<Vec<DirEntry>, Errno> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(real)? {
            let entry = entry?;
            let ft = entry.file_type()?;
            let kind = if ft.is_dir() {
                NodeKind::Directory
            } else if ft.is_symlink() {
                NodeKind::Symlink
            } else {
                NodeKind::File
            };
            let path = child(dir, &entry.file_name());
            let ino = lock(&self.inodes).ino_for(&path);
            out.push(DirEntry {
                ino,
                kind,
                name: entry.file_name(),
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// The contents source for reading the node at `path` (inode `ino`).
    pub(crate) fn read_src(&self, ino: Ino, path: &[u8]) -> Result<Src, Errno> {
        if let Some(rel) = self.git_rel(path) {
            return Ok(Src::File(std::fs::File::open(self.git_path(rel)?)?));
        }
        if let Some(s) = lock(&self.staged).get(&ino).cloned() {
            return Ok(Src::Staged(s));
        }
        match self.resolve_node(path).ok_or(Errno::ENOENT)? {
            Node::Dir { .. } => Err(Errno::EISDIR),
            Node::Symlink { .. } => Err(Errno::EINVAL),
            Node::File {
                content: Content::Blob(h),
                ..
            } => Ok(Src::File(std::fs::File::open(self.blobs_path(&h)?)?)),
            Node::File {
                content: Content::Lower(p),
                ..
            } => Ok(match self.lower.contents(&p)? {
                Contents::Bytes(b) => Src::Bytes(b),
                Contents::File(f) => Src::File(std::fs::File::open(f)?),
            }),
        }
    }

    /// Up to `size` bytes of file `ino` from `offset`, without a handle.
    ///
    /// # Errors
    ///
    /// `EISDIR` for a directory, `EINVAL` for a symlink, plus read errors.
    pub fn read(&self, ino: Ino, offset: u64, size: u32) -> Result<Vec<u8>, Errno> {
        let path = self.path_of(ino)?;
        let src = self.read_src(ino, &path)?;
        read_from(&src, offset, size)
    }

    /// The target of symlink `ino`.
    ///
    /// # Errors
    ///
    /// `EINVAL` if it is not a symlink, plus read errors.
    pub fn readlink(&self, ino: Ino) -> Result<Vec<u8>, Errno> {
        let path = self.path_of(ino)?;
        if let Some(rel) = self.git_rel(&path) {
            let target = std::fs::read_link(self.git_path(rel)?).map_err(|e| {
                if e.kind() == io::ErrorKind::InvalidInput {
                    Errno::EINVAL
                } else {
                    Errno::from(&e)
                }
            })?;
            return Ok(graft_core::path_to_bytes(&target));
        }
        match self.resolve_node(&path).ok_or(Errno::ENOENT)? {
            Node::Symlink { target, .. } => Ok(target),
            _ => Err(Errno::EINVAL),
        }
    }

    /// Resolves a relative path from the top, without following symlinks.
    ///
    /// # Errors
    ///
    /// As for [`GraftView::lookup`].
    pub fn resolve(&self, path: &Path) -> Result<Attr, Errno> {
        let mut attr = self.getattr(ROOT_INO)?;
        for comp in path.components() {
            match comp {
                Component::Normal(name) => attr = self.lookup(attr.ino, name)?,
                Component::CurDir => {}
                _ => return Err(Errno::EINVAL),
            }
        }
        Ok(attr)
    }
}

/// Reads up to `size` bytes at `offset` from `src`.
pub(crate) fn read_from(src: &Src, offset: u64, size: u32) -> Result<Vec<u8>, Errno> {
    match src {
        Src::Bytes(data) => {
            let start = usize::try_from(offset)
                .unwrap_or(usize::MAX)
                .min(data.len());
            let end = start.saturating_add(size as usize).min(data.len());
            Ok(data[start..end].to_vec())
        }
        Src::File(f) => read_at(f, offset, size).map_err(Errno::from),
        Src::Staged(s) => read_at(&s.file, offset, size).map_err(Errno::from),
        Src::Dir(_) => Err(Errno::EISDIR),
    }
}

pub(crate) fn read_at(file: &std::fs::File, offset: u64, size: u32) -> io::Result<Vec<u8>> {
    let mut buf = vec![0; size as usize];
    let mut filled = 0;
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        while filled < buf.len() {
            match file.read_at(&mut buf[filled..], offset + filled as u64) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (file, offset);
    }
    buf.truncate(filled);
    Ok(buf)
}
