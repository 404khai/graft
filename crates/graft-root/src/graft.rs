//! Git state for a graft's working tree (ADR 0003).
//!
//! A reflink graft is a small, standalone git repository whose object store
//! is Root's (via `objects/info/alternates`), checked out at a base commit on
//! its own branch `graft/<name>`. This module creates that repository,
//! materialises its files, and answers questions about it: what changed
//! since the base, and whether pruning it would lose work.

use std::collections::{HashMap, HashSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use gix::bstr::{BStr, BString, ByteSlice};
use gix::filter::plumbing::pipeline::convert::to_worktree::Options as ToWorktreeOptions;
use gix::index::entry::{Mode, Stage, Stat};
use graft_core::{CommitId, GraftId, GraftName};

use crate::{Repo, Result, RootError, git_err, to_commit_id};

/// Clones a file sharing data blocks where the filesystem allows. Supplied
/// by the filesystem backend so this crate stays free of platform code.
pub trait FileCloner: Sync {
    /// Creates `dst` (which does not exist) with the contents and
    /// permissions of `src`.
    ///
    /// # Errors
    ///
    /// Any I/O error; the caller falls back to writing from the object store.
    fn clone_file(&self, src: &Path, dst: &Path) -> std::io::Result<()>;
}

/// What [`create_worktree`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CheckoutStats {
    /// Files cloned from Root's working tree.
    pub cloned: u64,
    /// Files written from the object store.
    pub written: u64,
    /// Symbolic links created.
    pub symlinks: u64,
    /// Submodule directories created (left empty, as `git clone` does).
    pub submodules: u64,
    /// Paths that collided with an earlier path (case-insensitive
    /// filesystems) and were skipped, as git does with a warning.
    pub collisions: Vec<String>,
}

/// The name of the branch a graft's HEAD points at.
#[must_use]
pub fn branch_name(name: &GraftName) -> String {
    format!("graft/{name}")
}

/// The Root ref that keeps a graft's base commit reachable.
#[must_use]
pub fn pin_ref(id: GraftId) -> String {
    format!("refs/graft/{id}")
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> RootError + '_ {
    move |source| RootError::Io {
        path: path.to_owned(),
        source,
    }
}

fn boxed(
    e: impl std::error::Error + Send + Sync + 'static,
) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(e)
}

fn oid(id: &CommitId) -> Result<gix::ObjectId> {
    gix::ObjectId::from_hex(id.as_str().as_bytes())
        .map_err(|e| git_err("parsing a commit id")(boxed(e)))
}

/// Escapes a value for a double-quoted git config string.
fn config_quote(value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len() + 2);
    out.push(b'"');
    for &b in value {
        match b {
            b'"' | b'\\' => {
                out.push(b'\\');
                out.push(b);
            }
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\t' => out.extend_from_slice(b"\\t"),
            _ => out.push(b),
        }
    }
    out.push(b'"');
    out
}

impl Repo {
    /// Creates `refs/graft/<id>` in Root pointing at `commit`, so `git gc`
    /// never prunes a live graft's base. Overwrites an existing pin.
    ///
    /// # Errors
    ///
    /// If the ref cannot be written.
    pub fn pin(&self, id: GraftId, commit: &CommitId, name: &GraftName) -> Result<()> {
        self.repo
            .reference(
                pin_ref(id).as_str(),
                oid(commit)?,
                gix::refs::transaction::PreviousValue::Any,
                format!("graft: pin base of {name}"),
            )
            .map_err(|e| git_err("writing a graft pin ref")(boxed(e)))?;
        Ok(())
    }

    /// Deletes `refs/graft/<id>`. Succeeds if it does not exist.
    ///
    /// # Errors
    ///
    /// If the ref exists but cannot be deleted.
    pub fn unpin(&self, id: GraftId) -> Result<()> {
        use gix::refs::transaction::{Change, PreviousValue, RefEdit, RefLog};
        let name = pin_ref(id);
        let existing = self
            .repo
            .try_find_reference(name.as_str())
            .map_err(|e| git_err("reading a graft pin ref")(boxed(e)))?;
        let Some(existing) = existing else {
            return Ok(());
        };
        self.repo
            .edit_reference(RefEdit {
                change: Change::Delete {
                    expected: PreviousValue::Any,
                    log: RefLog::AndReference,
                },
                name: existing.name().to_owned(),
                deref: false,
            })
            .map_err(|e| git_err("deleting a graft pin ref")(boxed(e)))?;
        Ok(())
    }

    /// The commit a pin ref points at, if it exists.
    ///
    /// # Errors
    ///
    /// If refs cannot be read.
    pub fn pinned(&self, id: GraftId) -> Result<Option<CommitId>> {
        let found = self
            .repo
            .try_find_reference(pin_ref(id).as_str())
            .map_err(|e| git_err("reading a graft pin ref")(boxed(e)))?;
        match found {
            Some(mut r) => {
                let target = r
                    .peel_to_id()
                    .map_err(|e| git_err("peeling a graft pin ref")(boxed(e)))?;
                Ok(Some(to_commit_id(target.detach())?))
            }
            None => Ok(None),
        }
    }

    /// Paths of tracked files whose working-tree contents differ from
    /// Root's index (stat comparison, hashing racy entries).
    fn dirty_paths(&self) -> Result<HashSet<BString>> {
        let iter = self
            .repo
            .status(gix::progress::Discard)
            .map_err(|e| git_err("starting a status of Root")(boxed(e)))?
            .untracked_files(gix::status::UntrackedFiles::None)
            .index_worktree_submodules(None)
            .index_worktree_rewrites(None)
            .into_index_worktree_iter(Vec::new())
            .map_err(|e| git_err("running a status of Root")(boxed(e)))?;
        let mut dirty = HashSet::new();
        for item in iter {
            let item = item.map_err(|e| git_err("running a status of Root")(boxed(e)))?;
            dirty.insert(item.rela_path().to_owned());
        }
        Ok(dirty)
    }
}

/// One file of the base tree, and where its contents will come from.
struct Job {
    path: BString,
    mode: Mode,
    id: gix::ObjectId,
    /// Root's working-tree file and the stat Root's index has for it, when
    /// that file is known to hold exactly this blob.
    clone_from: Option<(PathBuf, Stat)>,
}

fn validate_path(path: &BStr) -> Result<()> {
    let bad = |reason: String| RootError::UnsafePath {
        path: path.to_string(),
        reason,
    };
    let opts = gix::validate::path::component::Options::default();
    for component in path.split(|&b| b == b'/') {
        let mode = None;
        gix::validate::path::component(component.as_bstr(), mode, opts)
            .map_err(|e| bad(e.to_string()))?;
    }
    Ok(())
}

fn rel_to_path(rela: &BStr) -> Result<PathBuf> {
    gix::path::from_bstr(rela)
        .map(std::borrow::Cow::into_owned)
        .map_err(|e| git_err("converting a tree path")(boxed(e)))
}

fn write_config(root: &Repo, git_dir: &Path) -> Result<()> {
    let mut config: Vec<u8> = Vec::new();
    config.extend_from_slice(
        b"# Written by graft. Includes Root's config, then overrides.\n[include]\n\tpath = ",
    );
    config.extend_from_slice(&config_quote(&graft_core::path_to_bytes(
        &root.repo.common_dir().join("config"),
    )));
    config.extend_from_slice(
        b"\n[core]\n\tbare = false\n\tlogallrefupdates = true\n[remote \"root\"]\n\turl = ",
    );
    config.extend_from_slice(&config_quote(&graft_core::path_to_bytes(&root.workdir)));
    config.extend_from_slice(b"\n\tfetch = +refs/heads/*:refs/remotes/root/*\n");
    let path = git_dir.join("config");
    std::fs::write(&path, config).map_err(io_err(&path))
}

/// Creates the graft's `.git` directory in `dest` (which must exist and be
/// empty), sharing Root's object store.
fn init_git_dir(root: &Repo, dest: &Path, branch: &str, base: &CommitId) -> Result<()> {
    if root
        .repo
        .config_snapshot()
        .string("core.worktree")
        .is_some()
    {
        return Err(RootError::Unsupported {
            reason: "Roots that set core.worktree are not supported".to_owned(),
        });
    }
    let git_dir = dest.join(".git");
    for dir in [
        git_dir.join("objects/info"),
        git_dir.join("objects/pack"),
        git_dir
            .join("refs/heads")
            .join(branch)
            .parent()
            .map(Path::to_owned)
            .unwrap_or_default(),
        git_dir.join("refs/tags"),
        git_dir.join("info"),
    ] {
        std::fs::create_dir_all(&dir).map_err(io_err(&dir))?;
    }
    let write = |rel: &str, data: &[u8]| {
        let path = git_dir.join(rel);
        std::fs::write(&path, data).map_err(io_err(&path))
    };
    write("HEAD", format!("ref: refs/heads/{branch}\n").as_bytes())?;
    write(
        &format!("refs/heads/{branch}"),
        format!("{base}\n").as_bytes(),
    )?;
    write(
        "description",
        b"A graft. See https://github.com/404khai/graft\n",
    )?;

    let objects = root.repo.common_dir().join("objects");
    let objects = objects.canonicalize().map_err(io_err(&objects))?;
    let mut alternates = graft_core::path_to_bytes(&objects);
    alternates.push(b'\n');
    write("objects/info/alternates", &alternates)?;

    for (from, to) in [("shallow", "shallow"), ("info/exclude", "info/exclude")] {
        let source = root.repo.common_dir().join(from);
        if source.is_file() {
            let target = git_dir.join(to);
            std::fs::copy(&source, &target).map_err(io_err(&target))?;
        }
    }
    write_config(root, &git_dir)
}

/// Builds the graft's working tree at `dest` checked out at `base`, on
/// branch `branch`, sharing Root's object store (ADR 0003).
///
/// `dest` must not exist. Files whose Root working-tree copy is known to
/// hold the base blob are created with `cloner`; every other file is written
/// from the object store through the git filter pipeline. On error, `dest`
/// may be partially populated and the caller should remove it.
///
/// # Errors
///
/// [`RootError::UnsafePath`] if the tree contains a path that could escape
/// the working tree, [`RootError::Unsupported`] for unsupported Root
/// configurations, plus I/O and git errors.
pub fn create_worktree(
    root: &Repo,
    dest: &Path,
    branch: &str,
    base: &CommitId,
    cloner: &dyn FileCloner,
) -> Result<CheckoutStats> {
    let started = std::time::Instant::now();
    let lap = |stage: &str| {
        tracing::debug!(
            stage,
            elapsed_ms = started.elapsed().as_millis(),
            "create_worktree"
        );
    };
    std::fs::create_dir(dest).map_err(io_err(dest))?;
    init_git_dir(root, dest, branch, base)?;
    let graft = gix::open(dest).map_err(|e| git_err("opening the new graft")(boxed(e)))?;

    let tree = graft
        .find_commit(oid(base)?)
        .map_err(|e| git_err("reading the base commit")(boxed(e)))?
        .tree_id()
        .map_err(|e| git_err("reading the base tree")(boxed(e)))?
        .detach();
    let mut index = graft
        .index_from_tree(&tree)
        .map_err(|e| git_err("building the graft index")(boxed(e)))?;
    // The filter pipeline reads .gitattributes from the index while the
    // working tree is still empty, so persist the index first.
    index
        .write(gix::index::write::Options::default())
        .map_err(|e| git_err("writing the graft index")(boxed(e)))?;

    lap("initial index written");
    let jobs = plan(root, &index)?;
    lap("planned (Root status done)");
    let stats = materialize(&graft, dest, &jobs, cloner)?;
    lap("files materialized");

    // Record real stats so `git status` in the graft is clean without
    // rehashing every file.
    let skipped: HashSet<&str> = stats.collisions.iter().map(String::as_str).collect();
    for (entry, path) in index.entries_mut_with_paths() {
        if skipped.contains(path.to_str_lossy().as_ref()) {
            continue;
        }
        let file = dest.join(rel_to_path(path)?);
        let meta = gix::index::fs::Metadata::from_path_no_follow(&file).map_err(io_err(&file))?;
        entry.stat = Stat::from_fs(&meta).map_err(|e| git_err("reading file times")(boxed(e)))?;
    }
    lap("stats collected");
    index
        .write(gix::index::write::Options::default())
        .map_err(|e| git_err("writing the graft index")(boxed(e)))?;
    lap("final index written");
    Ok(stats)
}

fn plan(root: &Repo, index: &gix::index::File) -> Result<Vec<Job>> {
    let dirty = root.dirty_paths()?;
    let root_index = root
        .repo
        .index_or_empty()
        .map_err(|e| git_err("reading Root's index")(boxed(e)))?;
    let mut clean: HashMap<&BStr, (gix::ObjectId, Mode, Stat)> = HashMap::new();
    for entry in root_index.entries() {
        let path = entry.path(&root_index);
        if entry.stage() == Stage::Unconflicted && !dirty.contains(path) {
            clean.insert(path, (entry.id, entry.mode, entry.stat));
        }
    }

    let mut jobs = Vec::with_capacity(index.entries().len());
    for entry in index.entries() {
        let path = entry.path(index);
        validate_path(path)?;
        let clone_from = match clean.get(path) {
            Some(&(id, mode, stat))
                if id == entry.id
                    && mode == entry.mode
                    && matches!(mode, Mode::FILE | Mode::FILE_EXECUTABLE) =>
            {
                Some((root.workdir.join(rel_to_path(path)?), stat))
            }
            _ => None,
        };
        jobs.push(Job {
            path: path.to_owned(),
            mode: entry.mode,
            id: entry.id,
            clone_from,
        });
    }
    Ok(jobs)
}

fn create_dirs(dest: &Path, jobs: &[Job]) -> Result<()> {
    let mut dirs: Vec<&BStr> = Vec::new();
    let mut seen: HashSet<&BStr> = HashSet::new();
    for job in jobs {
        let path = job.path.as_bstr();
        let mut end = 0;
        while let Some(pos) = path[end..].find_byte(b'/') {
            end += pos;
            let dir = path[..end].as_bstr();
            if seen.insert(dir) {
                dirs.push(dir);
            }
            end += 1;
        }
    }
    dirs.sort();
    for dir in dirs {
        let full = dest.join(rel_to_path(dir)?);
        match std::fs::create_dir(&full) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let meta = std::fs::symlink_metadata(&full).map_err(io_err(&full))?;
                if !meta.is_dir() {
                    return Err(RootError::UnsafePath {
                        path: dir.to_string(),
                        reason: "is both a directory and a file in the tree".to_owned(),
                    });
                }
            }
            Err(e) => return Err(io_err(&full)(e)),
        }
    }
    Ok(())
}

/// Whether Root's working-tree file still has the stat Root's index has.
fn source_unchanged(src: &Path, expected: &Stat) -> bool {
    let Ok(meta) = gix::index::fs::Metadata::from_path_no_follow(src) else {
        return false;
    };
    let Ok(now) = Stat::from_fs(&meta) else {
        return false;
    };
    now.matches(expected, gix::index::entry::stat::Options::default())
}

fn open_new_file(dst: &Path, executable: bool) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // The umask still applies, as with git.
        opts.mode(if executable { 0o777 } else { 0o666 });
        // Never write through a symlink, even one created behind our back.
        opts.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(not(unix))]
    let _ = executable;
    opts.open(dst)
}

enum Outcome {
    Cloned,
    Written,
    Symlink,
    Submodule,
    Collision,
}

fn write_from_object(
    pipeline: &mut gix::filter::Pipeline<'_>,
    repo: &gix::Repository,
    job: &Job,
    dst: &Path,
) -> Result<Outcome> {
    let blob = repo
        .find_blob(job.id)
        .map_err(|e| git_err("reading a blob")(boxed(e)))?;
    let mut converted = pipeline
        .convert_to_worktree(&blob.data, job.path.as_bstr(), ToWorktreeOptions::default())
        .map_err(|e| git_err("applying git filters")(boxed(e)))?;
    let mut data = Vec::new();
    converted.read_to_end(&mut data).map_err(io_err(dst))?;
    let mut file = match open_new_file(dst, job.mode == Mode::FILE_EXECUTABLE) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(Outcome::Collision),
        Err(e) => return Err(io_err(dst)(e)),
    };
    std::io::Write::write_all(&mut file, &data).map_err(io_err(dst))?;
    Ok(Outcome::Written)
}

fn run_job(
    pipeline: &mut gix::filter::Pipeline<'_>,
    repo: &gix::Repository,
    cloner: &dyn FileCloner,
    dest: &Path,
    job: &Job,
) -> Result<Outcome> {
    let target = dest.join(rel_to_path(job.path.as_bstr())?);
    if let Some((source, stat)) = &job.clone_from
        && source_unchanged(source, stat)
    {
        match cloner.clone_file(source, &target) {
            Ok(()) if source_unchanged(source, stat) => return Ok(Outcome::Cloned),
            Ok(()) => {
                // Root's copy changed while we cloned it; fall back.
                std::fs::remove_file(&target).map_err(io_err(&target))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Ok(Outcome::Collision);
            }
            Err(e) => {
                tracing::debug!(path = %target.display(), error = %e, "clone failed; writing from objects");
                let _ = std::fs::remove_file(&target);
            }
        }
    }
    write_from_object(pipeline, repo, job, &target)
}

/// Creates symlinks and submodule directories. Runs after every regular
/// file exists, so no file write can traverse a symlink from the tree.
fn run_late_job(repo: &gix::Repository, dest: &Path, job: &Job) -> Result<Outcome> {
    let target = dest.join(rel_to_path(job.path.as_bstr())?);
    let exists = |e: &std::io::Error| e.kind() == std::io::ErrorKind::AlreadyExists;
    if job.mode == Mode::COMMIT {
        return match std::fs::create_dir(&target) {
            Ok(()) => Ok(Outcome::Submodule),
            Err(e) if exists(&e) => Ok(Outcome::Collision),
            Err(e) => Err(io_err(&target)(e)),
        };
    }
    let blob = repo
        .find_blob(job.id)
        .map_err(|e| git_err("reading a symlink target")(boxed(e)))?;
    let link_target = rel_to_path(blob.data.as_bstr())?;
    #[cfg(unix)]
    let created = std::os::unix::fs::symlink(&link_target, &target);
    #[cfg(not(unix))]
    let created: std::io::Result<()> = {
        let _ = &link_target;
        Err(std::io::ErrorKind::Unsupported.into())
    };
    match created {
        Ok(()) => Ok(Outcome::Symlink),
        Err(e) if exists(&e) => Ok(Outcome::Collision),
        Err(e) => Err(io_err(&target)(e)),
    }
}

fn materialize(
    graft: &gix::Repository,
    dest: &Path,
    jobs: &[Job],
    cloner: &dyn FileCloner,
) -> Result<CheckoutStats> {
    create_dirs(dest, jobs)?;
    let (files, late): (Vec<&Job>, Vec<&Job>) = jobs
        .iter()
        .partition(|j| matches!(j.mode, Mode::FILE | Mode::FILE_EXECUTABLE));

    let stats = Mutex::new(CheckoutStats::default());
    let record = |job: &Job, outcome: Outcome| {
        let mut s = stats
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match outcome {
            Outcome::Cloned => s.cloned += 1,
            Outcome::Written => s.written += 1,
            Outcome::Symlink => s.symlinks += 1,
            Outcome::Submodule => s.submodules += 1,
            Outcome::Collision => s.collisions.push(job.path.to_string()),
        }
    };

    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let first_error: Mutex<Option<RootError>> = Mutex::new(None);
    let workers = std::thread::available_parallelism()
        .map_or(4, std::num::NonZero::get)
        .min(8)
        .min(files.len().div_ceil(64).max(1));

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let repo = graft.clone();
            let (next, failed, first_error, files, record) =
                (&next, &failed, &first_error, &files, &record);
            scope.spawn(move || {
                let fail = |e: RootError| {
                    failed.store(true, Ordering::Relaxed);
                    let mut slot = first_error
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    slot.get_or_insert(e);
                };
                let mut pipeline = match repo.filter_pipeline(None) {
                    Ok((p, _)) => p,
                    Err(e) => return fail(git_err("loading git filters")(boxed(e))),
                };
                loop {
                    if failed.load(Ordering::Relaxed) {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = files.get(i) else { return };
                    match run_job(&mut pipeline, &repo, cloner, dest, job) {
                        Ok(outcome) => record(job, outcome),
                        Err(e) => return fail(e),
                    }
                }
            });
        }
    });
    if let Some(e) = first_error
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    {
        return Err(e);
    }
    for job in late {
        let outcome = run_late_job(graft, dest, job)?;
        record(job, outcome);
    }
    Ok(stats
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner))
}

/// Adds `paths` to the graft's `.git/info/exclude`, so layers (shared
/// dependency trees) never show up as changes or block prune.
///
/// # Errors
///
/// If the exclude file cannot be written.
pub fn exclude_paths(workdir: &Path, paths: &[&Path]) -> Result<()> {
    use std::io::Write as _;
    if paths.is_empty() {
        return Ok(());
    }
    let file = workdir.join(".git/info/exclude");
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(io_err(dir))?;
    }
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .map_err(io_err(&file))?;
    let mut text = Vec::from(&b"\n# Graft layers (shared, see graft.toml)\n"[..]);
    for path in paths {
        // Anchored to the top level, and escaped so names are literal.
        text.push(b'/');
        for &b in &graft_core::path_to_bytes(path) {
            if matches!(b, b'*' | b'?' | b'[' | b'\\' | b'!' | b'#' | b' ') {
                text.push(b'\\');
            }
            text.push(b);
        }
        text.extend_from_slice(b"/\n");
    }
    out.write_all(&text).map_err(io_err(&file))
}

/// How a path differs from the graft's base commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeKind {
    /// Not in the base; present now.
    Added,
    /// In the base and present now, with different contents or mode.
    Modified,
    /// In the base; gone now.
    Deleted,
}

impl ChangeKind {
    /// One-letter code, as in `git status --short`.
    #[must_use]
    pub const fn code(self) -> char {
        match self {
            Self::Added => 'A',
            Self::Modified => 'M',
            Self::Deleted => 'D',
        }
    }
}

/// One changed path in a graft.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Change {
    /// Path relative to the graft's top level.
    pub path: PathBuf,
    /// How it changed.
    pub kind: ChangeKind,
}

/// A change with the contents needed to render it as a diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    /// The change.
    pub change: Change,
    /// The base version, in working-tree form (filters applied); `None` if
    /// added.
    pub old: Option<Vec<u8>>,
    /// The current version; `None` if deleted. Symlinks are their target.
    pub new: Option<Vec<u8>>,
}

/// A commit in a graft that Root's object store does not have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unintegrated {
    /// The ref pointing at it (`HEAD` or `refs/heads/...`).
    pub reference: String,
    /// The commit.
    pub commit: CommitId,
}

/// An existing graft's git repository.
pub struct GraftRepo {
    repo: gix::Repository,
    workdir: PathBuf,
}

impl std::fmt::Debug for GraftRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraftRepo")
            .field("workdir", &self.workdir)
            .finish_non_exhaustive()
    }
}

impl GraftRepo {
    /// Opens the graft whose working tree is `workdir` (no upward search).
    ///
    /// # Errors
    ///
    /// If `workdir` is not a graft's working tree.
    pub fn open(workdir: &Path) -> Result<Self> {
        let repo = gix::open(workdir).map_err(|e| git_err("opening the graft")(boxed(e)))?;
        Ok(Self {
            repo,
            workdir: workdir.to_owned(),
        })
    }

    /// The graft's working tree.
    #[must_use]
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    fn tree_of(&self, commit: &CommitId) -> Result<gix::ObjectId> {
        Ok(self
            .repo
            .find_commit(oid(commit)?)
            .map_err(|e| git_err("reading the base commit")(boxed(e)))?
            .tree_id()
            .map_err(|e| git_err("reading the base tree")(boxed(e)))?
            .detach())
    }

    /// Paths reported by a status against `tree` (or HEAD), including
    /// untracked, non-ignored files.
    fn status_paths(&self, tree: Option<gix::ObjectId>) -> Result<HashSet<BString>> {
        let mut platform = self
            .repo
            .status(gix::progress::Discard)
            .map_err(|e| git_err("starting a status")(boxed(e)))?
            .untracked_files(gix::status::UntrackedFiles::Files)
            .index_worktree_submodules(None)
            .index_worktree_rewrites(None)
            .tree_index_track_renames(gix::status::tree_index::TrackRenames::Disabled);
        if let Some(tree) = tree {
            platform = platform.head_tree(tree);
        }
        let iter = platform
            .into_iter(Vec::new())
            .map_err(|e| git_err("running a status")(boxed(e)))?;
        let mut paths = HashSet::new();
        for item in iter {
            let item = item.map_err(|e| git_err("running a status")(boxed(e)))?;
            paths.insert(item.location().to_owned());
        }
        Ok(paths)
    }

    /// Every path that differs between `base` and the working tree,
    /// whether committed in the graft or not. Ignored files are excluded.
    ///
    /// # Errors
    ///
    /// Git and I/O errors.
    pub fn changes_since(&self, base: &CommitId) -> Result<Vec<Change>> {
        let tree_id = self.tree_of(base)?;
        let tree = self
            .repo
            .find_tree(tree_id)
            .map_err(|e| git_err("reading the base tree")(boxed(e)))?;
        let mut changes = Vec::new();
        for rela in self.status_paths(Some(tree_id))? {
            let path = rel_to_path(rela.as_bstr())?;
            let in_base = tree
                .lookup_entry_by_path(&path)
                .map_err(|e| git_err("reading the base tree")(boxed(e)))?
                .is_some_and(|e| !e.mode().is_tree());
            let in_worktree = std::fs::symlink_metadata(self.workdir.join(&path)).is_ok();
            let kind = match (in_base, in_worktree) {
                (false, true) => ChangeKind::Added,
                (true, true) => ChangeKind::Modified,
                (true, false) => ChangeKind::Deleted,
                (false, false) => continue,
            };
            changes.push(Change { path, kind });
        }
        changes.sort();
        Ok(changes)
    }

    /// Paths with changes not committed in the graft: staged, unstaged or
    /// untracked (ignored files excluded).
    ///
    /// # Errors
    ///
    /// Git and I/O errors.
    pub fn uncommitted_paths(&self) -> Result<Vec<PathBuf>> {
        let mut paths = self
            .status_paths(None)?
            .iter()
            .map(|p| rel_to_path(p.as_bstr()))
            .collect::<Result<Vec<_>>>()?;
        paths.sort();
        Ok(paths)
    }

    /// Commits reachable from the graft's HEAD or local branches that Root's
    /// object store does not contain, so pruning would lose them.
    ///
    /// # Errors
    ///
    /// Git errors.
    pub fn unintegrated(&self, root: &Repo) -> Result<Vec<Unintegrated>> {
        let mut tips: Vec<(String, gix::ObjectId)> = Vec::new();
        if let Ok(head) = self.repo.head_id() {
            tips.push(("HEAD".to_owned(), head.detach()));
        }
        let refs = self
            .repo
            .references()
            .map_err(|e| git_err("listing graft branches")(boxed(e)))?;
        let branches = refs
            .local_branches()
            .map_err(|e| git_err("listing graft branches")(boxed(e)))?;
        for branch in branches {
            let mut branch =
                branch.map_err(|e| git_err("reading a graft branch")(Box::from(e.to_string())))?;
            let name = branch.name().as_bstr().to_string();
            if let Ok(id) = branch.peel_to_id() {
                tips.push((name, id.detach()));
            }
        }
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for (reference, id) in tips {
            let commit = to_commit_id(id)?;
            if seen.insert(commit.clone()) && !root.has_commit(&commit) {
                out.push(Unintegrated { reference, commit });
            }
        }
        Ok(out)
    }

    /// [`GraftRepo::changes_since`] with old and new contents, for diffs.
    ///
    /// # Errors
    ///
    /// Git and I/O errors.
    pub fn diff_since(&self, base: &CommitId) -> Result<Vec<FileDiff>> {
        let changes = self.changes_since(base)?;
        let tree = self
            .repo
            .find_tree(self.tree_of(base)?)
            .map_err(|e| git_err("reading the base tree")(boxed(e)))?;
        let (mut pipeline, _) = self
            .repo
            .filter_pipeline(None)
            .map_err(|e| git_err("loading git filters")(boxed(e)))?;
        let mut out = Vec::with_capacity(changes.len());
        for change in changes {
            let old = if change.kind == ChangeKind::Added {
                None
            } else {
                let entry = tree
                    .lookup_entry_by_path(&change.path)
                    .map_err(|e| git_err("reading the base tree")(boxed(e)))?;
                match entry {
                    Some(entry) if !entry.mode().is_commit() => {
                        let blob = self
                            .repo
                            .find_blob(entry.object_id())
                            .map_err(|e| git_err("reading a blob")(boxed(e)))?;
                        let rela = gix::path::into_bstr(&change.path)
                            .map_err(|e| git_err("converting a path")(boxed(e)))?;
                        let mut converted = pipeline
                            .convert_to_worktree(
                                &blob.data,
                                rela.as_ref(),
                                ToWorktreeOptions::default(),
                            )
                            .map_err(|e| git_err("applying git filters")(boxed(e)))?;
                        let mut data = Vec::new();
                        converted
                            .read_to_end(&mut data)
                            .map_err(io_err(&change.path))?;
                        Some(data)
                    }
                    _ => Some(Vec::new()),
                }
            };
            let new = match change.kind {
                ChangeKind::Deleted => None,
                _ => Some(read_worktree(&self.workdir.join(&change.path))?),
            };
            out.push(FileDiff { change, old, new });
        }
        Ok(out)
    }
}

fn read_worktree(path: &Path) -> Result<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path).map_err(io_err(path))?;
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(path).map_err(io_err(path))?;
        return Ok(graft_core::path_to_bytes(&target));
    }
    if meta.is_dir() {
        return Ok(Vec::new());
    }
    std::fs::read(path).map_err(io_err(path))
}

/// Renders a [`FileDiff`] in git's unified diff format.
#[must_use]
pub fn unified_diff(diff: &FileDiff) -> String {
    use gix::diff::blob::unified_diff::{
        ConsumeBinaryHunk, ConsumeBinaryHunkDelegate, ContextSize, HunkHeader,
    };
    use gix::diff::blob::{Algorithm, Diff, InternedInput, UnifiedDiff};
    use std::fmt::Write as _;

    struct Collect(String);
    impl ConsumeBinaryHunkDelegate for Collect {
        fn consume_binary_hunk(
            &mut self,
            _: HunkHeader,
            header: &str,
            hunk: &[u8],
        ) -> std::io::Result<()> {
            self.0.push_str(header);
            self.0.push_str(&String::from_utf8_lossy(hunk));
            Ok(())
        }
    }

    let path = diff.change.path.display();
    let mut out = format!("diff --git a/{path} b/{path}\n");
    match diff.change.kind {
        ChangeKind::Added => out.push_str("new file\n"),
        ChangeKind::Deleted => out.push_str("deleted file\n"),
        ChangeKind::Modified => {}
    }
    let old = diff.old.as_deref().unwrap_or_default();
    let new = diff.new.as_deref().unwrap_or_default();
    let binary = |d: &[u8]| d[..d.len().min(8000)].contains(&0);
    let (a, b) = (
        if diff.old.is_some() {
            format!("a/{path}")
        } else {
            "/dev/null".to_owned()
        },
        if diff.new.is_some() {
            format!("b/{path}")
        } else {
            "/dev/null".to_owned()
        },
    );
    if binary(old) || binary(new) {
        let _ = writeln!(out, "Binary files {a} and {b} differ");
        return out;
    }
    let _ = write!(out, "--- {a}\n+++ {b}\n");
    let input = InternedInput::new(old, new);
    let mut d = Diff::compute(Algorithm::Histogram, &input);
    d.postprocess_lines(&input);
    let sink = ConsumeBinaryHunk::new(Collect(String::new()), "\n");
    match UnifiedDiff::new(&d, &input, sink, ContextSize::symmetrical(3)).consume() {
        Ok(Collect(hunks)) => out.push_str(&hunks),
        Err(e) => {
            let _ = writeln!(out, "(cannot render diff: {e})");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_path_rejects_escapes() {
        for bad in [
            "..",
            "a/../b",
            ".",
            "a/./b",
            ".git",
            "a/.git/config",
            ".GIT",
            "a/.Git",
            "git~1",
            ".git ",
            ".git.",
            "a//b",
            "",
        ] {
            assert!(
                validate_path(bad.into()).is_err(),
                "{bad:?} must be rejected"
            );
        }
        for good in [
            "a",
            "src/lib.rs",
            ".gitignore",
            ".github/workflows/ci.yml",
            "a.git",
        ] {
            assert!(
                validate_path(good.into()).is_ok(),
                "{good:?} must be accepted"
            );
        }
    }

    #[test]
    fn config_values_are_quoted() {
        assert_eq!(config_quote(br#"/a "b"\c"#), br#""/a \"b\"\\c""#.to_vec());
        assert_eq!(config_quote(b"x\ny"), b"\"x\\ny\"".to_vec());
    }

    #[test]
    fn names() {
        let n: GraftName = "auth".parse().unwrap();
        assert_eq!(branch_name(&n), "graft/auth");
        assert_eq!(
            pin_ref(GraftId::from_bytes([0xab; 16])),
            format!("refs/graft/{}", "ab".repeat(16))
        );
    }
}
