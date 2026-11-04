//! Lazy, thread-safe reads of trees and blobs from Root's object store, for
//! virtual grafts (ADR 0006).
//!
//! [`ObjectStore`] answers one directory or one blob at a time, so serving
//! a graft costs only what is looked at. [`Filter`] applies git's
//! working-tree conversion (`eol`, `text`, smudge filters) with attributes
//! taken from the base tree, as a checkout of that tree would.

use std::ffi::OsString;
use std::fmt;
use std::sync::mpsc;

use gix::bstr::{BStr, BString, ByteSlice};
use gix::filter::plumbing::pipeline::convert::to_worktree::Options as ToWorktreeOptions;
use gix::objs::tree::EntryKind as GixKind;
use graft_core::CommitId;

use crate::{Repo, Result, RootError, git_err};

fn boxed(
    e: impl std::error::Error + Send + Sync + 'static,
) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(e)
}

/// The ID of a git object (tree or blob).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectHash(gix::ObjectId);

impl fmt::Debug for ObjectHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectHash({})", self.0)
    }
}

impl fmt::Display for ObjectHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// What a tree entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// A subdirectory.
    Tree,
    /// A regular file.
    Blob,
    /// A regular file with the executable bit.
    BlobExecutable,
    /// A symbolic link; the blob holds its target.
    Symlink,
    /// A submodule commit; shown as an empty directory.
    Submodule,
}

/// One entry of a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    /// The entry's name: a single path component.
    pub name: OsString,
    /// What it is.
    pub kind: EntryKind,
    /// Its object.
    pub id: ObjectHash,
}

/// A commit's root tree and the time to show for its files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitTree {
    /// The root tree.
    pub tree: ObjectHash,
    /// Committer time, in seconds since the Unix epoch.
    pub time: i64,
}

/// Thread-safe, read-only access to Root's object store.
#[derive(Clone)]
pub struct ObjectStore {
    repo: gix::ThreadSafeRepository,
    workdir: std::path::PathBuf,
    attributes_configured: bool,
}

impl fmt::Debug for ObjectStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObjectStore")
            .field("git_dir", &self.repo.git_dir())
            .field("workdir", &self.workdir)
            .field("attributes_configured", &self.attributes_configured)
            .finish()
    }
}

/// Whether config or files outside the tree can give paths attributes, so
/// that conversion may apply to a file in any directory.
fn attributes_configured(repo: &gix::Repository) -> bool {
    let config = repo.config_snapshot();
    let autocrlf = config.string("core.autocrlf").is_some_and(|v| {
        !matches!(
            v.to_str_lossy().to_lowercase().as_str(),
            "false" | "no" | "off" | "0"
        )
    });
    let configured = autocrlf
        || config.string("core.eol").is_some()
        || config.string("core.attributesFile").is_some();
    let info = repo.common_dir().join("info/attributes").is_file();
    // git's default global attributes file.
    let global = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .is_some_and(|dir| dir.join("git/attributes").is_file());
    configured || info || global
}

impl ObjectStore {
    /// Opens Root's object store for lazy reads.
    #[must_use]
    pub fn open(root: &Repo) -> Self {
        Self {
            attributes_configured: attributes_configured(&root.repo),
            workdir: root.workdir.clone(),
            repo: root.repo.clone().into_sync(),
        }
    }

    /// Root, opened for this thread.
    #[must_use]
    pub fn root(&self) -> Repo {
        Repo {
            repo: self.local(),
            workdir: self.workdir.clone(),
        }
    }

    fn local(&self) -> gix::Repository {
        self.repo.to_thread_local()
    }

    /// Whether Root's config (or `info/attributes`, or the user's global
    /// attributes file) can make conversion apply anywhere in the tree,
    /// regardless of `.gitattributes` files.
    #[must_use]
    pub fn attributes_configured(&self) -> bool {
        self.attributes_configured
    }

    /// The root tree of `commit` and its committer time.
    ///
    /// # Errors
    ///
    /// If the commit cannot be read.
    pub fn commit_tree(&self, commit: &CommitId) -> Result<CommitTree> {
        let oid = gix::ObjectId::from_hex(commit.as_str().as_bytes())
            .map_err(|e| git_err("parsing a commit id")(boxed(e)))?;
        let repo = self.local();
        let commit = repo
            .find_commit(oid)
            .map_err(|e| git_err("reading the base commit")(boxed(e)))?;
        let time = commit
            .time()
            .map_err(|e| git_err("reading the commit time")(boxed(e)))?
            .seconds;
        let tree = commit
            .tree_id()
            .map_err(|e| git_err("reading the base tree")(boxed(e)))?
            .detach();
        Ok(CommitTree {
            tree: ObjectHash(tree),
            time,
        })
    }

    /// The entries of tree `id`, in git's order. Entries whose names could
    /// escape the tree or touch `.git` (as a checkout would refuse) are left
    /// out with a warning.
    ///
    /// # Errors
    ///
    /// If the tree cannot be read.
    pub fn tree(&self, id: ObjectHash) -> Result<Vec<TreeEntry>> {
        let repo = self.local();
        let tree = repo
            .find_tree(id.0)
            .map_err(|e| git_err("reading a tree")(boxed(e)))?;
        let decoded = tree
            .decode()
            .map_err(|e| git_err("decoding a tree")(boxed(e)))?;
        let opts = gix::validate::path::component::Options::default();
        let mut out = Vec::with_capacity(decoded.entries.len());
        for entry in &decoded.entries {
            let name: &BStr = entry.filename;
            if let Err(e) = gix::validate::path::component(name, None, opts) {
                tracing::warn!(tree = %id, name = %name, error = %e, "skipping an unsafe tree entry");
                continue;
            }
            let Ok(os) = gix::path::from_bstr(name) else {
                tracing::warn!(tree = %id, name = %name, "skipping a tree entry with an unrepresentable name");
                continue;
            };
            let kind = match entry.mode.kind() {
                GixKind::Tree => EntryKind::Tree,
                GixKind::Blob => EntryKind::Blob,
                GixKind::BlobExecutable => EntryKind::BlobExecutable,
                GixKind::Link => EntryKind::Symlink,
                GixKind::Commit => EntryKind::Submodule,
            };
            out.push(TreeEntry {
                name: os.into_owned().into_os_string(),
                kind,
                id: ObjectHash(entry.oid.to_owned()),
            });
        }
        Ok(out)
    }

    /// The size of blob `id`, without reading its contents.
    ///
    /// # Errors
    ///
    /// If the object cannot be found.
    pub fn blob_size(&self, id: ObjectHash) -> Result<u64> {
        let header = self
            .local()
            .find_header(id.0)
            .map_err(|e| git_err("reading an object header")(boxed(e)))?;
        Ok(header.size())
    }

    /// The contents of blob `id`, unconverted.
    ///
    /// # Errors
    ///
    /// If the blob cannot be read.
    pub fn blob(&self, id: ObjectHash) -> Result<Vec<u8>> {
        let repo = self.local();
        let mut blob = repo
            .find_blob(id.0)
            .map_err(|e| git_err("reading a blob")(boxed(e)))?;
        Ok(blob.take_data())
    }
}

type Reply = mpsc::Sender<Result<Vec<u8>>>;

/// Converts blobs to their working-tree form, with attributes from one
/// tree. Runs on its own thread because gix's filter pipeline borrows its
/// repository; the pipeline is built on the first request.
#[derive(Debug)]
pub struct Filter {
    requests: mpsc::Sender<(BString, Vec<u8>, Reply)>,
}

impl Filter {
    /// Starts a filter for paths in `tree`.
    ///
    /// # Errors
    ///
    /// If the worker thread cannot be started.
    pub fn spawn(store: &ObjectStore, tree: ObjectHash) -> Result<Self> {
        let (requests, rx) = mpsc::channel::<(BString, Vec<u8>, Reply)>();
        let repo = store.repo.clone();
        std::thread::Builder::new()
            .name("graft-filter".to_owned())
            .spawn(move || filter_worker(&repo, tree, &rx))
            .map_err(|source| RootError::Io {
                path: store.repo.git_dir().to_owned(),
                source,
            })?;
        Ok(Self { requests })
    }

    /// `data`, the blob at repo-relative `path`, in working-tree form.
    ///
    /// # Errors
    ///
    /// If attributes cannot be loaded or a filter fails.
    pub fn to_worktree(&self, path: &[u8], data: Vec<u8>) -> Result<Vec<u8>> {
        let (tx, rx) = mpsc::channel();
        let gone = || RootError::Git {
            action: "applying git filters",
            source: "the filter thread stopped".into(),
        };
        self.requests
            .send((path.into(), data, tx))
            .map_err(|_| gone())?;
        rx.recv().map_err(|_| gone())?
    }
}

fn filter_worker(
    repo: &gix::ThreadSafeRepository,
    tree: ObjectHash,
    rx: &mpsc::Receiver<(BString, Vec<u8>, Reply)>,
) {
    let repo = repo.to_thread_local();
    let built = (|| -> Result<gix::filter::Pipeline<'_>> {
        let index = repo
            .index_from_tree(&tree.0)
            .map_err(|e| git_err("indexing the base tree for attributes")(boxed(e)))?;
        let attributes = repo
            .attributes_only(
                &index,
                gix::worktree::stack::state::attributes::Source::IdMapping,
            )
            .map_err(|e| git_err("loading attributes")(boxed(e)))?;
        gix::filter::Pipeline::new(&repo, attributes.detach())
            .map_err(|e| git_err("loading git filters")(boxed(e)))
    })();
    let mut pipeline = match built {
        Ok(p) => p,
        Err(e) => {
            let message = e.to_string();
            for (_, _, reply) in rx {
                let _ = reply.send(Err(RootError::Git {
                    action: "loading git filters",
                    source: message.clone().into(),
                }));
            }
            return;
        }
    };
    for (path, data, reply) in rx {
        let result = (|| {
            let mut converted = pipeline
                .convert_to_worktree(&data, path.as_bstr(), ToWorktreeOptions::default())
                .map_err(|e| git_err("applying git filters")(boxed(e)))?;
            let mut out = Vec::with_capacity(data.len());
            std::io::Read::read_to_end(&mut converted, &mut out).map_err(|source| {
                RootError::Io {
                    path: graft_core::path_from_bytes(path.to_vec()),
                    source,
                }
            })?;
            Ok(out)
        })();
        let _ = reply.send(result);
    }
}
