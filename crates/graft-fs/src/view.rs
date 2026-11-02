//! The read-only tree a virtual graft serves (ADR 0006).
//!
//! [`GraftView`] is the base commit's tree from Root's object store, with
//! each layer overlaid at its path from the layer cache. It is addressed by
//! inode numbers like a FUSE filesystem but has no FUSE dependency, so it
//! is tested directly on every platform. Directories are read from git the
//! first time they are looked up; inodes are allocated then and never
//! reused while the view lives.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use graft_core::CommitId;
use graft_root::tree::{EntryKind, Filter, ObjectHash, ObjectStore};

/// An inode number. [`ROOT_INO`] is the graft's top directory.
pub type Ino = u64;

/// The inode of the graft's top directory.
pub const ROOT_INO: Ino = 1;

/// Default budget for cached file contents.
pub const DEFAULT_CACHE_BYTES: usize = 64 << 20;

/// An `errno` value, for returning to the kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Errno(pub i32);

impl Errno {
    /// No such file or directory.
    pub const ENOENT: Self = Self(libc::ENOENT);
    /// Not a directory.
    pub const ENOTDIR: Self = Self(libc::ENOTDIR);
    /// Is a directory.
    pub const EISDIR: Self = Self(libc::EISDIR);
    /// Invalid argument (e.g. readlink of a non-link).
    pub const EINVAL: Self = Self(libc::EINVAL);
    /// I/O error: the cause is logged.
    pub const EIO: Self = Self(libc::EIO);
    /// Read-only filesystem.
    pub const EROFS: Self = Self(libc::EROFS);
}

impl From<&io::Error> for Errno {
    fn from(e: &io::Error) -> Self {
        Self(e.raw_os_error().unwrap_or(libc::EIO))
    }
}

/// Logs `e` and returns `EIO`: the kernel only gets a number, the log
/// keeps the reason.
fn eio(what: &str, e: &dyn std::fmt::Display) -> Errno {
    tracing::warn!(error = %e, "{what}");
    Errno::EIO
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

/// Where an inode's contents come from.
#[derive(Debug, Clone)]
enum Source {
    /// A git tree entry. `filtered` means attributes may apply, so contents
    /// go through the filter pipeline and size needs the converted bytes.
    Git {
        kind: EntryKind,
        id: ObjectHash,
        filtered: bool,
    },
    /// A file or directory inside a layer, served from the cache.
    Real(PathBuf),
    /// A directory that exists only to hold a layer (its parent path is not
    /// in the tree).
    Synthetic,
}

#[derive(Debug)]
struct Node {
    /// Path relative to the graft's top, as git sees it ('/'-separated).
    path: Vec<u8>,
    source: Source,
    /// Whether `.gitattributes` in this directory or an ancestor (or
    /// Root's config) means files below may be converted.
    attrs_scope: bool,
    children: Mutex<Option<Arc<Children>>>,
    size: OnceLock<u64>,
}

/// A directory's entries, sorted by name.
#[derive(Debug, Default)]
struct Children {
    entries: Vec<(OsString, Ino, NodeKind)>,
}

impl Children {
    fn find(&self, name: &OsStr) -> Option<Ino> {
        self.entries
            .binary_search_by(|(n, _, _)| n.as_os_str().cmp(name))
            .ok()
            .map(|i| self.entries[i].1)
    }
}

/// A small byte-bounded LRU of file contents.
#[derive(Debug)]
struct ContentCache {
    budget: usize,
    used: usize,
    tick: u64,
    entries: HashMap<Ino, (Arc<Vec<u8>>, u64)>,
}

impl ContentCache {
    fn get(&mut self, ino: Ino) -> Option<Arc<Vec<u8>>> {
        self.tick += 1;
        let tick = self.tick;
        self.entries.get_mut(&ino).map(|(data, used)| {
            *used = tick;
            Arc::clone(data)
        })
    }

    fn put(&mut self, ino: Ino, data: Arc<Vec<u8>>) {
        if data.len() > self.budget {
            return;
        }
        self.tick += 1;
        self.used += data.len();
        if let Some((old, _)) = self.entries.insert(ino, (data, self.tick)) {
            self.used -= old.len();
        }
        while self.used > self.budget {
            let Some((&victim, _)) = self.entries.iter().min_by_key(|(_, (_, t))| *t) else {
                break;
            };
            if let Some((old, _)) = self.entries.remove(&victim) {
                self.used -= old.len();
            }
        }
    }
}

/// The read-only tree of a virtual graft.
#[derive(Debug)]
pub struct GraftView {
    objects: ObjectStore,
    tree: ObjectHash,
    time: SystemTime,
    uid: u32,
    gid: u32,
    layers: Vec<LayerMount>,
    nodes: RwLock<Vec<Arc<Node>>>,
    cache: Mutex<ContentCache>,
    filter: OnceLock<Result<Filter, String>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn join(parent: &[u8], name: &OsStr) -> Vec<u8> {
    let name = graft_core::path_to_bytes(Path::new(name));
    if parent.is_empty() {
        return name;
    }
    let mut p = parent.to_vec();
    p.push(b'/');
    p.extend_from_slice(&name);
    p
}

/// Checks that a layer path is relative and has only normal components.
fn valid_layer_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|c| matches!(c, Component::Normal(n) if n != ".git"))
}

impl GraftView {
    /// A view of `base` from `objects`, with `layers` overlaid, owned by
    /// `uid`:`gid`.
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
        let attrs_scope = objects.attributes_configured();
        let top = Node {
            path: Vec::new(),
            source: Source::Git {
                kind: EntryKind::Tree,
                id: root.tree,
                filtered: attrs_scope,
            },
            attrs_scope,
            children: Mutex::new(None),
            size: OnceLock::new(),
        };
        Ok(Self {
            objects,
            tree: root.tree,
            time: UNIX_EPOCH + Duration::from_secs(secs),
            uid,
            gid,
            layers,
            // Index 0 is unused so that an inode number is its index.
            nodes: RwLock::new(vec![Arc::new(top)]),
            cache: Mutex::new(ContentCache {
                budget: DEFAULT_CACHE_BYTES,
                used: 0,
                tick: 0,
                entries: HashMap::new(),
            }),
            filter: OnceLock::new(),
        })
    }

    /// How many inodes have been allocated so far.
    #[must_use]
    pub fn allocated(&self) -> usize {
        self.nodes
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    fn node(&self, ino: Ino) -> Result<Arc<Node>, Errno> {
        let index =
            usize::try_from(ino.checked_sub(1).ok_or(Errno::ENOENT)?).map_err(|_| Errno::ENOENT)?;
        self.nodes
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(index)
            .cloned()
            .ok_or(Errno::ENOENT)
    }

    fn kind_of(source: &Source) -> Result<NodeKind, Errno> {
        Ok(match source {
            Source::Git { kind, .. } => match kind {
                EntryKind::Tree | EntryKind::Submodule => NodeKind::Directory,
                EntryKind::Blob | EntryKind::BlobExecutable => NodeKind::File,
                EntryKind::Symlink => NodeKind::Symlink,
            },
            Source::Synthetic => NodeKind::Directory,
            Source::Real(path) => {
                let ft = std::fs::symlink_metadata(path)
                    .map_err(|e| Errno::from(&e))?
                    .file_type();
                if ft.is_dir() {
                    NodeKind::Directory
                } else if ft.is_symlink() {
                    NodeKind::Symlink
                } else {
                    NodeKind::File
                }
            }
        })
    }

    /// Layers whose path is strictly below the directory at `dir`, as
    /// (first component below `dir`, the layer, whether that component is
    /// the layer itself).
    fn layers_below<'a>(&'a self, dir: &[u8]) -> Vec<(OsString, &'a LayerMount, bool)> {
        let dir_path = graft_core::path_from_bytes(dir.to_vec());
        self.layers
            .iter()
            .filter_map(|layer| {
                let rest = if dir.is_empty() {
                    layer.path.as_path()
                } else {
                    layer.path.strip_prefix(&dir_path).ok()?
                };
                let mut comps = rest.components();
                let first = comps.next()?.as_os_str().to_owned();
                Some((first, layer, comps.next().is_none()))
            })
            .collect()
    }

    /// Reads the entries of directory `node` (not yet allocated).
    fn read_entries(&self, node: &Node) -> Result<Vec<(OsString, Source, bool)>, Errno> {
        let mut out: Vec<(OsString, Source, bool)> = Vec::new();
        let mut scope = node.attrs_scope;
        match &node.source {
            Source::Git {
                kind: EntryKind::Tree,
                id,
                ..
            } => {
                let entries = self
                    .objects
                    .tree(*id)
                    .map_err(|e| eio("reading a tree", &e))?;
                scope = scope || entries.iter().any(|e| e.name == ".gitattributes");
                for e in entries {
                    out.push((
                        e.name,
                        Source::Git {
                            kind: e.kind,
                            id: e.id,
                            filtered: scope,
                        },
                        scope,
                    ));
                }
            }
            Source::Git {
                kind: EntryKind::Submodule,
                ..
            }
            | Source::Synthetic => {}
            Source::Real(path) => {
                let dir = std::fs::read_dir(path).map_err(|e| Errno::from(&e))?;
                for entry in dir {
                    let entry = entry.map_err(|e| Errno::from(&e))?;
                    let ft = entry.file_type().map_err(|e| Errno::from(&e))?;
                    if ft.is_dir() || ft.is_file() || ft.is_symlink() {
                        out.push((entry.file_name(), Source::Real(entry.path()), false));
                    }
                }
            }
            Source::Git { .. } => return Err(Errno::ENOTDIR),
        }
        if !matches!(node.source, Source::Real(_)) {
            for (first, layer, is_layer) in self.layers_below(&node.path) {
                let existing = out.iter().position(|(n, _, _)| *n == first);
                if is_layer {
                    if let Some(i) = existing {
                        tracing::warn!(layer = %layer.path.display(), "a layer hides a tracked path");
                        out.remove(i);
                    }
                    out.push((first, Source::Real(layer.source.clone()), false));
                } else {
                    match existing.map(|i| &out[i].1) {
                        None => out.push((first, Source::Synthetic, scope)),
                        Some(
                            Source::Git {
                                kind: EntryKind::Tree,
                                ..
                            }
                            | Source::Synthetic,
                        ) => {}
                        Some(_) => tracing::warn!(
                            layer = %layer.path.display(),
                            "a layer's parent is a file in the tree; the layer is not shown"
                        ),
                    }
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out.dedup_by(|a, b| a.0 == b.0);
        Ok(out)
    }

    fn children(&self, ino: Ino) -> Result<Arc<Children>, Errno> {
        let node = self.node(ino)?;
        if Self::kind_of(&node.source)? != NodeKind::Directory {
            return Err(Errno::ENOTDIR);
        }
        let mut slot = lock(&node.children);
        if let Some(children) = slot.as_ref() {
            return Ok(Arc::clone(children));
        }
        let entries = self.read_entries(&node)?;
        let mut kinds = Vec::with_capacity(entries.len());
        for (_, source, _) in &entries {
            kinds.push(Self::kind_of(source).unwrap_or(NodeKind::File));
        }
        let mut nodes = self.nodes.write().unwrap_or_else(PoisonError::into_inner);
        let mut children = Children::default();
        for ((name, source, attrs_scope), kind) in entries.into_iter().zip(kinds) {
            let path = join(&node.path, &name);
            nodes.push(Arc::new(Node {
                path,
                source,
                attrs_scope,
                children: Mutex::new(None),
                size: OnceLock::new(),
            }));
            children.entries.push((name, nodes.len() as Ino, kind));
        }
        drop(nodes);
        let children = Arc::new(children);
        *slot = Some(Arc::clone(&children));
        Ok(children)
    }

    fn filter(&self) -> Result<&Filter, Errno> {
        self.filter
            .get_or_init(|| Filter::spawn(&self.objects, self.tree).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| eio("starting the filter thread", e))
    }

    /// The working-tree contents of a git file, cached.
    fn git_contents(&self, ino: Ino, node: &Node) -> Result<Arc<Vec<u8>>, Errno> {
        let Source::Git {
            kind, id, filtered, ..
        } = &node.source
        else {
            return Err(Errno::EINVAL);
        };
        if let Some(data) = lock(&self.cache).get(ino) {
            return Ok(data);
        }
        let raw = self
            .objects
            .blob(*id)
            .map_err(|e| eio("reading a blob", &e))?;
        let data = if *filtered && *kind != EntryKind::Symlink {
            self.filter()?
                .to_worktree(&node.path, raw)
                .map_err(|e| eio("applying git filters", &e))?
        } else {
            raw
        };
        let data = Arc::new(data);
        lock(&self.cache).put(ino, Arc::clone(&data));
        Ok(data)
    }

    /// Attributes of inode `ino`.
    ///
    /// # Errors
    ///
    /// `ENOENT` for an unknown inode, `EIO` if git objects cannot be read,
    /// or the error from `lstat` for a layer file.
    pub fn getattr(&self, ino: Ino) -> Result<Attr, Errno> {
        let node = self.node(ino)?;
        let attr = |kind, size, perm| Attr {
            ino,
            kind,
            size,
            perm,
            nlink: 1,
            mtime: self.time,
            uid: self.uid,
            gid: self.gid,
        };
        match &node.source {
            Source::Git { kind, id, filtered } => Ok(match kind {
                EntryKind::Tree | EntryKind::Submodule => attr(NodeKind::Directory, 0, 0o755),
                EntryKind::Symlink | EntryKind::Blob | EntryKind::BlobExecutable => {
                    let size = if let Some(s) = node.size.get() {
                        *s
                    } else {
                        let s = if *filtered && *kind != EntryKind::Symlink {
                            self.git_contents(ino, &node)?.len() as u64
                        } else {
                            self.objects
                                .blob_size(*id)
                                .map_err(|e| eio("reading an object header", &e))?
                        };
                        *node.size.get_or_init(|| s)
                    };
                    match kind {
                        EntryKind::Symlink => attr(NodeKind::Symlink, size, 0o777),
                        EntryKind::BlobExecutable => attr(NodeKind::File, size, 0o755),
                        _ => attr(NodeKind::File, size, 0o644),
                    }
                }
            }),
            Source::Synthetic => Ok(attr(NodeKind::Directory, 0, 0o755)),
            Source::Real(path) => {
                let meta = std::fs::symlink_metadata(path).map_err(|e| Errno::from(&e))?;
                let kind = Self::kind_of(&node.source)?;
                #[cfg(unix)]
                let perm = {
                    use std::os::unix::fs::PermissionsExt;
                    u16::try_from(meta.permissions().mode() & 0o7777).unwrap_or(0o644)
                };
                #[cfg(not(unix))]
                let perm = 0o644;
                Ok(Attr {
                    mtime: meta.modified().unwrap_or(self.time),
                    ..attr(kind, meta.len(), perm)
                })
            }
        }
    }

    /// Looks up `name` in directory `parent`.
    ///
    /// # Errors
    ///
    /// `ENOENT` if there is no such entry, `ENOTDIR` if `parent` is not a
    /// directory, and the errors of [`GraftView::getattr`].
    pub fn lookup(&self, parent: Ino, name: &OsStr) -> Result<Attr, Errno> {
        let ino = self.children(parent)?.find(name).ok_or(Errno::ENOENT)?;
        self.getattr(ino)
    }

    /// The parent of directory `ino` (the top is its own parent).
    ///
    /// # Errors
    ///
    /// `ENOENT` for an unknown inode.
    pub fn parent(&self, ino: Ino) -> Result<Ino, Errno> {
        let node = self.node(ino)?;
        let Some(cut) = node.path.iter().rposition(|&b| b == b'/') else {
            return Ok(ROOT_INO);
        };
        // Walk from the top: parents were allocated before their children.
        let mut at = ROOT_INO;
        for comp in node.path[..cut].split(|&b| b == b'/') {
            let name = graft_core::path_from_bytes(comp.to_vec());
            at = self
                .children(at)?
                .find(name.as_os_str())
                .ok_or(Errno::ENOENT)?;
        }
        Ok(at)
    }

    /// The entries of directory `ino`, sorted by name, without `.` and `..`.
    ///
    /// # Errors
    ///
    /// `ENOTDIR` if it is not a directory, plus read errors.
    pub fn readdir(&self, ino: Ino) -> Result<Vec<DirEntry>, Errno> {
        Ok(self
            .children(ino)?
            .entries
            .iter()
            .map(|(name, ino, kind)| DirEntry {
                ino: *ino,
                kind: *kind,
                name: name.clone(),
            })
            .collect())
    }

    /// Up to `size` bytes of file `ino` from `offset`.
    ///
    /// # Errors
    ///
    /// `EISDIR` for a directory, `EINVAL` for a symlink, plus read errors.
    pub fn read(&self, ino: Ino, offset: u64, size: u32) -> Result<Vec<u8>, Errno> {
        let node = self.node(ino)?;
        match Self::kind_of(&node.source)? {
            NodeKind::Directory => return Err(Errno::EISDIR),
            NodeKind::Symlink => return Err(Errno::EINVAL),
            NodeKind::File => {}
        }
        if let Source::Real(path) = &node.source {
            return read_real(path, offset, size).map_err(|e| Errno::from(&e));
        }
        let data = self.git_contents(ino, &node)?;
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(data.len());
        let end = start.saturating_add(size as usize).min(data.len());
        Ok(data[start..end].to_vec())
    }

    /// The target of symlink `ino`.
    ///
    /// # Errors
    ///
    /// `EINVAL` if it is not a symlink, plus read errors.
    pub fn readlink(&self, ino: Ino) -> Result<Vec<u8>, Errno> {
        let node = self.node(ino)?;
        if Self::kind_of(&node.source)? != NodeKind::Symlink {
            return Err(Errno::EINVAL);
        }
        match &node.source {
            Source::Real(path) => std::fs::read_link(path)
                .map(|t| graft_core::path_to_bytes(&t))
                .map_err(|e| Errno::from(&e)),
            _ => Ok(self.git_contents(ino, &node)?.to_vec()),
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

fn read_real(path: &Path, offset: u64, size: u32) -> io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
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
        use std::io::{Read, Seek, SeekFrom};
        let mut file = file;
        file.seek(SeekFrom::Start(offset))?;
        filled = file.read(&mut buf)?;
    }
    buf.truncate(filled);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_cache_stays_within_budget() {
        let mut cache = ContentCache {
            budget: 10,
            used: 0,
            tick: 0,
            entries: HashMap::new(),
        };
        cache.put(1, Arc::new(vec![0; 4]));
        cache.put(2, Arc::new(vec![0; 4]));
        assert!(cache.get(1).is_some()); // 1 is now more recent than 2
        cache.put(3, Arc::new(vec![0; 4]));
        assert_eq!(cache.used, 8);
        assert!(cache.get(2).is_none(), "least recently used goes first");
        assert!(cache.get(1).is_some() && cache.get(3).is_some());
        cache.put(4, Arc::new(vec![0; 11]));
        assert!(
            cache.get(4).is_none(),
            "entries larger than the budget are not kept"
        );
    }

    #[test]
    fn layer_paths_must_be_plain() {
        assert!(valid_layer_path(Path::new("node_modules")));
        assert!(valid_layer_path(Path::new("web/node_modules")));
        for bad in ["", "/abs", "../up", "a/../b", ".git", "a/.git", "./x"] {
            assert!(!valid_layer_path(Path::new(bad)), "{bad}");
        }
    }
}
