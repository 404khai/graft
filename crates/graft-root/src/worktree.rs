//! Grafts registered as git linked worktrees (ADR 0027).
//!
//! git, libgit2, `JGit` and gix all find a repository's worktrees by reading
//! `$GIT_COMMON_DIR/worktrees/<id>/`: `HEAD`, `commondir`, `gitdir` and
//! `index`, with a `.git` file in the working tree pointing back there. A
//! graft that writes these files is listed by `git worktree list` and by
//! every editor and git client that shows worktrees.
//!
//! A registered graft has the id `graft-<name>` and checks out its own
//! branch, `graft-live/<name>`, in Root. Registration is opt-in (`[git]
//! worktrees` in `config.toml`): a graft created without it keeps the
//! private repository of ADR 0003, and a graft's layout never changes after
//! it is created.

use std::path::{Path, PathBuf};

use gix::bstr::ByteSlice;
use graft_core::{CommitId, GraftName};

use crate::graft::{Unintegrated, layer_exclude_lines};
use crate::{Repo, Result, RootError, git_err, to_commit_id};

/// Prefix of the branch a registered graft checks out in Root.
pub const LIVE_BRANCH_PREFIX: &str = "graft-live/";

/// File in a registration holding the excludes of a graft with layers.
const EXCLUDE_FILE: &str = "graft-exclude";

/// The branch a registered graft checks out: `graft-live/<name>`.
#[must_use]
pub fn live_branch(name: &GraftName) -> String {
    format!("{LIVE_BRANCH_PREFIX}{name}")
}

/// The full ref name of [`live_branch`].
#[must_use]
pub fn live_ref(name: &GraftName) -> String {
    format!("refs/heads/{}", live_branch(name))
}

/// The worktree id of a registered graft: `graft-<name>`.
#[must_use]
pub fn registration_id(name: &GraftName) -> String {
    format!("graft-{name}")
}

/// How the registered graft's files are served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// A real working tree (ADR 0003). Its private `.git` is adopted: the
    /// index moves into the registration and `.git` becomes a file.
    Reflink,
    /// A mount served by graftd (ADRs 0006, 0008). The mount serves `.git`
    /// as a file and the index is built on first use. Registered locked,
    /// because git cannot remove a mount point and must never prune it.
    Virtual,
}

/// What to do with a graft's `graft-live/<name>` branch when it is
/// unregistered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchPolicy {
    /// Delete it (the caller has checked that no work is lost, or was told
    /// to discard it).
    Delete,
    /// Delete it only if every commit on it is on another ref.
    KeepIfUnique,
}

/// What [`Repo::unregister`] did with the branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchOutcome {
    /// There was no branch.
    Absent,
    /// It was deleted.
    Deleted,
    /// It was kept: it has commits on no other ref, or another worktree has
    /// it checked out.
    Kept {
        /// Why.
        reason: String,
    },
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

/// The path git records for `workdir`'s `.git`: `workdir` with its parent
/// canonicalised (the working tree itself may not exist yet, for a virtual
/// graft that is not mounted).
fn dot_git(workdir: &Path) -> Result<PathBuf> {
    let (Some(parent), Some(name)) = (workdir.parent(), workdir.file_name()) else {
        return Err(RootError::Unsupported {
            reason: format!("{} cannot be a graft's working tree", workdir.display()),
        });
    };
    let parent = parent.canonicalize().map_err(io_err(parent))?;
    Ok(parent.join(name).join(".git"))
}

fn trim_newline(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}

/// Writes `data` to `path` through a temporary file and a rename, so a
/// reader never sees it half-written.
fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = path.with_extension("graft-tmp");
    std::fs::write(&tmp, data).map_err(io_err(&tmp))?;
    std::fs::rename(&tmp, path).map_err(io_err(path))
}

/// Moves `from` to `to`, copying when they are on different filesystems.
fn move_file(from: &Path, to: &Path) -> Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            std::fs::copy(from, to).map_err(io_err(to))?;
            std::fs::remove_file(from).map_err(io_err(from))
        }
        Err(e) => Err(io_err(to)(e)),
    }
}

/// Removes `dir` if it is empty, and then each empty parent up to (not
/// including) `stop`.
fn remove_empty_dirs(dir: &Path, stop: &Path) {
    let mut dir = dir.to_owned();
    while dir.starts_with(stop) && dir != stop {
        if std::fs::remove_dir(&dir).is_err() {
            return;
        }
        let Some(parent) = dir.parent() else { return };
        dir = parent.to_owned();
    }
}

/// Every commit reachable from `tips` and from no ref of `repo` other than
/// `own_ref`: the work only `own_ref` (and HEAD) hold. One entry per tip
/// that has any.
pub(crate) fn only_on(
    repo: &gix::Repository,
    tips: &[(String, gix::ObjectId)],
    own_ref: &str,
) -> Result<Vec<Unintegrated>> {
    let refs = repo
        .references()
        .map_err(|e| git_err("listing Root's refs")(boxed(e)))?;
    let all = refs
        .all()
        .map_err(|e| git_err("listing Root's refs")(boxed(e)))?;
    let mut hidden = Vec::new();
    for r in all {
        let Ok(mut r) = r else { continue };
        if r.name().as_bstr() == own_ref.as_bytes().as_bstr() {
            continue;
        }
        // Tags of trees or blobs peel to no commit; they hide nothing.
        if let Ok(id) = r.peel_to_commit() {
            hidden.push(id.id);
        }
    }
    let mut out = Vec::new();
    for (reference, tip) in tips {
        let hex = tip.to_string();
        if out.iter().any(|u: &Unintegrated| u.commit.as_str() == hex) {
            continue;
        }
        let mut walk = repo
            .rev_walk([*tip])
            .with_hidden(hidden.iter().copied())
            .all()
            .map_err(|e| git_err("walking a graft's commits")(boxed(e)))?;
        if let Some(first) = walk.next() {
            first.map_err(|e| git_err("walking a graft's commits")(boxed(e)))?;
            out.push(Unintegrated {
                reference: reference.clone(),
                commit: to_commit_id(*tip)?,
                in_root: true,
            });
        }
    }
    Ok(out)
}

impl Repo {
    /// Root's common git directory (`.git` of its main worktree).
    #[must_use]
    pub fn common_dir(&self) -> &Path {
        self.repo.common_dir()
    }

    /// Where graft `name`'s registration lives, whether or not it exists.
    #[must_use]
    pub fn registration_dir(&self, name: &GraftName) -> PathBuf {
        self.common_dir()
            .join("worktrees")
            .join(registration_id(name))
    }

    /// The registration of graft `name` with working tree `workdir`, if
    /// there is one and its `gitdir` names exactly that working tree.
    #[must_use]
    pub fn registration(&self, name: &GraftName, workdir: &Path) -> Option<PathBuf> {
        let dir = self.registration_dir(name);
        let gitdir = std::fs::read(dir.join("gitdir")).ok()?;
        let expected = dot_git(workdir).ok()?;
        (trim_newline(&gitdir) == graft_core::path_to_bytes(&expected).as_slice()).then_some(dir)
    }

    /// Registers graft `name`, whose working tree is (or, for a virtual
    /// graft, will be mounted at) `workdir`, as a linked worktree checked
    /// out at `base` on [`live_branch`]. `layers` are excluded from git.
    /// Returns the registration directory.
    ///
    /// For [`Layout::Reflink`], `workdir` must hold the private repository
    /// [`crate::graft::create_worktree`] built, with no commits.
    ///
    /// # Errors
    ///
    /// [`RootError::AlreadyExists`] if a worktree or branch of that name is
    /// not this graft's; [`RootError::Unsupported`] for Roots that set
    /// `core.worktree` or `extensions.worktreeConfig = false`; I/O and git
    /// errors. On error nothing this call created is left behind.
    pub fn register(
        &self,
        name: &GraftName,
        workdir: &Path,
        base: &CommitId,
        layout: Layout,
        layers: &[&Path],
    ) -> Result<PathBuf> {
        crate::graft::refuse_core_worktree(self)?;
        self.enable_worktree_config()?;
        let dot_git = dot_git(workdir)?;
        let admin = self.registration_dir(name);
        if std::fs::symlink_metadata(&admin).is_ok() {
            if self.registration(name, workdir).is_none() {
                let at = std::fs::read(admin.join("gitdir")).map_or_else(
                    |_| "an unknown path".to_owned(),
                    |b| String::from_utf8_lossy(trim_newline(&b)).into_owned(),
                );
                return Err(RootError::AlreadyExists {
                    reason: format!(
                        "Root already has a worktree with id {} (at {at}); remove it with `git worktree remove` or choose another graft name",
                        registration_id(name)
                    ),
                });
            }
            // Left by an interrupted `graft new` of this same graft: the
            // name is reserved in the store, so no live graft owns it.
            std::fs::remove_dir_all(&admin).map_err(io_err(&admin))?;
        }
        let branch = live_ref(name);
        let previous = self.claim_branch(name, &branch)?;
        let result = (|| -> Result<()> {
            self.set_ref(
                &branch,
                base,
                &format!("graft: create {}", live_branch(name)),
            )?;
            std::fs::create_dir_all(&admin).map_err(io_err(&admin))?;
            let write = |file: &str, data: &[u8]| {
                let path = admin.join(file);
                std::fs::write(&path, data).map_err(io_err(&path))
            };
            write("commondir", b"../..\n")?;
            let mut gitdir = graft_core::path_to_bytes(&dot_git);
            gitdir.push(b'\n');
            write("gitdir", &gitdir)?;
            write("HEAD", format!("ref: {branch}\n").as_bytes())?;
            let mut config = Vec::from(&b"# Written by graft (ADR 0027).\n"[..]);
            if layout == Layout::Virtual {
                // The mount reports commit times and blob sizes; inodes are
                // allocated lazily (ADR 0008).
                config.extend_from_slice(b"[core]\n\tcheckStat = minimal\n\ttrustctime = false\n");
            }
            write("config.worktree", &config)?;
            if layout == Layout::Virtual {
                write(
                    "locked",
                    format!(
                        "graft: virtual graft served by graftd; remove it with `graft prune {name}`\n"
                    )
                    .as_bytes(),
                )?;
            }
            // `graft run` grants writes to the graft's own ref namespace,
            // which must exist (ADR 0027).
            let logs = self.common_dir().join("logs/refs/heads/graft-live");
            std::fs::create_dir_all(&logs).map_err(io_err(&logs))?;
            if !layers.is_empty() {
                exclude_layers(&self.repo, &admin, layers)?;
            }
            if layout == Layout::Reflink {
                adopt_private_git(workdir, &admin)?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            let _ = std::fs::remove_dir_all(&admin);
            match previous {
                Some(old) => {
                    let _ = self.set_ref(&branch, &old, "graft: restore after a failed graft new");
                }
                None => {
                    let _ = self.delete_ref(&branch);
                }
            }
            return Err(e);
        }
        Ok(admin)
    }

    /// Removes graft `name`'s registration (only if it names `workdir`) and
    /// then, as `policy` says, its branch. Succeeds if neither exists.
    ///
    /// # Errors
    ///
    /// I/O and git errors.
    pub fn unregister(
        &self,
        name: &GraftName,
        workdir: &Path,
        policy: BranchPolicy,
    ) -> Result<BranchOutcome> {
        let worktrees = self.common_dir().join("worktrees");
        if let Some(admin) = self.registration(name, workdir) {
            std::fs::remove_dir_all(&admin).map_err(io_err(&admin))?;
            remove_empty_dirs(&worktrees, self.common_dir());
        }
        let branch = live_ref(name);
        let Some(tip) = self.ref_target(&branch)? else {
            return Ok(BranchOutcome::Absent);
        };
        if let Some(at) = self.checked_out_at(&branch) {
            return Ok(BranchOutcome::Kept {
                reason: format!("it is checked out at {}", at.display()),
            });
        }
        if policy == BranchPolicy::KeepIfUnique {
            let lost = only_on(&self.repo, &[(branch.clone(), tip)], &branch)?;
            if let Some(l) = lost.first() {
                return Ok(BranchOutcome::Kept {
                    reason: format!("commit {} is on no other branch", l.commit.short(7)),
                });
            }
        }
        self.delete_ref(&branch)?;
        let common = self.common_dir();
        remove_empty_dirs(
            &common.join("refs/heads/graft-live"),
            &common.join("refs/heads"),
        );
        remove_empty_dirs(
            &common.join("logs/refs/heads/graft-live"),
            &common.join("logs"),
        );
        Ok(BranchOutcome::Deleted)
    }

    /// Moves graft `name`'s [`live_branch`] to `commit` (for a virtual
    /// graft rebased before its index was ever built).
    ///
    /// # Errors
    ///
    /// If the ref cannot be written.
    pub fn move_live_branch(&self, name: &GraftName, commit: &CommitId) -> Result<()> {
        self.set_ref(
            &live_ref(name),
            commit,
            &format!("graft: rebase {}", live_branch(name)),
        )
    }

    /// Paths `graft run` must be able to write for a registered graft to
    /// commit: its registration, Root's object store and its own ref and
    /// reflog directories. Creates the directories if missing.
    ///
    /// # Errors
    ///
    /// If a directory cannot be created.
    pub fn registered_write_paths(&self, admin: &Path) -> Result<Vec<PathBuf>> {
        let common = self.common_dir();
        let paths = vec![
            admin.to_owned(),
            common.join("objects"),
            common.join("refs/heads/graft-live"),
            common.join("logs/refs/heads/graft-live"),
        ];
        for p in &paths {
            std::fs::create_dir_all(p).map_err(io_err(p))?;
        }
        Ok(paths)
    }

    /// Bytes used by the registration at `admin` (mostly its index).
    #[must_use]
    pub fn registration_bytes(admin: &Path) -> u64 {
        fn walk(dir: &Path) -> u64 {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return 0;
            };
            entries
                .flatten()
                .map(|e| match e.file_type() {
                    Ok(t) if t.is_dir() => walk(&e.path()),
                    Ok(_) => e.metadata().map_or(0, |m| m.len()),
                    Err(_) => 0,
                })
                .sum()
        }
        walk(admin)
    }

    /// Sets `extensions.worktreeConfig = true` in Root's config if it is
    /// not set, with git's lock protocol (`config.lock`, then rename).
    fn enable_worktree_config(&self) -> Result<()> {
        let path = self.common_dir().join("config");
        let lock = self.common_dir().join("config.lock");
        if worktree_config_set(&path)? {
            return Ok(());
        }
        let mut attempts = 0;
        let file = loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock)
            {
                Ok(f) => break f,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempts < 100 => {
                    attempts += 1;
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    if worktree_config_set(&path)? {
                        return Ok(());
                    }
                }
                Err(e) => return Err(io_err(&lock)(e)),
            }
        };
        let result = (|| -> Result<()> {
            use std::io::Write as _;
            // Read again under the lock: another process may have set it.
            let mut bytes = std::fs::read(&path).map_err(io_err(&path))?;
            if worktree_config_value(&bytes)?.is_some() {
                return Ok(());
            }
            if !bytes.is_empty() && !bytes.ends_with(b"\n") {
                bytes.push(b'\n');
            }
            bytes.extend_from_slice(b"[extensions]\n\tworktreeConfig = true\n");
            let mut file = file;
            file.write_all(&bytes).map_err(io_err(&lock))?;
            file.sync_all().map_err(io_err(&lock))?;
            std::fs::rename(&lock, &path).map_err(io_err(&path))
        })();
        let _ = std::fs::remove_file(&lock);
        result
    }

    /// Fails if `branch` exists with work it alone holds, or checked out in
    /// a worktree. Returns its current target, to restore on failure.
    fn claim_branch(&self, name: &GraftName, branch: &str) -> Result<Option<CommitId>> {
        let Some(tip) = self.ref_target(branch)? else {
            return Ok(None);
        };
        let shown = live_branch(name);
        if let Some(at) = self.checked_out_at(branch) {
            return Err(RootError::AlreadyExists {
                reason: format!(
                    "branch {shown} is checked out at {}; remove that worktree or choose another graft name",
                    at.display()
                ),
            });
        }
        if let Some(l) = only_on(&self.repo, &[(branch.to_owned(), tip)], branch)?.first() {
            return Err(RootError::AlreadyExists {
                reason: format!(
                    "branch {shown} already exists in Root and commit {} is on no other branch; rename or delete it, or choose another graft name",
                    l.commit.short(7)
                ),
            });
        }
        Ok(Some(to_commit_id(tip)?))
    }

    /// The worktree that has `branch` checked out, if any: Root itself or a
    /// linked worktree.
    fn checked_out_at(&self, branch: &str) -> Option<PathBuf> {
        let wanted = format!("ref: {branch}");
        let common = self.common_dir();
        let head_is = |head: &Path| {
            std::fs::read(head)
                .is_ok_and(|b| trim_newline(&b).trim_ascii_end() == wanted.as_bytes())
        };
        if head_is(&common.join("HEAD")) {
            return Some(self.workdir().to_owned());
        }
        let entries = std::fs::read_dir(common.join("worktrees")).ok()?;
        for e in entries.flatten() {
            if head_is(&e.path().join("HEAD")) {
                let at = std::fs::read(e.path().join("gitdir")).ok().map_or_else(
                    || e.path(),
                    |b| {
                        let p = graft_core::path_from_bytes(trim_newline(&b).to_vec());
                        p.parent().map_or(p.clone(), Path::to_owned)
                    },
                );
                return Some(at);
            }
        }
        None
    }

    fn ref_target(&self, name: &str) -> Result<Option<gix::ObjectId>> {
        let found = self
            .repo
            .try_find_reference(name)
            .map_err(|e| git_err("reading a graft branch")(boxed(e)))?;
        match found {
            Some(mut r) => Ok(Some(
                r.peel_to_id()
                    .map_err(|e| git_err("peeling a graft branch")(boxed(e)))?
                    .detach(),
            )),
            None => Ok(None),
        }
    }

    fn set_ref(&self, name: &str, commit: &CommitId, message: &str) -> Result<()> {
        self.repo
            .reference(
                name,
                oid(commit)?,
                gix::refs::transaction::PreviousValue::Any,
                message,
            )
            .map_err(|e| git_err("writing a graft branch")(boxed(e)))?;
        Ok(())
    }

    fn delete_ref(&self, name: &str) -> Result<()> {
        use gix::refs::transaction::{Change, PreviousValue, RefEdit, RefLog};
        let Some(existing) = self
            .repo
            .try_find_reference(name)
            .map_err(|e| git_err("reading a graft branch")(boxed(e)))?
        else {
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
            .map_err(|e| git_err("deleting a graft branch")(boxed(e)))?;
        Ok(())
    }
}

/// Adds `layers` to the excludes of the registration at `admin`: a
/// file holding the user's global excludes and then one anchored line
/// per layer, named by `core.excludesFile` in `config.worktree`
/// (`info/exclude` is shared with Root, so it cannot be used).
pub(crate) fn exclude_layers(repo: &gix::Repository, admin: &Path, layers: &[&Path]) -> Result<()> {
    use std::io::Write as _;
    let file = admin.join(EXCLUDE_FILE);
    if !file.is_file() {
        // `core.excludesFile` replaces the global excludes file, so
        // start from its contents.
        let mut text =
            Vec::from(&b"# Written by graft (ADR 0027): your global excludes, then layers.\n"[..]);
        if let Some(global) = global_excludes_file(repo)
            && let Ok(bytes) = std::fs::read(&global)
        {
            text.extend_from_slice(&bytes);
            if !text.ends_with(b"\n") {
                text.push(b'\n');
            }
        }
        std::fs::write(&file, text).map_err(io_err(&file))?;
        let config = admin.join("config.worktree");
        let mut out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&config)
            .map_err(io_err(&config))?;
        let mut line = Vec::from(&b"[core]\n\texcludesFile = "[..]);
        line.extend_from_slice(&crate::graft::config_quote(&graft_core::path_to_bytes(
            &file,
        )));
        line.push(b'\n');
        out.write_all(&line).map_err(io_err(&config))?;
    }
    let mut out = std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .map_err(io_err(&file))?;
    out.write_all(&layer_exclude_lines(layers))
        .map_err(io_err(&file))
}

/// The global excludes file git would read: `core.excludesFile`, else
/// `$XDG_CONFIG_HOME/git/ignore`, else `~/.config/git/ignore`.
fn global_excludes_file(repo: &gix::Repository) -> Option<PathBuf> {
    let snapshot = repo.config_snapshot();
    if let Ok(Some(path)) = snapshot.trusted_path("core.excludesFile") {
        return Some(path);
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg).join("git/ignore"));
    }
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(|home| PathBuf::from(home).join(".config/git/ignore"))
}

/// `extensions.worktreeConfig` in the config file at `path`: `Some` if set.
fn worktree_config_value(bytes: &[u8]) -> Result<Option<bool>> {
    let file = gix::config::File::from_bytes_no_includes(
        bytes,
        gix::config::file::Metadata::from(gix::config::Source::Local),
        gix::config::file::init::Options::default(),
    )
    .map_err(|e| git_err("parsing Root's config")(boxed(e)))?;
    file.boolean("extensions.worktreeConfig")
        .map_err(|e| git_err("reading extensions.worktreeConfig")(boxed(e)))
}

/// Whether Root's config already has `extensions.worktreeConfig = true`.
/// A Root that sets it to `false` is refused rather than overridden.
fn worktree_config_set(path: &Path) -> Result<bool> {
    let bytes = std::fs::read(path).map_err(io_err(path))?;
    match worktree_config_value(&bytes)? {
        Some(true) => Ok(true),
        Some(false) => Err(RootError::Unsupported {
            reason: "Root sets extensions.worktreeConfig = false; grafts registered as git worktrees need it (or set `[git] worktrees = false` in graft's config.toml)".to_owned(),
        }),
        None => Ok(false),
    }
}

/// Turns the private repository at `workdir/.git` (just built, with no
/// objects of its own) into a `.git` file pointing at `admin`: the index
/// moves into the registration.
fn adopt_private_git(workdir: &Path, admin: &Path) -> Result<()> {
    let private = workdir.join(".git");
    let index = private.join("index");
    move_file(&index, &admin.join("index"))?;
    let admin = admin.canonicalize().map_err(io_err(admin))?;
    let aside = workdir.join(".git.graft-private");
    std::fs::rename(&private, &aside).map_err(io_err(&private))?;
    let mut link = Vec::from(&b"gitdir: "[..]);
    link.extend_from_slice(&graft_core::path_to_bytes(&admin));
    link.push(b'\n');
    let written = write_atomic(&private, &link);
    if let Err(e) = written {
        let _ = std::fs::rename(&aside, &private);
        return Err(e);
    }
    std::fs::remove_dir_all(&aside).map_err(io_err(&aside))
}

/// Prepares a registered virtual graft's git state on first use (ADR
/// 0027): writes the index of `base` into `admin` if it is not there yet,
/// then `link`, the file the mount serves as `.git`. Opens no repository
/// through the mount, so it is safe to call while serving it.
///
/// # Errors
///
/// [`RootError::Unsupported`] if the registration is gone, plus git and
/// I/O errors.
pub fn prepare_virtual_git(root: &Repo, admin: &Path, link: &Path, base: &CommitId) -> Result<()> {
    if !admin.join("gitdir").is_file() {
        return Err(RootError::Unsupported {
            reason: format!(
                "the graft's git worktree registration {} is missing",
                admin.display()
            ),
        });
    }
    let index_path = admin.join("index");
    if !index_path.is_file() {
        let mut index = crate::graft::virtual_index(&root.repo, base)?;
        index.set_path(&index_path);
        index
            .write(gix::index::write::Options::default())
            .map_err(|e| git_err("writing the graft index")(boxed(e)))?;
    }
    if !link.is_file() {
        let admin = admin.canonicalize().map_err(io_err(admin))?;
        let mut text = Vec::from(&b"gitdir: "[..]);
        text.extend_from_slice(&graft_core::path_to_bytes(&admin));
        text.push(b'\n');
        write_atomic(link, &text)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(link, std::fs::Permissions::from_mode(0o444))
                .map_err(io_err(link))?;
        }
    }
    Ok(())
}
