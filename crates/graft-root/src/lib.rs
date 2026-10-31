//! Root access for Graft.
//!
//! A Root is the main repository: its git object store plus the layer cache
//! of shared, content-addressed directory trees (for example `node_modules`).
//! This crate opens repositories with `gix`, derives each Root's [`RootId`],
//! and resolves refs and commits. [`graft`] builds and inspects the small
//! git repositories that back reflink grafts, sharing Root's object store.
//! Objects are read lazily; nothing here shells out to `git`.
//!
//! The layer cache arrives in a later phase.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use graft_core::{CommitId, RootId};

pub mod graft;

/// Errors from opening or reading a repository.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RootError {
    /// No git repository contains the path.
    #[error("not a git repository: {} (run graft inside a git working tree)", path.display())]
    NotARepository {
        /// The path that was searched from.
        path: PathBuf,
    },

    /// The repository has no working tree, so it cannot be a Root.
    #[error("{} is a bare repository; a Root needs a working tree", path.display())]
    Bare {
        /// The repository's git directory.
        path: PathBuf,
    },

    /// HEAD does not point at a commit yet.
    #[error("{} has no commits yet; make a first commit before registering it", path.display())]
    NoCommits {
        /// The working tree.
        path: PathBuf,
    },

    /// A revision did not resolve to a commit.
    #[error("cannot resolve {rev:?} to a commit: {reason}")]
    BadRevision {
        /// The revision as given.
        rev: String,
        /// Why it failed.
        reason: String,
    },

    /// A path could not be canonicalised.
    #[error("cannot resolve {}: {source}", path.display())]
    Io {
        /// The path involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },

    /// A path in a tree could escape the working tree or touch `.git`.
    #[error("refusing unsafe path {path:?} in the tree: {reason}")]
    UnsafePath {
        /// The path as stored in the tree.
        path: String,
        /// Why it is unsafe.
        reason: String,
    },

    /// The repository uses a configuration Graft does not support yet.
    #[error("unsupported repository: {reason}")]
    Unsupported {
        /// What is unsupported.
        reason: String,
    },

    /// Reading from the object store or refs failed.
    #[error("{action}: {source}")]
    Git {
        /// What was being attempted.
        action: &'static str,
        /// The underlying `gix` error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// Result alias using [`RootError`].
pub type Result<T, E = RootError> = std::result::Result<T, E>;

pub(crate) fn git_err(
    action: &'static str,
) -> impl FnOnce(Box<dyn std::error::Error + Send + Sync>) -> RootError {
    move |source| RootError::Git { action, source }
}

/// Domain separator for [`derive_root_id`], so a Root ID can never collide
/// with another blake3 use. Bumping it changes every Root ID.
const ROOT_ID_CONTEXT: &str = "graft root-id v1";

/// Derives the [`RootId`] for a repository from the canonical path of its
/// working tree and its first commit (see [`Repo::first_commit`]).
///
/// The ID is the first 16 bytes of `blake3::derive_key` over
/// `<path bytes> NUL <commit hex>`. On Unix the path bytes are the raw
/// `OsStr` bytes, so non-UTF-8 paths are handled exactly.
#[must_use]
pub fn derive_root_id(canonical_path: &Path, first_commit: &CommitId) -> RootId {
    let mut hasher = blake3::Hasher::new_derive_key(ROOT_ID_CONTEXT);
    hasher.update(&graft_core::path_to_bytes(canonical_path));
    hasher.update(&[0]);
    hasher.update(first_commit.as_str().as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; RootId::LEN];
    bytes.copy_from_slice(&digest.as_bytes()[..RootId::LEN]);
    RootId::from_bytes(bytes)
}

/// An open git repository with a working tree.
pub struct Repo {
    repo: gix::Repository,
    workdir: PathBuf,
}

impl std::fmt::Debug for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repo")
            .field("workdir", &self.workdir)
            .finish_non_exhaustive()
    }
}

pub(crate) fn to_commit_id(id: gix::ObjectId) -> Result<CommitId> {
    id.to_string()
        .parse()
        .map_err(|e: graft_core::ParseIdError| RootError::Git {
            action: "reading an object id",
            source: Box::new(e),
        })
}

impl Repo {
    /// Finds the repository containing `path`, searching upwards like `git`.
    ///
    /// # Errors
    ///
    /// [`RootError::NotARepository`] if no repository contains `path`,
    /// [`RootError::Bare`] if it has no working tree, [`RootError::Io`] if
    /// `path` cannot be canonicalised.
    pub fn discover(path: &Path) -> Result<Self> {
        let start = path.canonicalize().map_err(|source| RootError::Io {
            path: path.to_owned(),
            source,
        })?;
        // Distinguish "no repository here" from real failures before opening.
        if let Err(e) = gix::discover::upwards(&start) {
            use gix::discover::upwards::Error as E;
            let not_found = matches!(
                e.downcast_any_ref::<E>(),
                Some(
                    E::NoGitRepository { .. }
                        | E::NoGitRepositoryWithinCeiling { .. }
                        | E::NoGitRepositoryWithinFs { .. }
                )
            );
            return Err(if not_found {
                RootError::NotARepository { path: start }
            } else {
                git_err("searching for a repository")(Box::new(e))
            });
        }
        let repo =
            gix::discover(&start).map_err(|e| git_err("opening the repository")(Box::new(e)))?;
        let Some(workdir) = repo.workdir() else {
            return Err(RootError::Bare {
                path: repo.git_dir().to_owned(),
            });
        };
        let workdir = workdir.canonicalize().map_err(|source| RootError::Io {
            path: workdir.to_owned(),
            source,
        })?;
        Ok(Self { repo, workdir })
    }

    /// The canonical path of the working tree's top level.
    #[must_use]
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// The repository's git directory (`.git`, or the worktree's git dir).
    #[must_use]
    pub fn git_dir(&self) -> &Path {
        self.repo.git_dir()
    }

    /// The commit HEAD points at.
    ///
    /// # Errors
    ///
    /// [`RootError::NoCommits`] if HEAD is unborn.
    pub fn head_commit(&self) -> Result<CommitId> {
        let mut head = self
            .repo
            .head()
            .map_err(|e| git_err("reading HEAD")(Box::new(e)))?;
        if head.is_unborn() {
            return Err(RootError::NoCommits {
                path: self.workdir.clone(),
            });
        }
        let commit = head
            .peel_to_commit()
            .map_err(|e| git_err("peeling HEAD to a commit")(Box::new(e)))?;
        to_commit_id(commit.id)
    }

    /// The full name of the branch HEAD points at (e.g. `refs/heads/main`),
    /// or `None` when HEAD is detached.
    ///
    /// # Errors
    ///
    /// If HEAD cannot be read.
    pub fn head_ref(&self) -> Result<Option<String>> {
        let name = self
            .repo
            .head_name()
            .map_err(|e| git_err("reading HEAD")(Box::new(e)))?;
        Ok(name.map(|n| n.as_bstr().to_string()))
    }

    /// Resolves a revision (`main`, `HEAD~2`, a commit hash, a tag) to the
    /// commit it names, peeling annotated tags.
    ///
    /// # Errors
    ///
    /// [`RootError::BadRevision`] if it does not name a commit.
    pub fn resolve_commit(&self, rev: &str) -> Result<CommitId> {
        let bad = |reason: String| RootError::BadRevision {
            rev: rev.to_owned(),
            reason,
        };
        let id = self
            .repo
            .rev_parse_single(rev)
            .map_err(|e| bad(e.to_string()))?;
        let commit = id
            .object()
            .map_err(|e| bad(e.to_string()))?
            .peel_to_commit()
            .map_err(|e| bad(e.to_string()))?;
        to_commit_id(commit.id)
    }

    /// Whether the object store contains the commit `id`.
    #[must_use]
    pub fn has_commit(&self, id: &CommitId) -> bool {
        let Ok(oid) = gix::ObjectId::from_hex(id.as_str().as_bytes()) else {
            return false;
        };
        self.repo.find_commit(oid).is_ok()
    }

    /// The first commit of the history HEAD is on: follow first parents
    /// from HEAD until a commit has none. In a shallow clone, the walk stops
    /// at the shallow boundary.
    ///
    /// This walks the whole first-parent chain, so it is only done when a
    /// Root is registered; afterwards the result is read from the store.
    ///
    /// # Errors
    ///
    /// [`RootError::NoCommits`] if HEAD is unborn, or if a commit cannot be
    /// read.
    pub fn first_commit(&self) -> Result<CommitId> {
        let head = self.head_commit()?;
        let mut id = gix::ObjectId::from_hex(head.as_str().as_bytes())
            .map_err(|e| git_err("parsing HEAD")(Box::new(e)))?;
        loop {
            let commit = self
                .repo
                .find_commit(id)
                .map_err(|e| git_err("reading a commit")(Box::new(e)))?;
            match commit.parent_ids().next() {
                Some(parent) if self.repo.has_object(parent) => id = parent.detach(),
                _ => return to_commit_id(id),
            }
        }
    }

    /// The [`RootId`] this repository would be registered under now.
    ///
    /// # Errors
    ///
    /// As for [`Repo::first_commit`].
    pub fn derive_root_id(&self) -> Result<(RootId, CommitId)> {
        let first = self.first_commit()?;
        Ok((derive_root_id(&self.workdir, &first), first))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_id_depends_on_path_and_commit() {
        let a: CommitId = "a".repeat(40).parse().unwrap();
        let b: CommitId = "b".repeat(40).parse().unwrap();
        let p = Path::new("/src/app");
        let q = Path::new("/src/app2");
        assert_eq!(derive_root_id(p, &a), derive_root_id(p, &a));
        assert_ne!(derive_root_id(p, &a), derive_root_id(p, &b));
        assert_ne!(derive_root_id(p, &a), derive_root_id(q, &a));
    }

    #[test]
    fn root_id_is_stable_across_releases() {
        // Changing this breaks every registered Root; bump the on-disk format.
        let commit: CommitId = "0123456789abcdef0123456789abcdef01234567".parse().unwrap();
        let id = derive_root_id(Path::new("/home/dev/project"), &commit);
        assert_eq!(id.to_string(), EXPECTED_ROOT_ID);
    }

    const EXPECTED_ROOT_ID: &str = "3c3c5222e9d0085488443cc4221de3fa";
}
