//! The write path of a [`GraftView`] (ADR 0008).
//!
//! Every metadata change runs under the view's write lock: it is validated
//! against the effective tree, applied to the in-memory delta, and journaled
//! with the changed delta rows in one transaction. If journaling fails the
//! in-memory change is reverted, so memory and disk never disagree.
//!
//! File contents are copied up into a staging file on first open for
//! writing; writes go straight to it, and it is hashed into a blob and
//! journaled when flushed, fsynced or released. `.git` is a real directory
//! and is changed in place, without journaling.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use graft_core::Timestamp;
use graft_store::delta::{Content, Node, Op, RelPath, WriteKind, is_within, parent};
use graft_store::{Journaled, Staged, StagedChange};

use crate::lower::{Contents, child, eio, lock};
use crate::view::{
    Attr, DirEntry, Errno, GraftView, Handle, Ino, Src, StagedFile, ViewError, Writer, read_from,
};

/// An inode about to lose its path: its last attributes if it is open, and
/// its staging file if it has one.
type Gone = (Ino, Option<Attr>, Option<Arc<StagedFile>>);

/// File type bits of a mode, and the regular-file type (POSIX values; `libc`
/// types them `u16` on macOS and `u32` on Linux).
const S_IFMT: u32 = 0o170_000;
const S_IFREG: u32 = 0o100_000;

/// Longest file name accepted, as on common local filesystems.
const NAME_MAX: usize = 255;

/// Changes requested by `setattr`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SetAttr {
    /// New permission bits.
    pub mode: Option<u32>,
    /// New owner (only the current owner is accepted).
    pub uid: Option<u32>,
    /// New group (only the current group is accepted).
    pub gid: Option<u32>,
    /// New size.
    pub size: Option<u64>,
    /// New modification time.
    pub mtime: Option<SystemTime>,
}

/// How a file is opened.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OpenMode {
    /// Opened for writing.
    pub write: bool,
    /// Truncated on open (`O_TRUNC`).
    pub truncate: bool,
}

impl OpenMode {
    /// From `open(2)` flags.
    #[must_use]
    pub fn from_flags(flags: i32) -> Self {
        let acc = flags & libc::O_ACCMODE;
        let write = acc == libc::O_WRONLY || acc == libc::O_RDWR;
        Self {
            write,
            truncate: write && flags & libc::O_TRUNC != 0,
        }
    }
}

/// Creates `dir` and its parents with mode `0700`.
pub(crate) fn private_dir(dir: &Path) -> io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)
}

fn millis(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn check_name(name: &OsStr) -> Result<(), Errno> {
    let n = name.len();
    if n == 0 || name == "." || name == ".." {
        return Err(Errno::EINVAL);
    }
    if n > NAME_MAX {
        return Err(Errno::ENAMETOOLONG);
    }
    Ok(())
}

fn open_rw(path: &Path) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
}

fn make_private(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(unix)]
fn create_mode(opts: &mut std::fs::OpenOptions, mode: u32) {
    std::os::unix::fs::OpenOptionsExt::mode(opts, mode);
}

#[cfg(not(unix))]
fn create_mode(_: &mut std::fs::OpenOptions, _: u32) {}

impl GraftView {
    fn writer(&self) -> Result<MutexGuard<'_, Writer>, Errno> {
        Ok(lock(self.writer.as_ref().ok_or(Errno::EROFS)?))
    }

    /// Sets the actor recorded on every following op (e.g. a `graft run`
    /// session ID), or clears it.
    pub fn set_actor(&self, actor: Option<String>) {
        if let Some(w) = &self.writer {
            lock(w).actor = actor;
        }
    }

    /// Applies `op` to the delta and journals it with `staged` bookkeeping.
    /// Reverts the in-memory change if journaling fails.
    fn commit(
        &self,
        w: &mut Writer,
        op: &Op,
        staged: &[StagedChange],
        sync: bool,
    ) -> Result<(), Errno> {
        let at = Timestamp::now();
        let mut delta = self.delta.write().unwrap_or_else(PoisonError::into_inner);
        let changes = delta.apply(op, at, &self.lower);
        let journaled = Journaled {
            at,
            op,
            actor: w.actor.as_deref(),
            changes: &changes,
            staged,
            sync,
        };
        if let Err(e) = w.store.record(w.graft, &journaled) {
            delta.revert(&changes);
            return Err(eio("journaling an operation", &e));
        }
        Ok(())
    }

    /// The parent directory of `path` must exist as a directory.
    fn check_parent(&self, path: &[u8]) -> Result<(), Errno> {
        match self.resolve_node(parent(path)) {
            Some(n) if n.is_dir() => Ok(()),
            Some(_) => Err(Errno::ENOTDIR),
            None => Err(Errno::ENOENT),
        }
    }

    fn new_child(&self, parent_ino: Ino, name: &OsStr) -> Result<RelPath, Errno> {
        check_name(name)?;
        let dir = self.path_of(parent_ino)?;
        Ok(child(&dir, name))
    }

    /// Creates a staging file for `path` holding `content` (or empty).
    fn new_staged(
        &self,
        w: &mut Writer,
        path: &[u8],
        mode: u32,
        content: Option<&Content>,
    ) -> Result<Arc<StagedFile>, Errno> {
        let id = w.next_staged;
        w.next_staged += 1;
        let work = w.work.join(id.to_string());
        let _ = std::fs::remove_file(&work);
        let filled = (|| -> Result<std::fs::File, Errno> {
            match content {
                None => {
                    let mut o = std::fs::OpenOptions::new();
                    o.read(true).write(true).create_new(true);
                    create_mode(&mut o, 0o600);
                    Ok(o.open(&work)?)
                }
                Some(Content::Blob(h)) => {
                    std::fs::copy(self.blobs_path(h)?, &work)?;
                    make_private(&work)?;
                    Ok(open_rw(&work)?)
                }
                Some(Content::Lower(p)) => match self.lower.contents(p)? {
                    Contents::Bytes(b) => {
                        std::fs::write(&work, b.as_slice())?;
                        make_private(&work)?;
                        Ok(open_rw(&work)?)
                    }
                    Contents::File(real) => {
                        std::fs::copy(real, &work)?;
                        make_private(&work)?;
                        Ok(open_rw(&work)?)
                    }
                },
            }
        })();
        let file = match filled {
            Ok(f) => f,
            Err(e) => {
                let _ = std::fs::remove_file(&work);
                return Err(e);
            }
        };
        let row = Staged {
            id,
            path: path.to_vec(),
            mode,
        };
        let graft = w.graft;
        if let Err(e) = w.store.stage(graft, &row) {
            let _ = std::fs::remove_file(&work);
            return Err(eio("recording a staging file", &e));
        }
        Ok(Arc::new(StagedFile {
            id,
            work,
            file,
            dirty: AtomicBool::new(false),
            kind: Mutex::new(WriteKind::Write),
            opens: AtomicUsize::new(0),
            orphan: AtomicBool::new(false),
        }))
    }

    /// The staging file for inode `ino` at `path`, copying the file up first
    /// if needed. Counts one more open.
    fn open_staged(
        &self,
        w: &mut Writer,
        ino: Ino,
        path: &[u8],
        truncate: bool,
    ) -> Result<Arc<StagedFile>, Errno> {
        let existing = lock(&self.staged).get(&ino).cloned();
        let s = if let Some(s) = existing {
            s
        } else {
            let (content, mode) = match self.resolve_node(path).ok_or(Errno::ENOENT)? {
                Node::File { content, mode, .. } => (content, mode),
                Node::Dir { .. } => return Err(Errno::EISDIR),
                Node::Symlink { .. } => return Err(Errno::EINVAL),
            };
            let source = if truncate { None } else { Some(&content) };
            let s = self.new_staged(w, path, mode, source)?;
            lock(&self.staged).insert(ino, Arc::clone(&s));
            s
        };
        s.opens.fetch_add(1, Ordering::AcqRel);
        if truncate {
            s.file.set_len(0)?;
            s.dirty.store(true, Ordering::Release);
            *lock(&s.kind) = WriteKind::Truncate;
        }
        Ok(s)
    }

    /// Journals the staging file's contents if they changed. With `drop`,
    /// the staging file is also retired (last close).
    fn finalize(
        &self,
        w: &mut Writer,
        ino: Ino,
        s: &StagedFile,
        sync: bool,
        drop: bool,
    ) -> Result<(), Errno> {
        let retire = |w: &mut Writer| {
            if drop {
                w.unstage(s.id);
            }
        };
        let path = lock(&self.inodes).path_of(ino).cloned();
        let Some(path) = path.filter(|_| !s.orphan.load(Ordering::Acquire)) else {
            retire(w);
            return Ok(());
        };
        if !s.dirty.swap(false, Ordering::AcqRel) {
            if sync {
                s.file.sync_all()?;
            }
            retire(w);
            return Ok(());
        }
        let stored = (|| {
            if sync {
                s.file.sync_data()?;
            }
            let blobs = self.blobs.as_ref().ok_or(Errno::EIO)?;
            let blob = blobs
                .put_file(&s.work, sync)
                .map_err(|e| eio("storing a blob", &e))?;
            let Some(Node::File { mode, .. }) = self.resolve_node(&path) else {
                return Err(Errno::ENOENT);
            };
            let kind = *lock(&s.kind);
            let op = Op::Write {
                kind,
                path: path.clone(),
                blob,
                mode,
            };
            let dropped = [StagedChange::Dropped { id: s.id }];
            let staged: &[StagedChange] = if drop { &dropped } else { &[] };
            self.commit(w, &op, staged, sync)
        })();
        match stored {
            Ok(()) => {
                *lock(&s.kind) = WriteKind::Write;
                Ok(())
            }
            Err(e) => {
                s.dirty.store(true, Ordering::Release);
                Err(e)
            }
        }
    }

    /// Journals files a crash left staged, then clears the staging area.
    pub(crate) fn recover(&mut self) -> Result<(), ViewError> {
        let Some(writer) = &self.writer else {
            return Ok(());
        };
        let mut w = lock(writer);
        let graft = w.graft;
        let rows = w.store.staged(graft)?;
        for row in rows {
            let work = w.work.join(row.id.to_string());
            let is_file = matches!(self.resolve_node(&row.path), Some(Node::File { .. }));
            if work.is_file() && is_file {
                let blob = self
                    .blobs
                    .as_ref()
                    .map(|b| b.put_file(&work, true))
                    .transpose()?;
                if let Some(blob) = blob {
                    let mode = match self.resolve_node(&row.path) {
                        Some(Node::File { mode, .. }) => mode,
                        _ => row.mode,
                    };
                    let op = Op::Write {
                        kind: WriteKind::Write,
                        path: row.path.clone(),
                        blob,
                        mode,
                    };
                    tracing::info!(path = %String::from_utf8_lossy(&row.path), "journaling a file left open by a crash");
                    self.commit(&mut w, &op, &[StagedChange::Dropped { id: row.id }], true)
                        .map_err(|e| ViewError::Io {
                            path: work.clone(),
                            source: io::Error::from_raw_os_error(e.0),
                        })?;
                }
            } else {
                let graft = w.graft;
                w.store.unstage(graft, row.id)?;
            }
        }
        if let Ok(dir) = std::fs::read_dir(&w.work) {
            for e in dir.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        w.next_staged = 1;
        Ok(())
    }

    fn add_handle(&self, ino: Ino, src: Src, write: bool) -> u64 {
        let fh = self.next_fh.fetch_add(1, Ordering::AcqRel);
        lock(&self.handles).insert(fh, Arc::new(Handle { ino, src, write }));
        fh
    }

    fn handle(&self, fh: u64) -> Result<Arc<Handle>, Errno> {
        lock(&self.handles).get(&fh).cloned().ok_or(Errno::EBADF)
    }

    fn has_handles(&self, ino: Ino) -> bool {
        lock(&self.handles).values().any(|h| h.ino == ino)
    }

    /// Opens file `ino`; returns a handle.
    ///
    /// # Errors
    ///
    /// `EROFS` for writing to a read-only view, `EISDIR` for a directory,
    /// plus read and copy-up errors.
    pub fn open(&self, ino: Ino, mode: OpenMode) -> Result<u64, Errno> {
        let path = self.path_of(ino)?;
        if let Some(rel) = self.git_rel(&path) {
            let real = self.git_path(rel)?;
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(mode.write)
                .truncate(mode.truncate)
                .open(real)?;
            return Ok(self.add_handle(ino, Src::File(file), mode.write));
        }
        if !mode.write {
            let src = self.read_src(ino, &path)?;
            return Ok(self.add_handle(ino, src, false));
        }
        let mut w = self.writer()?;
        let s = self.open_staged(&mut w, ino, &path, mode.truncate)?;
        drop(w);
        Ok(self.add_handle(ino, Src::Staged(s), true))
    }

    /// Opens directory `ino`, snapshotting its entries.
    ///
    /// # Errors
    ///
    /// As for [`GraftView::readdir`].
    pub fn opendir(&self, ino: Ino) -> Result<u64, Errno> {
        let entries = self.readdir(ino)?;
        Ok(self.add_handle(ino, Src::Dir(entries), false))
    }

    /// The entries snapshotted by [`GraftView::opendir`].
    ///
    /// # Errors
    ///
    /// `EBADF` for an unknown handle, `ENOTDIR` for a file handle.
    pub fn dir_entries(&self, fh: u64) -> Result<Vec<DirEntry>, Errno> {
        match &self.handle(fh)?.src {
            Src::Dir(e) => Ok(e.clone()),
            _ => Err(Errno::ENOTDIR),
        }
    }

    /// Reads through handle `fh`. A file another handle has open for
    /// writing is read from its staging file, so readers see every write.
    ///
    /// # Errors
    ///
    /// `EBADF` and read errors.
    pub fn read_handle(&self, fh: u64, offset: u64, size: u32) -> Result<Vec<u8>, Errno> {
        let h = self.handle(fh)?;
        if !matches!(h.src, Src::Staged(_) | Src::Dir(_))
            && let Some(s) = lock(&self.staged).get(&h.ino).cloned()
        {
            return read_from(&Src::Staged(s), offset, size);
        }
        read_from(&h.src, offset, size)
    }

    /// Writes through handle `fh`.
    ///
    /// # Errors
    ///
    /// `EBADF` if the handle is not open for writing, plus I/O errors.
    pub fn write(&self, fh: u64, offset: u64, data: &[u8]) -> Result<usize, Errno> {
        let h = self.handle(fh)?;
        if !h.write {
            return Err(Errno::EBADF);
        }
        let file = match &h.src {
            Src::Staged(s) => {
                s.dirty.store(true, Ordering::Release);
                &s.file
            }
            Src::File(f) => f,
            _ => return Err(Errno::EBADF),
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            file.write_all_at(data, offset)?;
        }
        #[cfg(not(unix))]
        let _ = (file, offset);
        Ok(data.len())
    }

    /// Journals what was written through `fh` (on `close`).
    ///
    /// # Errors
    ///
    /// Journaling errors.
    pub fn flush(&self, fh: u64) -> Result<(), Errno> {
        let h = self.handle(fh)?;
        if let Src::Staged(s) = &h.src {
            let mut w = self.writer()?;
            self.finalize(&mut w, h.ino, s, false, false)?;
        }
        Ok(())
    }

    /// Makes what was written through `fh` durable.
    ///
    /// # Errors
    ///
    /// Journaling and sync errors.
    pub fn fsync(&self, fh: u64) -> Result<(), Errno> {
        let h = self.handle(fh)?;
        match &h.src {
            Src::Staged(s) => {
                let mut w = self.writer()?;
                self.finalize(&mut w, h.ino, s, true, false)
            }
            Src::File(f) if h.write => Ok(f.sync_all()?),
            _ => Ok(()),
        }
    }

    /// Closes handle `fh`, journaling and retiring a staging file on its
    /// last close.
    ///
    /// # Errors
    ///
    /// Journaling errors (the handle is closed regardless).
    pub fn release(&self, fh: u64) -> Result<(), Errno> {
        let Some(h) = lock(&self.handles).remove(&fh) else {
            return Ok(());
        };
        let mut result = Ok(());
        if let Src::Staged(s) = &h.src {
            // Under the write lock, so no open can revive it meanwhile.
            let mut w = self.writer()?;
            if s.opens.fetch_sub(1, Ordering::AcqRel) != 1 {
                return Ok(());
            }
            result = self.finalize(&mut w, h.ino, s, false, true);
            let mut staged = lock(&self.staged);
            if staged.get(&h.ino).is_some_and(|cur| Arc::ptr_eq(cur, s))
                && s.opens.load(Ordering::Acquire) == 0
            {
                staged.remove(&h.ino);
                let _ = std::fs::remove_file(&s.work);
            }
        }
        if !self.has_handles(h.ino) {
            lock(&self.inodes).forget_orphan(h.ino);
        }
        result
    }

    /// Releases a directory handle.
    pub fn releasedir(&self, fh: u64) {
        lock(&self.handles).remove(&fh);
    }

    /// Creates and opens a regular file.
    ///
    /// # Errors
    ///
    /// `EEXIST`, `ENOENT`/`ENOTDIR` for a bad parent, `EROFS`, I/O errors.
    pub fn create(
        &self,
        parent: Ino,
        name: &OsStr,
        mode: u32,
        umask: u32,
    ) -> Result<(Attr, u64), Errno> {
        let path = self.new_child(parent, name)?;
        let mode = mode & !umask & 0o7777;
        if let Some(rel) = self.git_rel(&path) {
            let mut o = std::fs::OpenOptions::new();
            o.read(true).write(true).create_new(true);
            create_mode(&mut o, mode);
            let file = o.open(self.git_path(rel)?)?;
            let ino = lock(&self.inodes).ino_for(&path);
            let fh = self.add_handle(ino, Src::File(file), true);
            return Ok((self.attr_at(ino, &path)?, fh));
        }
        let mut w = self.writer()?;
        self.check_parent(&path)?;
        if self.resolve_node(&path).is_some() {
            return Err(Errno::EEXIST);
        }
        let s = self.new_staged(&mut w, &path, mode, None)?;
        let blobs = self.blobs.as_ref().ok_or(Errno::EIO)?;
        let created = blobs
            .put_bytes(b"")
            .map_err(|e| eio("storing a blob", &e))
            .and_then(|blob| {
                self.commit(
                    &mut w,
                    &Op::Write {
                        kind: WriteKind::Create,
                        path: path.clone(),
                        blob,
                        mode,
                    },
                    &[],
                    false,
                )
            });
        if let Err(e) = created {
            w.unstage(s.id);
            let _ = std::fs::remove_file(&s.work);
            return Err(e);
        }
        let ino = lock(&self.inodes).ino_for(&path);
        s.opens.store(1, Ordering::Release);
        lock(&self.staged).insert(ino, Arc::clone(&s));
        drop(w);
        let fh = self.add_handle(ino, Src::Staged(s), true);
        Ok((self.attr_at(ino, &path)?, fh))
    }

    /// Creates a regular file without opening it (`mknod` of `S_IFREG`).
    /// Other node types are refused with `EPERM`.
    ///
    /// # Errors
    ///
    /// As for [`GraftView::create`].
    pub fn mknod(&self, parent: Ino, name: &OsStr, mode: u32, umask: u32) -> Result<Attr, Errno> {
        if mode & S_IFMT != S_IFREG {
            return Err(Errno::EPERM);
        }
        let (attr, fh) = self.create(parent, name, mode, umask)?;
        self.release(fh)?;
        self.getattr(attr.ino)
    }

    /// Creates a directory.
    ///
    /// # Errors
    ///
    /// `EEXIST`, a bad parent, `EROFS`, journaling errors.
    pub fn mkdir(&self, parent: Ino, name: &OsStr, mode: u32, umask: u32) -> Result<Attr, Errno> {
        let path = self.new_child(parent, name)?;
        let mode = mode & !umask & 0o7777;
        if let Some(rel) = self.git_rel(&path) {
            let mut b = std::fs::DirBuilder::new();
            #[cfg(unix)]
            std::os::unix::fs::DirBuilderExt::mode(&mut b, mode);
            b.create(self.git_path(rel)?)?;
        } else {
            let mut w = self.writer()?;
            self.check_parent(&path)?;
            if self.resolve_node(&path).is_some() {
                return Err(Errno::EEXIST);
            }
            self.commit(
                &mut w,
                &Op::Mkdir {
                    path: path.clone(),
                    mode,
                },
                &[],
                false,
            )?;
        }
        let ino = lock(&self.inodes).ino_for(&path);
        self.attr_at(ino, &path)
    }

    /// Creates a symbolic link.
    ///
    /// # Errors
    ///
    /// `EEXIST`, a bad parent, `EROFS`, journaling errors.
    pub fn symlink(&self, parent: Ino, name: &OsStr, target: &Path) -> Result<Attr, Errno> {
        let path = self.new_child(parent, name)?;
        if let Some(rel) = self.git_rel(&path) {
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, self.git_path(rel)?)?;
        } else {
            let mut w = self.writer()?;
            self.check_parent(&path)?;
            if self.resolve_node(&path).is_some() {
                return Err(Errno::EEXIST);
            }
            let op = Op::Symlink {
                path: path.clone(),
                target: graft_core::path_to_bytes(target),
            };
            self.commit(&mut w, &op, &[], false)?;
        }
        let ino = lock(&self.inodes).ino_for(&path);
        self.attr_at(ino, &path)
    }

    /// Before `dir` is deleted or replaced: staging bookkeeping for files
    /// open below it, and the inodes to orphan with their attributes.
    fn orphans_below(&self, dir: &[u8]) -> (Vec<StagedChange>, Vec<Gone>) {
        let inodes = lock(&self.inodes).within(dir);
        let mut changes = Vec::new();
        let mut gone = Vec::new();
        for (path, ino) in inodes {
            let s = lock(&self.staged).get(&ino).cloned();
            if let Some(s) = &s {
                changes.push(StagedChange::Dropped { id: s.id });
            }
            let attr = if s.is_some() || self.has_handles(ino) {
                self.attr_at(ino, &path).ok()
            } else {
                None
            };
            gone.push((ino, attr, s));
        }
        (changes, gone)
    }

    fn orphan(&self, dir: &[u8], gone: Vec<Gone>) {
        let mut inodes = lock(&self.inodes);
        inodes.remove(dir);
        for (ino, attr, s) in gone {
            if let Some(s) = s {
                s.orphan.store(true, Ordering::Release);
            }
            if let Some(a) = attr {
                inodes.orphan(ino, a);
            }
        }
    }

    fn remove(&self, parent: Ino, name: &OsStr, dir: bool) -> Result<(), Errno> {
        let path = self.new_child(parent, name)?;
        if let Some(rel) = self.git_rel(&path) {
            if rel.is_empty() {
                return Err(Errno::EPERM);
            }
            let real = self.git_path(rel)?;
            if dir {
                std::fs::remove_dir(real)?;
            } else {
                std::fs::remove_file(real)?;
            }
            lock(&self.inodes).remove(&path);
            return Ok(());
        }
        let mut w = self.writer()?;
        match (self.resolve_node(&path).ok_or(Errno::ENOENT)?, dir) {
            (Node::Dir { .. }, false) => return Err(Errno::EISDIR),
            (Node::File { .. } | Node::Symlink { .. }, true) => return Err(Errno::ENOTDIR),
            (Node::Dir { .. }, true) => {
                let delta = self.delta.read().unwrap_or_else(PoisonError::into_inner);
                if !delta.children(&path, &self.lower).is_empty() {
                    return Err(Errno::ENOTEMPTY);
                }
            }
            _ => {}
        }
        let (staged, gone) = self.orphans_below(&path);
        let op = if dir {
            Op::Rmdir { path: path.clone() }
        } else {
            Op::Unlink { path: path.clone() }
        };
        self.commit(&mut w, &op, &staged, false)?;
        self.orphan(&path, gone);
        Ok(())
    }

    /// Removes a file or symlink.
    ///
    /// # Errors
    ///
    /// `ENOENT`, `EISDIR`, `EROFS`, journaling errors.
    pub fn unlink(&self, parent: Ino, name: &OsStr) -> Result<(), Errno> {
        self.remove(parent, name, false)
    }

    /// Removes an empty directory.
    ///
    /// # Errors
    ///
    /// `ENOENT`, `ENOTDIR`, `ENOTEMPTY`, `EROFS`, journaling errors.
    pub fn rmdir(&self, parent: Ino, name: &OsStr) -> Result<(), Errno> {
        self.remove(parent, name, true)
    }

    /// Renames `parent/name` to `new_parent/new_name`. With `noreplace`, an
    /// existing destination is an error (`RENAME_NOREPLACE`).
    ///
    /// # Errors
    ///
    /// POSIX rename errors, `EXDEV` across `.git`, `EROFS`, journaling
    /// errors.
    pub fn rename(
        &self,
        parent: Ino,
        name: &OsStr,
        new_parent: Ino,
        new_name: &OsStr,
        noreplace: bool,
    ) -> Result<(), Errno> {
        let from = self.new_child(parent, name)?;
        let to = self.new_child(new_parent, new_name)?;
        match (self.git_rel(&from), self.git_rel(&to)) {
            (Some(a), Some(b)) => {
                if a.is_empty() || b.is_empty() {
                    return Err(Errno::EPERM);
                }
                let dest = self.git_path(b)?;
                if noreplace && std::fs::symlink_metadata(&dest).is_ok() {
                    return Err(Errno::EEXIST);
                }
                std::fs::rename(self.git_path(a)?, dest)?;
                let mut inodes = lock(&self.inodes);
                inodes.remove(&to);
                inodes.rename(&from, &to);
                return Ok(());
            }
            (None, None) => {}
            _ => return Err(Errno::EXDEV),
        }
        let mut w = self.writer()?;
        let node = self.resolve_node(&from).ok_or(Errno::ENOENT)?;
        self.check_parent(&to)?;
        if from == to {
            return Ok(());
        }
        if is_within(&to, &from) {
            return Err(Errno::EINVAL);
        }
        if let Some(existing) = self.resolve_node(&to) {
            if noreplace {
                return Err(Errno::EEXIST);
            }
            match (node.is_dir(), existing.is_dir()) {
                (true, true) => {
                    let delta = self.delta.read().unwrap_or_else(PoisonError::into_inner);
                    if !delta.children(&to, &self.lower).is_empty() {
                        return Err(Errno::ENOTEMPTY);
                    }
                }
                (true, false) => return Err(Errno::ENOTDIR),
                (false, true) => return Err(Errno::EISDIR),
                (false, false) => {}
            }
        }
        let (mut staged, gone) = self.orphans_below(&to);
        for (p, ino) in lock(&self.inodes).within(&from) {
            if let Some(s) = lock(&self.staged).get(&ino) {
                let mut dest = to.clone();
                dest.extend_from_slice(&p[from.len()..]);
                staged.push(StagedChange::Moved {
                    id: s.id,
                    path: dest,
                });
            }
        }
        let op = Op::Rename {
            from: from.clone(),
            to: to.clone(),
        };
        self.commit(&mut w, &op, &staged, false)?;
        self.orphan(&to, gone);
        lock(&self.inodes).rename(&from, &to);
        Ok(())
    }

    /// Changes attributes of `ino`.
    ///
    /// # Errors
    ///
    /// `EPERM` for an ownership change, `EROFS`, I/O and journaling errors.
    pub fn setattr(&self, ino: Ino, set: SetAttr) -> Result<Attr, Errno> {
        if set.uid.is_some_and(|u| u != self.uid) || set.gid.is_some_and(|g| g != self.gid) {
            return Err(Errno::EPERM);
        }
        let path = self.path_of(ino)?;
        if let Some(rel) = self.git_rel(&path) {
            let real = self.git_path(rel)?;
            #[cfg(unix)]
            if let Some(mode) = set.mode {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&real, std::fs::Permissions::from_mode(mode & 0o7777))?;
            }
            if let Some(size) = set.size {
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(&real)?
                    .set_len(size)?;
            }
            if let Some(t) = set.mtime {
                std::fs::File::open(&real)?.set_modified(t)?;
            }
            return self.attr_at(ino, &path);
        }
        let mut w = self.writer()?;
        if let Some(size) = set.size {
            self.truncate(&mut w, ino, &path, size)?;
        }
        if let Some(mode) = set.mode {
            self.commit(
                &mut w,
                &Op::Chmod {
                    path: path.clone(),
                    mode: mode & 0o7777,
                },
                &[],
                false,
            )?;
        }
        if let Some(t) = set.mtime {
            // Journal pending writes first, so they do not override the
            // time being set.
            if let Some(s) = lock(&self.staged).get(&ino).cloned() {
                self.finalize(&mut w, ino, &s, false, false)?;
            }
            self.commit(
                &mut w,
                &Op::Touch {
                    path: path.clone(),
                    mtime: millis(t),
                },
                &[],
                false,
            )?;
        }
        drop(w);
        self.attr_at(ino, &path)
    }

    fn truncate(&self, w: &mut Writer, ino: Ino, path: &[u8], size: u64) -> Result<(), Errno> {
        let was_open = lock(&self.staged).contains_key(&ino);
        let s = self.open_staged(w, ino, path, false)?;
        let result = (|| {
            s.file.set_len(size)?;
            s.dirty.store(true, Ordering::Release);
            *lock(&s.kind) = WriteKind::Truncate;
            if was_open {
                Ok(())
            } else {
                self.finalize(w, ino, &s, false, false)
            }
        })();
        if s.opens.fetch_sub(1, Ordering::AcqRel) == 1 {
            w.unstage(s.id);
            lock(&self.staged).remove(&ino);
            let _ = std::fs::remove_file(&s.work);
        }
        result
    }

    /// Bytes free where the graft's changes are stored.
    #[must_use]
    pub fn free_bytes(&self) -> u64 {
        let Some(w) = &self.writer else {
            return 0;
        };
        let work = lock(w).work.clone();
        crate::available_bytes(&work).unwrap_or(0)
    }
}
