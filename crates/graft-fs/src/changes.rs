//! What changed in a virtual graft, from its delta alone (ADR 0008).
//!
//! The candidates are the delta's paths and the base files they hide; each
//! is compared with the base tree as git would (contents in working-tree
//! form, and the executable bit). The working tree is never scanned.

use std::collections::BTreeSet;
use std::path::PathBuf;

use graft_core::CommitId;
use graft_core::GraftId;
use graft_root::graft::{Change, ChangeKind, FileDiff};
use graft_root::tree::ObjectStore;
use graft_store::Store;
use graft_store::blobs::BlobStore;
use graft_store::delta::{Content, Delta, Lower, LowerKind, Node, RelPath, join};

use crate::backend::{FsError, Result as FsResult};

use crate::lower::{Contents, LowerTree};
use crate::view::{Errno, LayerMount};

/// Errors working out changes.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ChangesError {
    /// The base commit could not be read.
    #[error(transparent)]
    Root(#[from] graft_root::RootError),
    /// A file's contents could not be read.
    #[error("reading {path}: errno {errno}")]
    Read {
        /// The graft-relative path.
        path: String,
        /// The error number.
        errno: i32,
    },
}

/// The changes of a virtual graft's `delta` against `base`, with the file
/// contents on both sides.
struct Compare<'a> {
    base: LowerTree,
    full: &'a LowerTree,
    delta: &'a Delta,
    blobs: &'a BlobStore,
}

/// A file as git sees it: contents and the executable bit.
#[derive(Debug, PartialEq, Eq)]
struct GitFile {
    bytes: Vec<u8>,
    exec: bool,
}

fn read_err(path: &[u8]) -> impl FnOnce(Errno) -> ChangesError + '_ {
    move |e| ChangesError::Read {
        path: String::from_utf8_lossy(path).into_owned(),
        errno: e.0,
    }
}

impl Compare<'_> {
    /// Every file path the delta can have changed, in order.
    fn candidates(&self) -> BTreeSet<RelPath> {
        let mut out = BTreeSet::new();
        for (path, _) in self.delta.entries() {
            out.insert(path.clone());
            // Base files below a path the delta replaced or deleted.
            let mut stack = vec![path.clone()];
            while let Some(dir) = stack.pop() {
                if let Some(LowerKind::Dir { .. }) = self.base.kind(&dir) {
                    for name in self.base.children(&dir) {
                        let p = join(&dir, &name);
                        out.insert(p.clone());
                        stack.push(p);
                    }
                }
            }
        }
        out
    }

    fn lower_file(tree: &LowerTree, path: &[u8]) -> Result<Option<GitFile>, ChangesError> {
        Ok(match tree.kind(path) {
            Some(LowerKind::File { mode }) => {
                let bytes = match tree.contents(path).map_err(read_err(path))? {
                    Contents::Bytes(b) => b.to_vec(),
                    Contents::File(f) => std::fs::read(f).map_err(|e| ChangesError::Read {
                        path: String::from_utf8_lossy(path).into_owned(),
                        errno: e.raw_os_error().unwrap_or(libc::EIO),
                    })?,
                };
                Some(GitFile {
                    bytes,
                    exec: mode & 0o111 != 0,
                })
            }
            Some(LowerKind::Symlink { target }) => Some(GitFile {
                bytes: target,
                exec: false,
            }),
            Some(LowerKind::Dir { .. }) | None => None,
        })
    }

    fn now_file(&self, path: &[u8]) -> Result<Option<GitFile>, ChangesError> {
        Ok(match self.delta.resolve(path, self.full) {
            Some(Node::File { content, mode, .. }) => {
                let bytes = match content {
                    Content::Blob(h) => {
                        std::fs::read(self.blobs.path(&h)).map_err(|e| ChangesError::Read {
                            path: String::from_utf8_lossy(path).into_owned(),
                            errno: e.raw_os_error().unwrap_or(libc::EIO),
                        })?
                    }
                    Content::Lower(p) => match Self::lower_file(self.full, &p)? {
                        Some(f) => f.bytes,
                        None => Vec::new(),
                    },
                };
                Some(GitFile {
                    bytes,
                    exec: mode & 0o111 != 0,
                })
            }
            Some(Node::Symlink { target, .. }) => Some(GitFile {
                bytes: target,
                exec: false,
            }),
            Some(Node::Dir { .. }) | None => None,
        })
    }

    fn diffs(&self) -> Result<Vec<FileDiff>, ChangesError> {
        let mut out = Vec::new();
        for path in self.candidates() {
            let old = Self::lower_file(&self.base, &path)?;
            let new = self.now_file(&path)?;
            let kind = match (&old, &new) {
                (None, Some(_)) => ChangeKind::Added,
                (Some(_), None) => ChangeKind::Deleted,
                (Some(a), Some(b)) if a != b => ChangeKind::Modified,
                _ => continue,
            };
            out.push(FileDiff {
                change: Change {
                    path: graft_core::path_from_bytes(path.clone()),
                    kind,
                },
                old: old.map(|f| f.bytes),
                new: new.map(|f| f.bytes),
            });
        }
        Ok(out)
    }
}

/// What `delta` changed against `base` (with `layers` overlaid on the
/// current side, as the graft sees them), with contents, sorted by path.
/// Paths under a directory the delta created are covered because the delta
/// has an entry for each of them.
///
/// # Errors
///
/// If the base commit or a file's contents cannot be read.
pub fn virtual_diffs(
    objects: &ObjectStore,
    base: &CommitId,
    layers: Vec<LayerMount>,
    delta: &Delta,
    blobs: &BlobStore,
) -> Result<Vec<FileDiff>, ChangesError> {
    let tree = objects.commit_tree(base)?.tree;
    let full = LowerTree::new(objects.clone(), tree, layers);
    diffs_over(&full, delta, blobs)
}

/// As [`virtual_diffs`], over an existing lower tree.
pub(crate) fn diffs_over(
    full: &LowerTree,
    delta: &Delta,
    blobs: &BlobStore,
) -> Result<Vec<FileDiff>, ChangesError> {
    Compare {
        base: LowerTree::new(full.objects().clone(), full.tree(), Vec::new()),
        full,
        delta,
        blobs,
    }
    .diffs()
}

/// As [`virtual_diffs`], without the contents.
///
/// # Errors
///
/// As for [`virtual_diffs`].
pub fn virtual_changes(
    objects: &ObjectStore,
    base: &CommitId,
    layers: Vec<LayerMount>,
    delta: &Delta,
    blobs: &BlobStore,
) -> Result<Vec<Change>, ChangesError> {
    Ok(virtual_diffs(objects, base, layers, delta, blobs)?
        .into_iter()
        .map(|d| d.change)
        .collect())
}

/// Drops added paths that `ignored` reports, as git status does.
#[must_use]
pub fn without_ignored<S: std::hash::BuildHasher>(
    diffs: Vec<FileDiff>,
    ignored: &std::collections::HashSet<PathBuf, S>,
) -> Vec<FileDiff> {
    diffs
        .into_iter()
        .filter(|d| d.change.kind != ChangeKind::Added || !ignored.contains(&d.change.path))
        .collect()
}

/// The layers of graft `id`, as mounts from the Root's layer cache under
/// `root_dir`.
///
/// # Errors
///
/// Store errors, or a referenced layer without a record.
pub fn layer_mounts(
    store: &Store,
    root_dir: &std::path::Path,
    id: GraftId,
) -> FsResult<Vec<LayerMount>> {
    let cache = graft_root::layer::LayerCache::new(root_dir.join("layers"));
    let all = store.layers()?;
    store
        .graft_layers(id)?
        .into_iter()
        .map(|key| {
            let record = all
                .iter()
                .find(|l| l.key == key)
                .ok_or_else(|| FsError::Io {
                    action: "finding a layer record for",
                    path: cache.path(&key),
                    source: std::io::Error::other("the layer is referenced but not recorded"),
                })?;
            Ok(LayerMount {
                path: record.path.clone(),
                source: cache.path(&key),
            })
        })
        .collect()
}

/// Bytes a virtual graft stores of its own: the distinct blobs its delta
/// references (counted in full even when another graft shares them) plus
/// its staging files and `.git` (`grafts/<id>/` under `root_dir`).
///
/// # Errors
///
/// Store and I/O errors.
pub fn virtual_disk_usage(store: &Store, root_dir: &std::path::Path, id: GraftId) -> FsResult<u64> {
    let blobs = BlobStore::new(root_dir.join("blobs"));
    let unique: std::collections::HashSet<_> = store.delta(id)?.blobs().collect();
    let mut bytes: u64 = unique
        .iter()
        .filter_map(|h| std::fs::metadata(blobs.path(h)).ok())
        .map(|m| m.len())
        .sum();
    let dir = root_dir.join("grafts").join(id.to_string());
    if dir.exists() {
        bytes += crate::unshared_bytes(&dir).map_err(|source| FsError::Io {
            action: "measuring",
            path: dir.clone(),
            source,
        })?;
    }
    Ok(bytes)
}
