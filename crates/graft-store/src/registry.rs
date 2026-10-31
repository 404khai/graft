//! Finding and registering the Root for a repository.
//!
//! A Root's directory is named by its [`RootId`], which is derived from the
//! repository's canonical path and its first commit (ADR 0002). Deriving it
//! needs a walk of the first-parent history, so it is done once, at
//! registration. Every later lookup scans `$GRAFT_HOME/roots/` for a
//! database whose recorded path is this repository and whose first commit
//! the repository still contains.

use std::path::Path;

use graft_core::{GraftHome, GraftName, RootId, Timestamp};
use graft_root::Repo;

use crate::{Registration, Result, RootRecord, Store, StoreError};

/// Every registered Root under `home`, sorted by path then ID.
///
/// Directories that are not Root directories, or whose database is still
/// being created by another process, are skipped. A database that cannot be
/// read is logged and skipped, so one damaged Root never hides the others.
///
/// # Errors
///
/// Only if `$GRAFT_HOME/roots` exists but cannot be listed.
pub fn list_roots(home: &GraftHome) -> Result<Vec<(RootRecord, Store)>> {
    let dir = home.roots_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(StoreError::Io {
                action: "listing",
                path: dir,
                source,
            });
        }
    };
    let mut roots = Vec::new();
    for entry in entries.flatten() {
        let Some(id) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<RootId>().ok())
        else {
            continue;
        };
        let root_dir = entry.path();
        if !Store::db_path(&root_dir).is_file() {
            continue;
        }
        match Store::open(&root_dir).and_then(|store| Ok((store.root()?, store))) {
            Ok((record, store)) if record.id == id => roots.push((record, store)),
            Ok((record, _)) => {
                tracing::warn!(dir = %root_dir.display(), found = %record.id, "Root directory name does not match its database; skipping");
            }
            Err(StoreError::NotInitialised { .. }) => {}
            Err(e) => {
                tracing::warn!(dir = %root_dir.display(), error = %e, "cannot read Root database; skipping");
            }
        }
    }
    roots.sort_by(|(a, _), (b, _)| (&a.path, a.id).cmp(&(&b.path, b.id)));
    Ok(roots)
}

/// The registered Root for `repo`, if there is one.
///
/// If the path has been registered more than once (the repository was
/// replaced by an unrelated one at the same path, then registered again),
/// the Root whose first commit `repo` contains wins; ties go to the most
/// recently registered.
///
/// # Errors
///
/// As for [`list_roots`].
pub fn find_root(home: &GraftHome, repo: &Repo) -> Result<Option<(RootRecord, Store)>> {
    Ok(find_by_path(home, repo.workdir())?
        .into_iter()
        .filter(|(record, _)| repo.has_commit(&record.first_commit))
        .max_by_key(|(record, _)| record.registered_at))
}

fn find_by_path(home: &GraftHome, path: &Path) -> Result<Vec<(RootRecord, Store)>> {
    Ok(list_roots(home)?
        .into_iter()
        .filter(|(record, _)| record.path == path)
        .collect())
}

/// Registers `repo` as a Root, or returns the existing registration.
///
/// Idempotent and safe to run from many processes at once: exactly one
/// concurrent caller gets [`Registration::Created`].
///
/// # Errors
///
/// [`graft_root::RootError::NoCommits`] (as [`StoreError::Root`]) for an
/// empty repository, plus store errors.
pub fn register(home: &GraftHome, repo: &Repo) -> Result<(RootRecord, Store, Registration)> {
    if let Some((record, store)) = find_root(home, repo)? {
        return Ok((record, store, Registration::Existing));
    }
    let (id, first_commit) = repo.derive_root_id()?;
    let record = RootRecord {
        id,
        path: repo.workdir().to_owned(),
        first_commit,
        registered_at: Timestamp::now(),
    };
    let (store, registration) = Store::create(&home.root_dir(&id), &record)?;
    // A concurrent registration may have won with an earlier timestamp.
    let record = store.root()?;
    Ok((record, store, registration))
}

/// A Root found from a path, which may be inside one of its grafts.
#[derive(Debug)]
pub struct Located {
    /// The Root's record.
    pub record: RootRecord,
    /// Its open database.
    pub store: Store,
    /// Root's repository.
    pub repo: Repo,
    /// The graft `start` was inside, if any.
    pub inside_graft: Option<GraftName>,
}

/// Finds the Root for `start`: either the repository containing it, or, when
/// `start` is inside a graft under `$GRAFT_HOME/roots/<id>/mounts/<name>`,
/// the Root that graft belongs to. A graft's own repository is never
/// mistaken for a Root.
///
/// # Errors
///
/// [`StoreError::NotRegistered`] if the repository is not a Root, plus
/// repository and store errors.
pub fn locate(home: &GraftHome, start: &Path) -> Result<Located> {
    let start = start.canonicalize().map_err(|source| StoreError::Io {
        action: "resolving",
        path: start.to_owned(),
        source,
    })?;
    if let Ok(roots) = home.roots_dir().canonicalize()
        && let Ok(rest) = start.strip_prefix(&roots)
    {
        let mut parts = rest
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned());
        if let Some(id) = parts.next().and_then(|p| p.parse::<RootId>().ok()) {
            let store = Store::open(&home.root_dir(&id))?;
            let record = store.root()?;
            let repo = Repo::discover(&record.path)?;
            let inside_graft = match (parts.next().as_deref(), parts.next()) {
                (Some("mounts"), Some(name)) => name.parse().ok(),
                _ => None,
            };
            return Ok(Located {
                record,
                store,
                repo,
                inside_graft,
            });
        }
    }
    let repo = Repo::discover(&start)?;
    match find_root(home, &repo)? {
        Some((record, store)) => Ok(Located {
            record,
            store,
            repo,
            inside_graft: None,
        }),
        None => Err(StoreError::NotRegistered {
            path: repo.workdir().to_owned(),
        }),
    }
}
