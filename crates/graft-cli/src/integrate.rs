//! `graft integrate`, `graft queue` and `graft rebase` (ADR 0017).
//!
//! Integration happens in a scratch reflink checkout of the target's head
//! under `$GRAFT_HOME/roots/<root-id>/integrate/`: the graft's changes are
//! merged in file by file, conflicts go to the resolver hook, the test gate
//! runs confined, and only then is a commit made and one ref of Root moved
//! with compare-and-swap. Root's working tree is never written, unless the
//! operator passes `--update-checkout` for the branch Root has checked out.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, bail};
use graft_blueprint::{BLUEPRINT_FILE, IntegrateMode, IntegrateSpec, parse_integrate};
use graft_core::{CommitId, GraftId, GraftName, GraftState};
use graft_fs::{FsBackend, GraftSpec, ReflinkBackend};
use graft_integrate::{Merged, Merger, Method};
use graft_root::Repo;
use graft_root::graft::{FileDiff, GraftRepo};
use graft_store::{GraftRecord, Integration, IntegrationState};

use crate::grafts::{Ctx, find, open, parse_name};
use crate::{Environment, Output};

/// Arguments for `graft integrate`.
#[derive(Debug, clap::Args)]
pub(crate) struct IntegrateArgs {
    /// The grafts to integrate, in order.
    #[arg(required = true)]
    names: Vec<String>,
    /// The branch to land on. Defaults to `[integrate] branch` in
    /// graft.toml, then the branch Root has checked out.
    #[arg(long, value_name = "BRANCH")]
    onto: Option<String>,
    /// The commit message (else the graft's first commit subject, else a
    /// generated one).
    #[arg(long, short)]
    message: Option<String>,
    /// Run every step except landing.
    #[arg(long)]
    dry_run: bool,
    /// Prune each graft once it has landed, instead of marking it
    /// integrated.
    #[arg(long)]
    prune: bool,
    /// Allow landing on the branch Root has checked out: if Root's working
    /// tree is clean, it is fast-forwarded with `git merge --ff-only`.
    #[arg(long)]
    update_checkout: bool,
    /// A task description for the resolver hook.
    #[arg(long, value_name = "FILE")]
    task: Option<PathBuf>,
}

/// Arguments for `graft queue`.
#[derive(Debug, clap::Args)]
pub(crate) struct QueueArgs {
    /// How many recent requests to show.
    #[arg(long, default_value_t = 20)]
    limit: u32,
    /// Show every request's full output (stopped ones show three lines
    /// by default).
    #[arg(long, short)]
    verbose: bool,
}

/// Arguments for `graft rebase`.
#[derive(Debug, clap::Args)]
pub(crate) struct RebaseArgs {
    /// The graft to move.
    name: String,
    /// The branch whose head to move it to (default as for integrate).
    #[arg(long, value_name = "BRANCH")]
    onto: Option<String>,
}

/// Runs `git` in `dir` and returns its stdout, trimmed.
fn git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context("running git")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// The `[integrate]` table at `commit`.
fn spec_at(repo: &Repo, commit: &CommitId) -> anyhow::Result<IntegrateSpec> {
    let text = repo
        .read_file_at(commit, Path::new(BLUEPRINT_FILE))?
        .map(String::from_utf8)
        .transpose()
        .context("graft.toml is not valid UTF-8")?
        .unwrap_or_default();
    Ok(parse_integrate(&text, BLUEPRINT_FILE)?)
}

/// The trust hashes of the integrate gate and resolver.
pub(crate) fn approvals(spec: &IntegrateSpec) -> Vec<(graft_core::CommandHash, String)> {
    let mut out = Vec::new();
    if let Some(t) = &spec.test {
        out.push(format!("test `{t}`"));
    }
    if let Some(r) = &spec.resolver {
        out.push(format!("resolver `{r}`"));
    }
    out.into_iter()
        .map(|item| {
            (
                graft_root::layer::blueprint_item_hash("integrate", &item),
                format!("integrate {item}"),
            )
        })
        .collect()
}

/// `graft trust`'s approvals for `[integrate]` at `commit`.
pub(crate) fn approvals_at(
    repo: &Repo,
    commit: &CommitId,
) -> anyhow::Result<Vec<(graft_core::CommandHash, String)>> {
    Ok(approvals(&spec_at(repo, commit)?))
}

/// The branch to land on: `--onto`, `[integrate] branch`, Root's branch.
fn target_branch(ctx: &Ctx, onto: Option<&str>) -> anyhow::Result<String> {
    if let Some(b) = onto {
        return Ok(b.trim_start_matches("refs/heads/").to_owned());
    }
    let head = ctx.located.repo.head_commit()?;
    if let Some(b) = spec_at(&ctx.located.repo, &head)?.branch {
        return Ok(b);
    }
    match ctx.located.repo.head_ref()? {
        Some(r) => Ok(r.trim_start_matches("refs/heads/").to_owned()),
        None => bail!("Root's HEAD is detached; choose a branch with --onto"),
    }
}

/// The commit `branch` points at in Root.
fn branch_head(ctx: &Ctx, branch: &str) -> anyhow::Result<CommitId> {
    ctx.located
        .repo
        .resolve_commit(&format!("refs/heads/{branch}"))
        .with_context(|| format!("no branch {branch} in Root"))
}

/// A graft's changes against its base, with contents.
fn changes(ctx: &Ctx, record: &GraftRecord, out: &mut Output) -> anyhow::Result<Vec<FileDiff>> {
    let path = ctx.backend.path(&record.name);
    if record.backend.is_virtual() {
        return crate::virtual_grafts::diffs(
            &ctx.home,
            &ctx.located,
            &ctx.root_dir(),
            &path,
            record,
            &mut out.warnings,
        );
    }
    Ok(GraftRepo::open(&path)?.diff_since(&record.base_commit)?)
}

/// The permission bits of `path` in the graft, if it is a file there.
fn graft_mode(ctx: &Ctx, record: &GraftRecord, path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::symlink_metadata(ctx.backend.path(&record.name).join(path))
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map(|m| m.permissions().mode() & 0o777)
}

/// What merging a graft's changes into a directory did.
#[derive(Default)]
struct Applied {
    structural: Vec<PathBuf>,
    conflicts: Vec<(PathBuf, String)>,
}

/// Merges `diffs` into `dir`, whose files are "ours". Conflicting files
/// get conflict markers where both sides have them.
fn apply(
    ctx: &Ctx,
    record: &GraftRecord,
    dir: &Path,
    diffs: &[FileDiff],
    merger: &Merger,
) -> anyhow::Result<Applied> {
    use std::os::unix::fs::PermissionsExt as _;
    let mut applied = Applied::default();
    for d in diffs {
        let target = dir.join(&d.change.path);
        let (ours, ours_link) = read_entry(&target)?.map_or((None, false), |(b, l)| (Some(b), l));
        let theirs_link = graft_is_link(ctx, record, &d.change.path);
        if ours_link || theirs_link {
            // Symlinks are not merged: one side must have left it alone.
            if ours.as_deref() == d.new.as_deref() && ours_link == theirs_link {
                continue;
            }
            if ours.as_deref() == d.old.as_deref() {
                write_entry(&target, d.new.as_deref(), theirs_link)?;
                continue;
            }
            applied.conflicts.push((
                d.change.path.clone(),
                "a symlink changed in both Root and the graft".to_owned(),
            ));
            continue;
        }
        let result = merger.merge(
            &d.change.path,
            d.old.as_deref(),
            ours.as_deref(),
            d.new.as_deref(),
            dir,
        )?;
        let contents = match result {
            Merged::Clean { contents, method } => {
                if method == Method::Structural {
                    applied.structural.push(d.change.path.clone());
                }
                contents
            }
            Merged::Conflict { markers, reason } => {
                applied.conflicts.push((d.change.path.clone(), reason));
                match markers {
                    Some(m) => Some(m),
                    None => continue,
                }
            }
        };
        match contents {
            Some(bytes) => {
                if let Some(p) = target.parent() {
                    std::fs::create_dir_all(p)
                        .with_context(|| format!("creating {}", p.display()))?;
                }
                std::fs::write(&target, bytes)
                    .with_context(|| format!("writing {}", target.display()))?;
                if let Some(mode) = graft_mode(ctx, record, &d.change.path) {
                    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode))?;
                }
            }
            None => match std::fs::remove_file(&target) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("removing {}", target.display())),
            },
        }
    }
    Ok(applied)
}

/// The contents of `path` (a symlink's target), and whether it is a
/// symlink; `None` if absent.
fn read_entry(path: &Path) -> anyhow::Result<Option<(Vec<u8>, bool)>> {
    use std::os::unix::ffi::OsStrExt as _;
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("inspecting {}", path.display())),
    };
    if meta.file_type().is_symlink() {
        let t = std::fs::read_link(path).with_context(|| format!("reading {}", path.display()))?;
        return Ok(Some((t.as_os_str().as_bytes().to_vec(), true)));
    }
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(Some((bytes, false)))
}

/// Replaces `path` with a file or a symlink (`contents` is its target), or
/// removes it if `contents` is `None`.
fn write_entry(path: &Path, contents: Option<&[u8]>, link: bool) -> anyhow::Result<()> {
    use std::os::unix::ffi::OsStrExt as _;
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => bail!("{} is a directory", path.display()),
        Ok(_) => {
            std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
        }
        Err(_) => {}
    }
    let Some(bytes) = contents else {
        return Ok(());
    };
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).with_context(|| format!("creating {}", p.display()))?;
    }
    if link {
        std::os::unix::fs::symlink(std::ffi::OsStr::from_bytes(bytes), path)
            .with_context(|| format!("creating symlink {}", path.display()))
    } else {
        std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
    }
}

/// Whether `path` is a symlink in the graft.
fn graft_is_link(ctx: &Ctx, record: &GraftRecord, path: &Path) -> bool {
    std::fs::symlink_metadata(ctx.backend.path(&record.name).join(path))
        .is_ok_and(|m| m.file_type().is_symlink())
}

/// Whether `bytes` still has a conflict marker line.
fn has_markers(bytes: &[u8]) -> bool {
    String::from_utf8_lossy(bytes)
        .lines()
        .any(|l| l.starts_with("<<<<<<< ") || l.starts_with(">>>>>>> ") || l == "=======")
}

/// The commit message for graft `record`.
fn message_for(ctx: &Ctx, record: &GraftRecord, given: Option<&str>) -> String {
    let subject = given.map(str::to_owned).or_else(|| {
        let dir = ctx.backend.path(&record.name);
        let range = format!("{}..HEAD", record.base_commit);
        git(&dir, &["log", "--reverse", "--format=%s", &range])
            .ok()
            .and_then(|s| s.lines().next().map(str::to_owned))
            .filter(|s| !s.is_empty())
    });
    let subject = subject.unwrap_or_else(|| format!("chore: integrate graft {}", record.name));
    format!("{subject}\n\nGraft: {} {}\n", record.name, record.id)
}

/// The outcome of one request.
struct Outcome {
    state: IntegrationState,
    output: String,
    commit: Option<String>,
}

impl Outcome {
    fn stop(state: IntegrationState, output: impl Into<String>) -> Self {
        Self {
            state,
            output: output.into(),
            commit: None,
        }
    }
}

/// Context the resolver hook gets, as files in the scratch checkout.
fn write_resolver_inputs(
    ctx: &Ctx,
    record: &GraftRecord,
    dir: &Path,
    head: &CommitId,
    diffs: &[FileDiff],
    task: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    let info = dir.join(".graft-integrate");
    std::fs::create_dir_all(&info)?;
    let graft_diff: String = diffs.iter().map(graft_root::graft::unified_diff).collect();
    std::fs::write(info.join("graft.diff"), graft_diff)?;
    let root_range = format!("{}..{}", record.base_commit, head);
    let root_diff = git(&ctx.located.record.path, &["diff", &root_range]).unwrap_or_default();
    std::fs::write(info.join("root.diff"), root_diff)?;
    let log = ctx
        .located
        .store
        .op_log(record.id)?
        .iter()
        .fold(String::new(), |mut s, r| {
            let _ = writeln!(
                s,
                "{} {} {}",
                r.seq,
                r.op.kind(),
                String::from_utf8_lossy(r.op.path())
            );
            s
        });
    std::fs::write(info.join("graft.oplog"), log)?;
    if let Some(t) = task {
        std::fs::copy(t, info.join("task.md"))
            .with_context(|| format!("reading {}", t.display()))?;
    }
    Ok(info)
}

/// Runs the test gate in `dir`; an [`Outcome`] if it stopped the entry.
fn gate(dir: &Path, spec: &IntegrateSpec, log: &mut String) -> anyhow::Result<Option<Outcome>> {
    let Some(test) = &spec.test else {
        log.push_str("no test gate ([integrate] test is not set)\n");
        return Ok(None);
    };
    let timeout = Duration::from_secs(spec.timeout_secs);
    let (status, tail) = crate::run::run_scratch(dir, test, &[("GRAFT_INTEGRATE", "1")], timeout)?;
    Ok(match status {
        Some(s) if s.success() => {
            let _ = writeln!(log, "test gate `{test}` passed");
            None
        }
        Some(s) => Some(Outcome::stop(
            IntegrationState::Failed,
            format!("{log}test gate `{test}` failed ({s}):\n{tail}"),
        )),
        None => Some(Outcome::stop(
            IntegrationState::Failed,
            format!(
                "{log}test gate `{test}` timed out after {}s:\n{tail}",
                spec.timeout_secs
            ),
        )),
    })
}

/// Hands remaining conflicts to the resolver hook; an [`Outcome`] if they
/// stop the entry.
#[allow(clippy::too_many_arguments)] // one call site
fn resolve(
    ctx: &Ctx,
    record: &GraftRecord,
    dir: &Path,
    head: &CommitId,
    diffs: &[FileDiff],
    applied: &Applied,
    spec: &IntegrateSpec,
    args: &IntegrateArgs,
    log: &mut String,
) -> anyhow::Result<Option<Outcome>> {
    let listed = applied
        .conflicts
        .iter()
        .fold(String::new(), |mut s, (p, why)| {
            let _ = writeln!(s, "  {}: {why}", p.display());
            s
        });
    let Some(resolver) = &spec.resolver else {
        return Ok(Some(Outcome::stop(
            IntegrationState::Conflict,
            format!("{log}conflicts:\n{listed}"),
        )));
    };
    let info = write_resolver_inputs(ctx, record, dir, head, diffs, args.task.as_deref())?;
    let paths: Vec<String> = applied
        .conflicts
        .iter()
        .map(|(p, _)| p.display().to_string())
        .collect();
    let joined = paths.join("\n");
    let info_s = info.display().to_string();
    let (status, tail) = crate::run::run_scratch(
        dir,
        resolver,
        &[
            ("GRAFT_CONFLICTS", &joined),
            ("GRAFT_INTEGRATE_INFO", &info_s),
            ("GRAFT_NAME", record.name.as_str()),
        ],
        Duration::from_secs(spec.timeout_secs),
    )?;
    std::fs::remove_dir_all(&info)?;
    let unresolved = paths
        .iter()
        .any(|p| std::fs::read(dir.join(p)).map_or(true, |b| has_markers(&b)));
    if !status.is_some_and(|s| s.success()) || unresolved {
        return Ok(Some(Outcome::stop(
            IntegrationState::Conflict,
            format!("{log}conflicts:\n{listed}resolver `{resolver}` did not resolve them:\n{tail}"),
        )));
    }
    let _ = writeln!(log, "resolver `{resolver}` resolved: {}", paths.join(", "));
    Ok(None)
}

/// Runs one request in a scratch checkout of the target's head.
fn attempt(
    ctx: &mut Ctx,
    entry: &Integration,
    record: &GraftRecord,
    args: &IntegrateArgs,
    out: &mut Output,
) -> anyhow::Result<Outcome> {
    let head = branch_head(ctx, &entry.target)?;
    let spec = spec_at(&ctx.located.repo, &head)?;
    for (hash, what) in approvals(&spec) {
        if !ctx.located.store.is_trusted(&hash)? {
            return Ok(Outcome::stop(
                IntegrationState::Failed,
                format!("{what} is not approved; review it and run `graft trust`"),
            ));
        }
    }
    let diffs = changes(ctx, record, out)?;
    if diffs.is_empty() {
        return Ok(Outcome::stop(
            IntegrationState::Failed,
            "nothing to integrate: no changes since base",
        ));
    }
    let backend = ReflinkBackend::new(ctx.root_dir().join("integrate"));
    let id = GraftId::random()?;
    let name: GraftName = format!("land-{}", &id.to_string()[..12]).parse()?;
    let created = backend.create(
        &ctx.located.repo,
        &GraftSpec {
            id,
            name: &name,
            base: &head,
        },
    )?;
    let result = (|| -> anyhow::Result<Outcome> {
        let dir = created.path.clone();
        if let Ok(planned) = crate::layers::plan(
            &ctx.located.repo,
            &ctx.located.store,
            &ctx.layers,
            &head,
            None,
            None,
        ) {
            crate::layers::link_ready(&planned, &ctx.layers, &dir)?;
        }
        let merger = Merger::detect();
        let applied = apply(ctx, record, &dir, &diffs, &merger)?;
        let mut log = String::new();
        if !applied.structural.is_empty() {
            let list: Vec<String> = applied
                .structural
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            let _ = writeln!(log, "structural merge (Mergiraf): {}", list.join(", "));
        }
        if !applied.conflicts.is_empty()
            && let Some(stop) = resolve(
                ctx, record, &dir, &head, &diffs, &applied, &spec, args, &mut log,
            )?
        {
            return Ok(stop);
        }
        if let Some(stop) = gate(&dir, &spec, &mut log)? {
            return Ok(stop);
        }
        if entry.dry_run {
            return Ok(Outcome::stop(IntegrationState::Checked, log));
        }
        git(&dir, &["add", "-A"])?;
        let msg_path = dir.join(".git").join("GRAFT_INTEGRATE_MSG");
        std::fs::write(
            &msg_path,
            message_for(ctx, record, entry.message.as_deref()),
        )?;
        git(
            &dir,
            &[
                "commit",
                "-q",
                "--no-verify",
                "-F",
                &msg_path.display().to_string(),
            ],
        )?;
        let new = git(&dir, &["rev-parse", "HEAD"])?;
        land(ctx, &dir, &head, &new, &entry.target, record, &spec, args)?;
        let _ = writeln!(
            log,
            "landed {} on {}",
            &new[..12],
            landed_ref(&spec, &entry.target, record)
        );
        if spec.mode == IntegrateMode::Push {
            publish(ctx, &spec, &entry.target, record, &msg_path, &mut log)?;
        }
        Ok(Outcome {
            state: IntegrationState::Landed,
            output: log,
            commit: Some(new),
        })
    })();
    backend.prune(&name, id)?;
    result
}

fn landed_ref(spec: &IntegrateSpec, target: &str, record: &GraftRecord) -> String {
    match spec.mode {
        IntegrateMode::Commit => format!("refs/heads/{target}"),
        IntegrateMode::Branch | IntegrateMode::Push => {
            format!("refs/heads/graft/{}", record.name)
        }
    }
}

/// `push` mode: pushes `graft/<name>` to the remote and, with `pr`,
/// opens a pull request into `target` with `gh`.
fn publish(
    ctx: &Ctx,
    spec: &IntegrateSpec,
    target: &str,
    record: &GraftRecord,
    message: &Path,
    log: &mut String,
) -> anyhow::Result<()> {
    let root = &ctx.located.record.path;
    let branch = format!("graft/{}", record.name);
    let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
    git(
        root,
        &["push", "-q", "--force-with-lease", &spec.remote, &refspec],
    )
    .with_context(|| format!("pushing {branch} to {}", spec.remote))?;
    let _ = writeln!(log, "pushed {branch} to {}", spec.remote);
    if !spec.pr {
        return Ok(());
    }
    let text = std::fs::read_to_string(message)?;
    let (title, body) = text.split_once('\n').unwrap_or((text.as_str(), ""));
    let gh = Command::new("gh")
        .args([
            "pr",
            "create",
            "--head",
            &branch,
            "--base",
            target,
            "--title",
            title,
            "--body",
            body.trim(),
        ])
        .current_dir(root)
        .output();
    match gh {
        Ok(o) if o.status.success() => {
            let _ = writeln!(log, "opened {}", String::from_utf8_lossy(&o.stdout).trim());
        }
        Ok(o) => {
            let _ = writeln!(
                log,
                "could not open a pull request (gh: {}); {branch} is pushed",
                String::from_utf8_lossy(&o.stderr).trim()
            );
        }
        Err(_) => {
            let _ = writeln!(
                log,
                "gh is not installed, so no pull request was opened; {branch} is pushed"
            );
        }
    }
    Ok(())
}

/// Copies commit `new` into Root and moves the ref from `head` to it.
#[allow(clippy::too_many_arguments)] // one call site
fn land(
    ctx: &Ctx,
    scratch: &Path,
    head: &CommitId,
    new: &str,
    target: &str,
    record: &GraftRecord,
    spec: &IntegrateSpec,
    args: &IntegrateArgs,
) -> anyhow::Result<()> {
    let root = ctx.located.record.path.clone();
    let git_dir = ctx.located.repo.git_dir().display().to_string();
    let staging = format!("refs/graft/integrating/{}", record.id);
    // Push copies the objects into Root's object store.
    git(
        scratch,
        &[
            "push",
            "-q",
            "--no-verify",
            &git_dir,
            &format!("{new}:{staging}"),
        ],
    )?;
    let moved = (|| {
        let refname = landed_ref(spec, target, record);
        if spec.mode != IntegrateMode::Commit {
            return git(&root, &["update-ref", &refname, new]).map(drop);
        }
        let checked_out = ctx.located.repo.head_ref()?.as_deref() == Some(refname.as_str());
        if !checked_out {
            return git(&root, &["update-ref", &refname, new, head.as_str()]).map(drop);
        }
        if !args.update_checkout {
            bail!(
                "{target} is checked out in Root, so its working tree would fall behind; \
                 land elsewhere with --onto, or pass --update-checkout to fast-forward Root"
            );
        }
        let dirty = git(&root, &["status", "--porcelain", "--untracked-files=no"])?;
        if !dirty.is_empty() {
            bail!("Root's working tree has changes, so it cannot be fast-forwarded:\n{dirty}");
        }
        if git(&root, &["rev-parse", "HEAD"])? != head.as_str() {
            bail!("{target} moved while integrating");
        }
        git(&root, &["merge", "--ff-only", "-q", new]).map(drop)
    })();
    let _ = git(&root, &["update-ref", "-d", &staging]);
    moved
}

/// Processes the queue in order, holding `integrate.lock`.
fn process(ctx: &mut Ctx, args: &IntegrateArgs, out: &mut Output) -> anyhow::Result<bool> {
    let lock_path = ctx.root_dir().join("integrate.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("opening {}", lock_path.display()))?;
    lock.lock()
        .with_context(|| format!("locking {}", lock_path.display()))?;
    ctx.located.store.requeue_running()?;
    let mut all_ok = true;
    while let Some(entry) = ctx.located.store.next_queued()? {
        ctx.located
            .store
            .set_integration(entry.id, IntegrationState::Running, "", None)?;
        let record = ctx.located.store.graft_by_id(entry.graft_id)?;
        let outcome = match record {
            Some(r) if r.state == GraftState::Live => {
                let mut o = attempt(ctx, &entry, &r, args, out);
                // The target moved under us: retry once from its new head.
                if matches!(&o, Err(e) if format!("{e:#}").contains("update-ref")) {
                    o = attempt(ctx, &entry, &r, args, out);
                }
                o.unwrap_or_else(|e| Outcome::stop(IntegrationState::Failed, format!("{e:#}")))
            }
            Some(r) => Outcome::stop(
                IntegrationState::Failed,
                format!("graft {} is {}, not live", r.name, r.state),
            ),
            None => Outcome::stop(IntegrationState::Failed, "the graft no longer exists"),
        };
        ctx.located.store.set_integration(
            entry.id,
            outcome.state,
            &outcome.output,
            outcome.commit.as_deref(),
        )?;
        let short = outcome.commit.as_deref().map_or("", |c| &c[..12]);
        let _ = writeln!(
            out.stdout,
            "{:<8} {} → {} {short}",
            outcome.state.as_str(),
            entry.graft_name,
            entry.target
        );
        if outcome.state == IntegrationState::Landed {
            if args.prune {
                let pruned = crate::grafts::prune(
                    &crate::grafts::PruneArgs::forced(entry.graft_name.clone()),
                    &env_of(ctx),
                );
                if let Err(e) = pruned {
                    out.warnings.push(format!(
                        "landed {}, but pruning it failed: {e:#}",
                        entry.graft_name
                    ));
                }
            } else {
                ctx.located
                    .store
                    .set_state(entry.graft_id, GraftState::Integrated)?;
            }
        } else if outcome.state != IntegrationState::Checked {
            all_ok = false;
            for line in outcome.output.lines() {
                let _ = writeln!(out.stdout, "    {line}");
            }
        }
    }
    drop(lock);
    Ok(all_ok)
}

/// An [`Environment`] for calling other commands from inside one.
fn env_of(ctx: &Ctx) -> Environment {
    Environment {
        home: Ok(ctx.home.clone()),
        config: None,
    }
}

/// `graft integrate`.
pub(crate) fn integrate(
    args: &IntegrateArgs,
    env: &Environment,
) -> anyhow::Result<(Output, std::process::ExitCode)> {
    let mut ctx = open(env)?;
    let target = target_branch(&ctx, args.onto.as_deref())?;
    branch_head(&ctx, &target)?;
    if let Some(t) = &args.task
        && !t.is_file()
    {
        bail!("task file {} does not exist", t.display());
    }
    for n in &args.names {
        let name = parse_name(n)?;
        let record = find(&ctx.located.store, &name)?;
        if record.state != GraftState::Live {
            bail!(
                "graft {name} is {}; only live grafts can be integrated",
                record.state
            );
        }
    }
    for n in &args.names {
        let name = parse_name(n)?;
        let record = find(&ctx.located.store, &name)?;
        ctx.located.store.enqueue(
            record.id,
            &name,
            &target,
            args.message.as_deref(),
            args.dry_run,
        )?;
    }
    let mut out = Output::default();
    let ok = process(&mut ctx, args, &mut out)?;
    let code = if ok {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    };
    Ok((out, code))
}

/// `graft queue`.
pub(crate) fn queue(args: &QueueArgs, env: &Environment) -> anyhow::Result<Output> {
    let ctx = open(env)?;
    let all = ctx.located.store.integrations(args.limit)?;
    if all.is_empty() {
        return Ok(Output::stdout("The integrate queue is empty.\n".to_owned()));
    }
    let mut s = format!(
        "{:>4}  {:<9} {:<20} {:<20} {:<12} WHEN\n",
        "ID", "STATE", "GRAFT", "TARGET", "COMMIT"
    );
    for i in &all {
        let state = if i.dry_run && i.state == IntegrationState::Queued {
            "queued*"
        } else {
            i.state.as_str()
        };
        let commit = i.commit.as_deref().map_or("", |c| &c[..12]);
        let _ = writeln!(
            s,
            "{:>4}  {state:<9} {:<20} {:<20} {commit:<12} {}",
            i.id, i.graft_name, i.target, i.updated_at
        );
        let stopped = matches!(
            i.state,
            IntegrationState::Conflict | IntegrationState::Failed
        );
        if stopped || (args.verbose && !i.output.is_empty()) {
            let lines: Vec<&str> = i.output.lines().collect();
            let shown = if args.verbose {
                &lines[..]
            } else {
                &lines[..lines.len().min(3)]
            };
            for l in shown {
                let _ = writeln!(s, "        {l}");
            }
            if !args.verbose && lines.len() > shown.len() {
                let _ = writeln!(
                    s,
                    "        ... (graft queue --verbose for all {} lines)",
                    lines.len()
                );
            }
        }
    }
    Ok(Output::stdout(s))
}

/// The files `range` changed in Root (paths relative to the top level).
fn changed_in_root(root: &Path, from: &CommitId, to: &CommitId) -> anyhow::Result<Vec<PathBuf>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            from.as_str(),
            to.as_str(),
        ])
        .output()
        .context("running git diff")?;
    if !out.status.success() {
        bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| graft_core::path_from_bytes(p.to_vec()))
        .collect())
}

/// `graft rebase`: moves a reflink graft onto the target's head.
pub(crate) fn rebase(args: &RebaseArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let mut ctx = open(env)?;
    let record = find(&ctx.located.store, &name)?;
    if record.state != GraftState::Live {
        bail!(
            "graft {name} is {}; only live grafts can be rebased",
            record.state
        );
    }
    if record.backend.is_virtual() {
        bail!("graft {name} is virtual; rebasing virtual grafts is not supported yet (ADR 0017)");
    }
    let target = target_branch(&ctx, args.onto.as_deref())?;
    let head = branch_head(&ctx, &target)?;
    if head == record.base_commit {
        return Ok(Output::stdout(format!(
            "graft {name} is already based on {target} ({})\n",
            head.short(7)
        )));
    }
    let mut out = Output::default();
    let dir = ctx.backend.path(&name);
    let diffs = changes(&ctx, &record, &mut out)?;
    let touched: std::collections::BTreeSet<&PathBuf> =
        diffs.iter().map(|d| &d.change.path).collect();
    // First bring every file Root changed and the graft did not to the new
    // head, then merge the graft's own changes over the new head's files.
    let mut from_head = Vec::new();
    for path in changed_in_root(&ctx.located.record.path, &record.base_commit, &head)? {
        if touched.contains(&path) {
            continue;
        }
        if ctx.located.repo.tree_has(&head, &path)? {
            from_head.push(path);
        } else {
            write_entry(&dir.join(&path), None, false)?;
        }
    }
    // git checks them out with their modes, symlinks and filters.
    for chunk in from_head.chunks(200) {
        let mut cmd = vec!["checkout".to_owned(), head.to_string(), "--".to_owned()];
        cmd.extend(chunk.iter().map(|p| p.display().to_string()));
        let args: Vec<&str> = cmd.iter().map(String::as_str).collect();
        git(&dir, &args)?;
    }
    let scratch = ctx.root_dir().join("integrate");
    std::fs::create_dir_all(&scratch)?;
    let merger = Merger::detect();
    let mut conflicts = Vec::new();
    for d in &diffs {
        let ours = ctx.located.repo.read_file_at(&head, &d.change.path)?;
        let result = merger.merge(
            &d.change.path,
            d.old.as_deref(),
            ours.as_deref(),
            d.new.as_deref(),
            &scratch,
        )?;
        let target_path = dir.join(&d.change.path);
        match result {
            Merged::Clean {
                contents: Some(b), ..
            } => std::fs::write(&target_path, b)?,
            Merged::Clean { contents: None, .. } => {
                let _ = std::fs::remove_file(&target_path);
            }
            Merged::Conflict { markers, reason } => {
                if let Some(m) = markers {
                    std::fs::write(&target_path, m)?;
                }
                conflicts.push(format!("{}: {reason}", d.change.path.display()));
            }
        }
    }
    // Move the graft's git state and record to the new base.
    git(&dir, &["reset", "-q", "--mixed", head.as_str()])?;
    ctx.located.repo.pin(record.id, &head, &name)?;
    ctx.located.store.set_base(record.id, &head)?;
    let _ = writeln!(
        out.stdout,
        "Rebased graft {name} onto {target} ({})",
        head.short(7)
    );
    if !conflicts.is_empty() {
        let _ = writeln!(
            out.stdout,
            "Conflicts to resolve in the graft (marked with <<<<<<<):"
        );
        for c in conflicts {
            let _ = writeln!(out.stdout, "  {c}");
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_leftover_markers() {
        assert!(has_markers(
            b"a\n<<<<<<< Root\nb\n=======\nc\n>>>>>>> graft\n"
        ));
        assert!(!has_markers(b"a\n<<<< not a marker\n"));
    }
}
