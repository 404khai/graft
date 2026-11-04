//! The lower tree of a virtual graft: the base commit's tree from Root's
//! object store with each layer overlaid at its path (ADR 0006). It never
//! changes while the graft lives; the delta sits on top (ADR 0008).
//!
//! Directories are read from git the first time they are needed and kept;
//! file contents go through a byte-bounded cache, with git filters applied
//! where attributes can reach.

use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use graft_root::tree::{EntryKind, Filter, ObjectHash, ObjectStore};
use graft_store::delta::{Lower, LowerKind, RelPath, join};

use crate::view::{Errno, LayerMount};

/// Default budget for cached file contents.
pub const DEFAULT_CACHE_BYTES: usize = 64 << 20;

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Logs `e` and returns `EIO`: the kernel only gets a number, the log keeps
/// the reason.
pub(crate) fn eio(what: &str, e: &dyn std::fmt::Display) -> Errno {
    tracing::warn!(error = %e, "{what}");
    Errno::EIO
}

/// Where a lower entry comes from.
#[derive(Debug, Clone)]
pub(crate) enum Source {
    /// A git tree entry. `filtered` means attributes may apply, so contents
    /// go through the filter pipeline and size needs the converted bytes.
    Git {
        kind: EntryKind,
        id: ObjectHash,
        filtered: bool,
    },
    /// A file or directory inside a layer, served from the cache.
    Real(PathBuf),
    /// A directory that exists only to hold a layer.
    Synthetic,
}

#[derive(Debug, Clone)]
struct LEntry {
    source: Source,
    /// Whether `.gitattributes` here or above (or Root's config) means files
    /// below may be converted.
    attrs_scope: bool,
}

type Listing = Arc<Vec<(Vec<u8>, LEntry)>>;

/// A small byte-bounded LRU of file contents.
#[derive(Debug)]
pub(crate) struct ContentCache {
    budget: usize,
    used: usize,
    tick: u64,
    entries: HashMap<RelPath, (Arc<Vec<u8>>, u64)>,
}

impl ContentCache {
    pub(crate) fn new(budget: usize) -> Self {
        Self {
            budget,
            used: 0,
            tick: 0,
            entries: HashMap::new(),
        }
    }

    fn get(&mut self, key: &[u8]) -> Option<Arc<Vec<u8>>> {
        self.tick += 1;
        let tick = self.tick;
        self.entries.get_mut(key).map(|(data, used)| {
            *used = tick;
            Arc::clone(data)
        })
    }

    fn put(&mut self, key: RelPath, data: Arc<Vec<u8>>) {
        if data.len() > self.budget {
            return;
        }
        self.tick += 1;
        self.used += data.len();
        if let Some((old, _)) = self.entries.insert(key, (data, self.tick)) {
            self.used -= old.len();
        }
        while self.used > self.budget {
            let Some(victim) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, t))| *t)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some((old, _)) = self.entries.remove(&victim) {
                self.used -= old.len();
            }
        }
    }
}

/// Checks that a layer path is relative and has only normal components.
pub(crate) fn valid_layer_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|c| matches!(c, Component::Normal(n) if n != ".git"))
}

/// What a lower path's contents are, for reading.
#[derive(Debug, Clone)]
pub(crate) enum Contents {
    /// Bytes held in memory (a git blob in working-tree form).
    Bytes(Arc<Vec<u8>>),
    /// A real file in a layer.
    File(PathBuf),
}

/// The lower tree.
#[derive(Debug)]
pub(crate) struct LowerTree {
    objects: ObjectStore,
    tree: ObjectHash,
    layers: Vec<LayerMount>,
    root_scope: bool,
    dirs: Mutex<HashMap<RelPath, Option<Listing>>>,
    cache: Mutex<ContentCache>,
    sizes: Mutex<HashMap<RelPath, u64>>,
    filter: OnceLock<Result<Filter, String>>,
}

fn bytes_of(name: &std::ffi::OsStr) -> Vec<u8> {
    graft_core::path_to_bytes(Path::new(name))
}

impl LowerTree {
    pub(crate) fn new(objects: ObjectStore, tree: ObjectHash, layers: Vec<LayerMount>) -> Self {
        let root_scope = objects.attributes_configured();
        Self {
            objects,
            tree,
            layers,
            root_scope,
            dirs: Mutex::new(HashMap::new()),
            cache: Mutex::new(ContentCache::new(DEFAULT_CACHE_BYTES)),
            sizes: Mutex::new(HashMap::new()),
            filter: OnceLock::new(),
        }
    }

    pub(crate) fn objects(&self) -> &ObjectStore {
        &self.objects
    }

    pub(crate) fn tree(&self) -> ObjectHash {
        self.tree
    }

    /// Layers strictly below `dir`: (first component below `dir`, the layer,
    /// whether that component is the layer itself).
    fn layers_below(&self, dir: &[u8]) -> Vec<(Vec<u8>, &LayerMount, bool)> {
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
                let first = bytes_of(comps.next()?.as_os_str());
                Some((first, layer, comps.next().is_none()))
            })
            .collect()
    }

    fn top(&self) -> LEntry {
        LEntry {
            source: Source::Git {
                kind: EntryKind::Tree,
                id: self.tree,
                filtered: self.root_scope,
            },
            attrs_scope: self.root_scope,
        }
    }

    fn entry(&self, path: &[u8]) -> Result<Option<LEntry>, Errno> {
        if path.is_empty() {
            return Ok(Some(self.top()));
        }
        let (dir, name) = match path.iter().rposition(|&b| b == b'/') {
            Some(i) => (&path[..i], &path[i + 1..]),
            None => (&[][..], path),
        };
        let Some(listing) = self.listing(dir)? else {
            return Ok(None);
        };
        Ok(listing
            .binary_search_by(|(n, _)| n.as_slice().cmp(name))
            .ok()
            .map(|i| listing[i].1.clone()))
    }

    /// The entries of the lower directory at `dir`, or `None` if there is no
    /// directory there.
    fn listing(&self, dir: &[u8]) -> Result<Option<Listing>, Errno> {
        if let Some(cached) = lock(&self.dirs).get(dir) {
            return Ok(cached.clone());
        }
        let listing = match self.entry(dir)? {
            Some(e) if Self::is_dir(&e.source) => Some(Arc::new(self.read_entries(dir, &e)?)),
            _ => None,
        };
        lock(&self.dirs).insert(dir.to_vec(), listing.clone());
        Ok(listing)
    }

    fn is_dir(source: &Source) -> bool {
        match source {
            Source::Git { kind, .. } => matches!(kind, EntryKind::Tree | EntryKind::Submodule),
            Source::Synthetic => true,
            Source::Real(p) => std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()),
        }
    }

    fn read_entries(&self, dir: &[u8], node: &LEntry) -> Result<Vec<(Vec<u8>, LEntry)>, Errno> {
        let mut out: Vec<(Vec<u8>, LEntry)> = Vec::new();
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
                        bytes_of(&e.name),
                        LEntry {
                            source: Source::Git {
                                kind: e.kind,
                                id: e.id,
                                filtered: scope,
                            },
                            attrs_scope: scope,
                        },
                    ));
                }
            }
            Source::Git { .. } | Source::Synthetic => {}
            Source::Real(path) => {
                for entry in std::fs::read_dir(path).map_err(|e| Errno::from(&e))? {
                    let entry = entry.map_err(|e| Errno::from(&e))?;
                    let ft = entry.file_type().map_err(|e| Errno::from(&e))?;
                    if ft.is_dir() || ft.is_file() || ft.is_symlink() {
                        out.push((
                            bytes_of(&entry.file_name()),
                            LEntry {
                                source: Source::Real(entry.path()),
                                attrs_scope: false,
                            },
                        ));
                    }
                }
            }
        }
        if !matches!(node.source, Source::Real(_)) {
            for (first, layer, is_layer) in self.layers_below(dir) {
                let existing = out.iter().position(|(n, _)| *n == first);
                if is_layer {
                    if let Some(i) = existing {
                        tracing::warn!(layer = %layer.path.display(), "a layer hides a tracked path");
                        out.remove(i);
                    }
                    out.push((
                        first,
                        LEntry {
                            source: Source::Real(layer.source.clone()),
                            attrs_scope: false,
                        },
                    ));
                } else {
                    match existing.map(|i| &out[i].1.source) {
                        None => out.push((
                            first,
                            LEntry {
                                source: Source::Synthetic,
                                attrs_scope: scope,
                            },
                        )),
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

    fn filter(&self) -> Result<&Filter, Errno> {
        self.filter
            .get_or_init(|| Filter::spawn(&self.objects, self.tree).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| eio("starting the filter thread", e))
    }

    fn git_bytes(
        &self,
        path: &[u8],
        kind: EntryKind,
        id: ObjectHash,
        filtered: bool,
    ) -> Result<Arc<Vec<u8>>, Errno> {
        if let Some(data) = lock(&self.cache).get(path) {
            return Ok(data);
        }
        let raw = self
            .objects
            .blob(id)
            .map_err(|e| eio("reading a blob", &e))?;
        let data = if filtered && kind != EntryKind::Symlink {
            self.filter()?
                .to_worktree(path, raw)
                .map_err(|e| eio("applying git filters", &e))?
        } else {
            raw
        };
        let data = Arc::new(data);
        lock(&self.cache).put(path.to_vec(), Arc::clone(&data));
        Ok(data)
    }

    /// The contents of the lower file at `path`.
    pub(crate) fn contents(&self, path: &[u8]) -> Result<Contents, Errno> {
        match self.entry(path)?.ok_or(Errno::ENOENT)?.source {
            Source::Git { kind, id, filtered } => {
                Ok(Contents::Bytes(self.git_bytes(path, kind, id, filtered)?))
            }
            Source::Real(p) => Ok(Contents::File(p)),
            Source::Synthetic => Err(Errno::EISDIR),
        }
    }

    /// The size of the lower file at `path`.
    pub(crate) fn size(&self, path: &[u8]) -> Result<u64, Errno> {
        if let Some(s) = lock(&self.sizes).get(path) {
            return Ok(*s);
        }
        let size = match self.entry(path)?.ok_or(Errno::ENOENT)?.source {
            Source::Git { kind, id, filtered } => {
                if filtered && kind != EntryKind::Symlink {
                    self.git_bytes(path, kind, id, filtered)?.len() as u64
                } else {
                    self.objects
                        .blob_size(id)
                        .map_err(|e| eio("reading an object header", &e))?
                }
            }
            Source::Real(p) => std::fs::symlink_metadata(&p)
                .map_err(|e| Errno::from(&e))?
                .len(),
            Source::Synthetic => 0,
        };
        lock(&self.sizes).insert(path.to_vec(), size);
        Ok(size)
    }

    /// The modification time of a layer path, if it is one.
    pub(crate) fn real_mtime(&self, path: &[u8]) -> Option<std::time::SystemTime> {
        match self.entry(path).ok()??.source {
            Source::Real(p) => std::fs::symlink_metadata(p).ok()?.modified().ok(),
            _ => None,
        }
    }

    fn kind_of(&self, path: &[u8], e: &LEntry) -> io::Result<LowerKind> {
        Ok(match &e.source {
            Source::Git { kind, id, .. } => match kind {
                EntryKind::Tree | EntryKind::Submodule => LowerKind::Dir { mode: 0o755 },
                EntryKind::Blob => LowerKind::File { mode: 0o644 },
                EntryKind::BlobExecutable => LowerKind::File { mode: 0o755 },
                EntryKind::Symlink => LowerKind::Symlink {
                    target: self
                        .objects
                        .blob(*id)
                        .map_err(|e| io::Error::other(e.to_string()))?,
                },
            },
            Source::Synthetic => LowerKind::Dir { mode: 0o755 },
            Source::Real(p) => {
                let meta = std::fs::symlink_metadata(p)?;
                #[cfg(unix)]
                let mode = {
                    use std::os::unix::fs::PermissionsExt;
                    meta.permissions().mode() & 0o7777
                };
                #[cfg(not(unix))]
                let mode = 0o644;
                if meta.is_dir() {
                    LowerKind::Dir { mode }
                } else if meta.file_type().is_symlink() {
                    LowerKind::Symlink {
                        target: graft_core::path_to_bytes(&std::fs::read_link(p)?),
                    }
                } else {
                    let _ = path;
                    LowerKind::File { mode }
                }
            }
        })
    }
}

impl Lower for LowerTree {
    fn kind(&self, path: &[u8]) -> Option<LowerKind> {
        let Ok(Some(e)) = self.entry(path) else {
            return None;
        };
        match self.kind_of(path, &e) {
            Ok(k) => Some(k),
            Err(err) => {
                tracing::warn!(path = %String::from_utf8_lossy(path), error = %err, "reading a lower entry");
                None
            }
        }
    }

    fn children(&self, path: &[u8]) -> Vec<Vec<u8>> {
        match self.listing(path) {
            Ok(Some(l)) => l.iter().map(|(n, _)| n.clone()).collect(),
            _ => Vec::new(),
        }
    }
}

/// Joins a graft-relative path and a file name.
pub(crate) fn child(dir: &[u8], name: &std::ffi::OsStr) -> RelPath {
    join(dir, &bytes_of(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_cache_stays_within_budget() {
        let mut cache = ContentCache::new(10);
        cache.put(b"1".to_vec(), Arc::new(vec![0; 4]));
        cache.put(b"2".to_vec(), Arc::new(vec![0; 4]));
        assert!(cache.get(b"1").is_some()); // 1 is now more recent than 2
        cache.put(b"3".to_vec(), Arc::new(vec![0; 4]));
        assert_eq!(cache.used, 8);
        assert!(cache.get(b"2").is_none(), "least recently used goes first");
        assert!(cache.get(b"1").is_some() && cache.get(b"3").is_some());
        cache.put(b"4".to_vec(), Arc::new(vec![0; 11]));
        assert!(
            cache.get(b"4").is_none(),
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
