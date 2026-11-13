//! `graft materialize` and `graft import` (ADR 0020).
//!
//! Materialize writes a graft's files (its base with its changes; not
//! layers or ignored files) out as a plain directory or a branch in Root,
//! for tools that cannot work on a graft directly. Import is the way back:
//! it makes a graft's files match a directory or branch, so the graft's
//! delta becomes the difference between that tree and the graft's base.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use graft_core::{CommitId, GraftId, GraftName, GraftState};
use graft_fs::{FsBackend, GraftSpec, ReflinkBackend};
use graft_root::graft::{ChangeKind, FileDiff};

use crate::grafts::{Ctx, daemon, find, open, parse_name};
use crate::integrate::{changes, git, graft_is_link, graft_mode, read_entry, write_entry};
use crate::{Environment, Output};

/// Arguments for `graft materialize`.
#[derive(Debug, clap::Args)]
pub(crate) struct MaterializeArgs {
    /// The graft.
    name: String,
    /// Create this branch in Root, one commit on the graft's base.
    #[arg(long, value_name = "BRANCH", required_unless_present = "dir")]
    branch: Option<String>,
    /// Write the files to this directory, which must not exist or be empty.
    #[arg(long, value_name = "PATH")]
    dir: Option<PathBuf>,
}

/// Arguments for `graft import`.
#[derive(Debug, clap::Args)]
pub(crate) struct ImportArgs {
    /// The graft whose files to replace.
    name: String,
    /// Take the files from this directory (its `.git` is ignored).
    #[arg(
        long,
        value_name = "PATH",
        conflicts_with = "branch",
        required_unless_present = "branch"
    )]
    dir: Option<PathBuf>,
    /// Take the files from this branch or commit of Root.
    #[arg(long, value_name = "REV")]
    branch: Option<String>,
}

/// A scratch checkout of `commit` under `$GRAFT_HOME`, removed on drop.
struct Scratch {
    backend: ReflinkBackend,
    name: GraftName,
    id: GraftId,
    path: PathBuf,
}

impl Scratch {
    fn new(ctx: &Ctx, commit: &CommitId) -> anyhow::Result<Self> {
        let backend = ReflinkBackend::new(ctx.root_dir().join("integrate"));
        let id = GraftId::random()?;
        let name: GraftName = format!("export-{}", &id.to_string()[..12]).parse()?;
        let created = backend.create(
            &ctx.located.repo,
            &GraftSpec {
                id,
                name: &name,
                base: commit,
            },
        )?;
        Ok(Self {
            backend,
            name,
            id,
            path: created.path,
        })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(e) = self.backend.prune(&self.name, self.id) {
            tracing::warn!(error = %e, "could not remove a scratch checkout");
        }
    }
}

/// Applies a graft's changes to a checkout of its base.
fn apply_changes(
    ctx: &Ctx,
    record: &graft_store::GraftRecord,
    dir: &Path,
    diffs: &[FileDiff],
) -> anyhow::Result<()> {
    for d in diffs {
        let target = dir.join(&d.change.path);
        let link = graft_is_link(ctx, record, &d.change.path);
        write_entry(&target, d.new.as_deref(), link)?;
        if !link
            && d.new.is_some()
            && let Some(mode) = graft_mode(ctx, record, &d.change.path)
        {
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}

/// `graft materialize`.
pub(crate) fn materialize(args: &MaterializeArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let ctx = open(env)?;
    let record = find(&ctx.located.store, &name)?;
    if let Some(dir) = &args.dir
        && dir.exists()
        && std::fs::read_dir(dir).map_or(true, |mut d| d.next().is_some())
    {
        bail!(
            "{} exists and is not an empty directory; materialize never overwrites",
            dir.display()
        );
    }
    if let Some(b) = &args.branch
        && ctx
            .located
            .repo
            .resolve_commit(&format!("refs/heads/{b}"))
            .is_ok()
    {
        bail!("branch {b} already exists in Root; choose another name");
    }
    let mut out = Output::default();
    let diffs = changes(&ctx, &record, &mut out)?;
    let scratch = Scratch::new(&ctx, &record.base_commit)?;
    apply_changes(&ctx, &record, &scratch.path, &diffs)?;
    if let Some(b) = &args.branch {
        git(&scratch.path, &["add", "-A"])?;
        let msg = format!(
            "chore: materialize graft {name}\n\nGraft: {name} {}\n",
            record.id
        );
        let msg_path = scratch.path.join(".git").join("GRAFT_MATERIALIZE_MSG");
        std::fs::write(&msg_path, msg)?;
        git(
            &scratch.path,
            &[
                "commit",
                "-q",
                "--no-verify",
                "--allow-empty",
                "-F",
                &msg_path.display().to_string(),
            ],
        )?;
        let git_dir = ctx.located.repo.git_dir().display().to_string();
        git(
            &scratch.path,
            &[
                "push",
                "-q",
                "--no-verify",
                &git_dir,
                &format!("HEAD:refs/heads/{b}"),
            ],
        )?;
        let commit = git(&scratch.path, &["rev-parse", "HEAD"])?;
        let _ = writeln!(
            out.stdout,
            "Materialized graft {name} as branch {b} ({})",
            &commit[..12]
        );
    }
    if let Some(dir) = &args.dir {
        // An empty directory may be there already; rmdir cannot remove
        // anything else.
        if dir.exists() {
            std::fs::remove_dir(dir)
                .with_context(|| format!("replacing empty {}", dir.display()))?;
        }
        let parent = dir
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
        let cloner = match graft_fs::same_filesystem(&scratch.path, parent) {
            Ok(true) => graft_fs::ReflinkCloner::new(),
            _ => graft_fs::ReflinkCloner::copy_only("different filesystems"),
        };
        graft_fs::clone_tree(&scratch.path, dir, &cloner)
            .with_context(|| format!("writing {}", dir.display()))?;
        let copied_git = dir.join(".git");
        std::fs::remove_dir_all(&copied_git)
            .with_context(|| format!("removing {}", copied_git.display()))?;
        let _ = writeln!(
            out.stdout,
            "Materialized graft {name} into {}",
            dir.display()
        );
    }
    Ok(out)
}

/// Every file under `dir`, relative, except `.git`.
fn walk(dir: &Path) -> anyhow::Result<BTreeSet<PathBuf>> {
    let mut out = BTreeSet::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        for e in std::fs::read_dir(dir.join(&rel))
            .with_context(|| format!("reading {}", dir.join(&rel).display()))?
        {
            let e = e?;
            if rel.as_os_str().is_empty() && e.file_name() == ".git" {
                continue;
            }
            let p = rel.join(e.file_name());
            if e.file_type()?.is_dir() {
                stack.push(p);
            } else {
                out.insert(p);
            }
        }
    }
    Ok(out)
}

/// `dir/rel`, refusing a path whose existing parent resolves outside `dir`.
fn inside(dir: &Path, rel: &Path) -> anyhow::Result<PathBuf> {
    let root = dir.canonicalize()?;
    let full = root.join(rel);
    let mut existing = full.parent().unwrap_or(&root).to_owned();
    while !existing.exists() {
        existing = existing.parent().unwrap_or(&root).to_owned();
    }
    if !existing.canonicalize()?.starts_with(&root) {
        bail!("{} leaves the graft", rel.display());
    }
    Ok(full)
}

/// Removes empty directories from `start` up to (not including) `root`.
fn prune_empty(root: &Path, start: &Path) {
    let mut d = start.to_owned();
    while d.starts_with(root) && d != root {
        if std::fs::remove_dir(&d).is_err() {
            break;
        }
        if !d.pop() {
            break;
        }
    }
}

/// `graft import`.
pub(crate) fn import(args: &ImportArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let ctx = open(env)?;
    let record = find(&ctx.located.store, &name)?;
    if record.state != GraftState::Live {
        bail!(
            "graft {name} is {}; only live grafts can import",
            record.state
        );
    }
    let mut out = Output::default();
    let scratch;
    let source = match (&args.dir, &args.branch) {
        (Some(d), _) => d
            .canonicalize()
            .with_context(|| format!("reading {}", d.display()))?,
        (None, Some(rev)) => {
            let commit = ctx.located.repo.resolve_commit(rev)?;
            scratch = Scratch::new(&ctx, &commit)?;
            scratch.path.clone()
        }
        (None, None) => bail!("give --dir or --branch"),
    };
    let target = if record.backend.is_virtual() {
        daemon(&ctx.home, &mut out)?.mount(&ctx.located.record.id, &name)?
    } else {
        ctx.backend.path(&name)
    };
    // What the graft has now: its base's files, with its own adds and
    // deletes. Ignored files and layers are not part of it.
    let listed = git(
        &ctx.located.record.path,
        &[
            "ls-tree",
            "-r",
            "--name-only",
            "-z",
            record.base_commit.as_str(),
        ],
    )?;
    let mut current: BTreeSet<PathBuf> = listed
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .collect();
    for d in changes(&ctx, &record, &mut out)? {
        match d.change.kind {
            ChangeKind::Deleted => {
                current.remove(&d.change.path);
            }
            _ => {
                current.insert(d.change.path);
            }
        }
    }
    let wanted = walk(&source)?;
    let (mut written, mut removed) = (0, 0);
    for rel in &wanted {
        let src = source.join(rel);
        let (bytes, link) = read_entry(&src)?.unwrap_or_default();
        let dst = inside(&target, rel)?;
        if read_entry(&dst)?.as_ref() != Some(&(bytes.clone(), link)) {
            write_entry(&dst, Some(&bytes), link)?;
            written += 1;
        }
        if !link {
            let mode = std::fs::metadata(&src)?.permissions().mode() & 0o777;
            let have = std::fs::metadata(&dst)?.permissions().mode() & 0o777;
            if (mode & 0o111 != 0) != (have & 0o111 != 0) {
                std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(mode))?;
                written += 1;
            }
        }
    }
    for rel in current.difference(&wanted) {
        let dst = inside(&target, rel)?;
        if std::fs::symlink_metadata(&dst).is_ok() {
            write_entry(&dst, None, false)?;
            removed += 1;
            if let Some(parent) = dst.parent() {
                prune_empty(&target, parent);
            }
        }
    }
    let _ = writeln!(
        out.stdout,
        "Imported {} into graft {name}: {written} written, {removed} removed",
        args.dir.as_ref().map_or_else(
            || args.branch.clone().unwrap_or_default(),
            |d| d.display().to_string()
        )
    );
    Ok(out)
}
