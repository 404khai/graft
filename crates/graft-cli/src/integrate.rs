//! `graft integrate`, `graft queue` and `graft rebase` (ADR 0017).
//!
//! Integration happens in a scratch reflink checkout of the target's head
//! under `$GRAFT_HOME/roots/<root-id>/integrate/`: the graft's changes are
//! merged in file by file, conflicts go to the resolver hook, the test gate
//! runs confined, and only then is a commit made and one ref of Root moved
//! with compare-and-swap. Root's working tree is never written, unless the
//! operator passes `--update-checkout` for the branch Root has checked out.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, bail};
use graft_blueprint::{BLUEPRINT_FILE, IntegrateMode, IntegrateSpec, parse_integrate};
use graft_core::{CommitId, GraftId, GraftName, GraftState};
use graft_fs::{FsBackend, GraftSpec, ReflinkBackend};
use graft_integrate::{Merged, Merger, Method};
use graft_root::Repo;
use graft_root::graft::{Change, ChangeKind, FileDiff, GraftRepo};
use graft_store::{GraftRecord, Integration, IntegrationState};

use crate::grafts::{Ctx, find, open, parse_name};
use crate::timing::time;
use crate::{Environment, Output};

/// Arguments for `graft integrate`.
#[derive(Debug, clap::Args)]
#[allow(clippy::struct_excessive_bools)] // independent command-line flags
pub(crate) struct IntegrateArgs {
    /// The grafts to integrate, in order.
    #[arg(required = true)]
    names: Vec<String>,
    /// The branch to land on. Defaults to `[integrate] branch` in
    /// graft.toml, then the branch Root has checked out.
    #[arg(long, value_name = "BRANCH")]
    onto: Option<String>,
    /// Create the target branch at Root's HEAD if it does not exist.
    #[arg(long)]
    create_branch: bool,
    /// A test gate for this call, run like `[integrate] test` (which it
    /// replaces). Given on the command line, it needs no `graft trust`.
    #[arg(long, value_name = "COMMAND")]
    test: Option<String>,
    /// Message for squash or the final uncommitted-changes commit.
    #[arg(long, short)]
    message: Option<String>,
    /// Squash the graft into one commit instead of preserving its history.
    #[arg(long)]
    squash: bool,
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
    /// Print JSON: `{"results": [{graft, target, state, code, commit, commits, commit_count,
    /// output}], "gated": bool, "radar": [overlaps involving them]}`.
    #[arg(long)]
    json: bool,
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
pub(crate) fn git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
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

/// Where a request starts.
struct Start {
    /// The target's head, or Root's HEAD for a branch to create.
    head: CommitId,
    /// The target does not exist yet; landing creates it.
    new_branch: bool,
}

/// The [`Start`] on `branch`; a missing one is an error unless `create`.
fn start_of(ctx: &Ctx, branch: &str, create: bool) -> anyhow::Result<Start> {
    match branch_head(ctx, branch) {
        Ok(head) => Ok(Start {
            head,
            new_branch: false,
        }),
        Err(_) if create => Ok(Start {
            head: ctx.located.repo.head_commit()?,
            new_branch: true,
        }),
        Err(e) => Err(e.context("pass --create-branch to create it")),
    }
}

/// A graft's changes against its base, with contents.
pub(crate) fn changes(
    ctx: &Ctx,
    record: &GraftRecord,
    out: &mut Output,
) -> anyhow::Result<Vec<FileDiff>> {
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
pub(crate) fn graft_mode(ctx: &Ctx, record: &GraftRecord, path: &Path) -> Option<u32> {
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

fn log_structural(applied: &Applied, log: &mut String) {
    if !applied.structural.is_empty() {
        let paths: Vec<_> = applied
            .structural
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        let _ = writeln!(log, "structural merge (Mergiraf): {}", paths.join(", "));
    }
}

/// Merges `diffs` into `dir`, whose files are "ours". Conflicting files
/// get conflict markers where both sides have them.
fn apply(dir: &Path, diffs: &[FileDiff], merger: &Merger) -> anyhow::Result<Applied> {
    use std::os::unix::fs::PermissionsExt as _;
    let mut applied = Applied::default();
    for d in diffs {
        let target = dir.join(&d.change.path);
        let (ours, ours_link) = read_entry(&target)?.map_or((None, false), |(b, l)| (Some(b), l));
        let theirs_link = d.mode == 0o120_000;
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
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(d.mode & 0o777))?;
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
pub(crate) fn read_entry(path: &Path) -> anyhow::Result<Option<(Vec<u8>, bool)>> {
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
pub(crate) fn write_entry(path: &Path, contents: Option<&[u8]>, link: bool) -> anyhow::Result<()> {
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
pub(crate) fn graft_is_link(ctx: &Ctx, record: &GraftRecord, path: &Path) -> bool {
    std::fs::symlink_metadata(ctx.backend.path(&record.name).join(path))
        .is_ok_and(|m| m.file_type().is_symlink())
}

/// Admits every change of `record` against other live grafts' claims
/// (ADR 0018); the refusals, if any. Reflink grafts never pass writes
/// through Graft, so this is where their claims are enforced.
fn claims_refuse(
    ctx: &mut Ctx,
    record: &GraftRecord,
    diffs: &[FileDiff],
) -> anyhow::Result<Option<String>> {
    let claims = crate::plan::claims_for_admission(&ctx.located.store, None)?;
    if claims.iter().all(|c| c.graft == record.name.as_str()) {
        return Ok(None);
    }
    let mine = crate::plan::plan_of(&claims, record.name.as_str());
    let mut refused = String::new();
    for d in diffs {
        let changed = graft_radar::symbols::changed_symbols(
            &d.change.path,
            d.old.as_deref(),
            d.new.as_deref(),
        );
        let verdict = graft_sched::admission::admit(
            record.name.as_str(),
            mine.as_ref(),
            &d.change.path,
            changed.as_ref(),
            &claims,
        );
        if verdict != graft_sched::admission::Verdict::Allowed {
            let text = verdict.describe();
            let path = d.change.path.display().to_string();
            ctx.located
                .store
                .record_admission(record.name.as_str(), &path, &text)?;
            let _ = writeln!(refused, "{path}: {text}");
        }
    }
    Ok((!refused.is_empty()).then_some(refused))
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
        git(&dir, &["log", "--reverse", "--format=%s (%h)", &range])
            .ok()
            .map(|s| format!("chore: integrate graft {}\n\n{s}", record.name))
            .filter(|s| !s.is_empty())
    });
    let subject = subject.unwrap_or_else(|| format!("chore: integrate graft {}", record.name));
    format!("{subject}\n\nGraft: {} {}\n", record.name, record.id)
}

/// Commits made in the graft since its base, in dependency order.
fn original_commits(ctx: &Ctx, record: &GraftRecord) -> anyhow::Result<Vec<String>> {
    let source = ctx.backend.path(&record.name);
    if record.backend.is_virtual()
        && !crate::virtual_grafts::has_git(&ctx.located, &ctx.root_dir(), &source, record)
    {
        return Ok(Vec::new());
    }
    let range = format!("{}..HEAD", record.base_commit);
    Ok(
        git(&source, &["rev-list", "--reverse", "--topo-order", &range])?
            .lines()
            .map(str::to_owned)
            .collect(),
    )
}

/// Creates the optional final commit and the message file used by publish.
fn commit_pending(
    ctx: &Ctx,
    record: &GraftRecord,
    entry: &Integration,
    dir: &Path,
    needed: bool,
) -> anyhow::Result<(PathBuf, Option<String>)> {
    let message = dir.join(".git").join("GRAFT_INTEGRATE_MSG");
    std::fs::write(&message, message_for(ctx, record, entry.message.as_deref()))?;
    let commit = if needed {
        time("integrate: commit", || git(dir, &["add", "-A"]))?;
        git(
            dir,
            &[
                "commit",
                "--allow-empty",
                "-q",
                "-F",
                &message.display().to_string(),
            ],
        )?;
        Some(git(dir, &["rev-parse", "HEAD"])?)
    } else {
        None
    };
    Ok((message, commit))
}

/// The changes introduced by one commit, relative to its first parent.
fn commit_diffs(dir: &Path, parent: &str, commit: &str) -> anyhow::Result<Vec<FileDiff>> {
    let names = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["diff", "--no-renames", "--name-only", "-z", parent, commit])
        .output()?;
    anyhow::ensure!(names.status.success(), "listing commit changes failed");
    let mut diffs = Vec::new();
    for name in names.stdout.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(name));
        let read = |rev: &str| -> anyhow::Result<Option<Vec<u8>>> {
            let mut object = std::ffi::OsString::from(format!("{rev}:"));
            object.push(&path);
            let result = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["cat-file", "--filters"])
                .arg(object)
                .output()?;
            if !result.status.success() {
                let entry = Command::new("git")
                    .arg("-C")
                    .arg(dir)
                    .args(["ls-tree", rev, "--"])
                    .arg(&path)
                    .output()?;
                anyhow::ensure!(
                    entry.status.success() && entry.stdout.is_empty(),
                    "reading commit file: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
            }
            Ok(result.status.success().then_some(result.stdout))
        };
        let old = read(parent)?;
        let new = read(commit)?;
        let mode_rev = if new.is_some() { commit } else { parent };
        let mode = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["ls-tree", mode_rev, "--"])
            .arg(&path)
            .output()?;
        anyhow::ensure!(mode.status.success(), "reading commit file mode failed");
        let mode = String::from_utf8_lossy(&mode.stdout);
        let mode = u32::from_str_radix(
            mode.split_whitespace()
                .next()
                .context("missing file mode")?,
            8,
        )?;
        let kind = if old.is_none() {
            ChangeKind::Added
        } else if new.is_none() {
            ChangeKind::Deleted
        } else {
            ChangeKind::Modified
        };
        diffs.push(FileDiff {
            change: Change { path, kind },
            mode,
            old,
            new,
        });
    }
    Ok(diffs)
}

/// A landing or dry-run summary.
fn describe_commits(commits: &[String], squash_count: Option<usize>, dry_run: bool) -> String {
    let (verb, count) = squash_count.map_or_else(
        || {
            (
                if dry_run { "would land" } else { "landing" },
                commits.len(),
            )
        },
        |count| (if dry_run { "would squash" } else { "squashed" }, count),
    );
    format!("{verb} {count} commits: {}\n", commits.join(" "))
}

/// The outcome of one request.
struct Outcome {
    state: IntegrationState,
    output: String,
    commit: Option<String>,
    commits: Vec<String>,
    /// It passed with no test gate configured.
    ungated: bool,
    /// Why it stopped, for agents (ADR 0025); `None` derives one from
    /// `state`.
    code: Option<&'static str>,
}

impl Outcome {
    fn summary(&self) -> String {
        let mut summary = format!(
            "{} {} {}: {}\n",
            if self.state == IntegrationState::Checked {
                "would land"
            } else {
                "landed"
            },
            self.commits.len(),
            if self.commits.len() == 1 {
                "commit"
            } else {
                "commits"
            },
            self.commits.join(" ")
        );
        if let Some(line) = self
            .output
            .lines()
            .find(|line| line.starts_with("squashed ") || line.starts_with("would squash "))
        {
            let _ = writeln!(summary, "{line}");
        }
        summary
    }

    fn stop(state: IntegrationState, output: impl Into<String>) -> Self {
        Self {
            state,
            output: output.into(),
            commit: None,
            commits: Vec::new(),
            ungated: false,
            code: None,
        }
    }

    fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }

    /// The code an agent sees for this outcome.
    fn code(&self) -> &'static str {
        self.code.unwrap_or(match self.state {
            IntegrationState::Conflict => "conflict",
            IntegrationState::Landed => "landed",
            IntegrationState::Checked => "checked",
            _ => "failed",
        })
    }

    fn passed(
        state: IntegrationState,
        output: String,
        commit: Option<String>,
        spec: &IntegrateSpec,
    ) -> Self {
        Self {
            state,
            output,
            commit,
            commits: Vec::new(),
            ungated: spec.test.is_none(),
            code: None,
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
        Some(s) => Some(
            Outcome::stop(
                IntegrationState::Failed,
                format!("{log}test gate `{test}` failed ({s}):\n{tail}"),
            )
            .with_code("gate_failed"),
        ),
        None => Some(
            Outcome::stop(
                IntegrationState::Failed,
                format!(
                    "{log}test gate `{test}` timed out after {}s:\n{tail}",
                    spec.timeout_secs
                ),
            )
            .with_code("gate_failed"),
        ),
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

/// Checks before merging: approvals (not of the gate when the caller
/// gave it), something to integrate, claims. The graft's changes, or the
/// outcome that stops the entry.
fn preflight(
    ctx: &mut Ctx,
    record: &GraftRecord,
    spec: &IntegrateSpec,
    caller_gate: bool,
    out: &mut Output,
) -> anyhow::Result<Result<Vec<FileDiff>, Outcome>> {
    let checked = IntegrateSpec {
        test: if caller_gate { None } else { spec.test.clone() },
        ..spec.clone()
    };
    for (hash, what) in approvals(&checked) {
        if !ctx.located.store.is_trusted(&hash)? {
            return Ok(Err(Outcome::stop(
                IntegrationState::Failed,
                format!("{what} is not approved; review it and run `graft trust`"),
            )
            .with_code("not_approved")));
        }
    }
    let diffs = changes(ctx, record, out)?;
    let has_commits = !original_commits(ctx, record)?.is_empty();
    if diffs.is_empty() && !has_commits {
        return Ok(Err(Outcome::stop(
            IntegrationState::Failed,
            "nothing to integrate: no changes since base",
        )
        .with_code("nothing_to_integrate")));
    }
    if let Some(refused) = claims_refuse(ctx, record, &diffs)? {
        return Ok(Err(
            Outcome::stop(IntegrationState::Failed, refused).with_code("claimed")
        ));
    }
    Ok(Ok(diffs))
}

/// A scratch checkout of a target's head under `integrate/`.
///
/// One checkout serves a whole queue run while nothing runs in it: after
/// a request lands without a resolver or test gate, the checkout is at
/// the commit it just landed, so the next request on the same target
/// reuses it instead of checking out a fresh tree (ADR 0021). Anything
/// that ran in it, or any doubt about its state, gets a fresh one.
struct Scratch {
    backend: ReflinkBackend,
    name: GraftName,
    id: GraftId,
    dir: PathBuf,
    /// The commit the checkout is at, with a clean tree.
    head: CommitId,
}

impl Scratch {
    fn create(ctx: &Ctx, head: &CommitId) -> anyhow::Result<Self> {
        let backend = ReflinkBackend::new(ctx.root_dir().join("integrate"));
        let id = GraftId::random()?;
        let name: GraftName = format!("land-{}", &id.to_string()[..12]).parse()?;
        let created = time("integrate: check out scratch tree", || {
            backend.create(
                &ctx.located.repo,
                &GraftSpec {
                    id,
                    name: &name,
                    base: head,
                },
            )
        })?;
        git(
            &created.path,
            &["config", "--local", "core.hooksPath", "/dev/null"],
        )?;
        Ok(Self {
            backend,
            name,
            id,
            dir: created.path,
            head: head.clone(),
        })
    }

    /// Whether the checkout is exactly `head`: HEAD matches and nothing
    /// is modified, untracked or ignored.
    fn is_exactly(&self, head: &CommitId) -> bool {
        self.head == *head
            && git(&self.dir, &["rev-parse", "HEAD"]).is_ok_and(|h| h == head.as_str())
            && git(
                &self.dir,
                &[
                    "status",
                    "--porcelain",
                    "--ignored=matching",
                    "--untracked-files=all",
                ],
            )
            .is_ok_and(|s| s.is_empty())
    }

    /// The kept checkout if it is exactly `head`, else a fresh one.
    fn for_head(ctx: &Ctx, kept: Option<Self>, head: &CommitId) -> anyhow::Result<Self> {
        match kept {
            Some(s) if s.is_exactly(head) => Ok(s),
            Some(s) => {
                s.remove()?;
                Self::create(ctx, head)
            }
            None => Self::create(ctx, head),
        }
    }

    fn remove(self) -> anyhow::Result<()> {
        let this = std::mem::ManuallyDrop::new(self);
        time("integrate: remove scratch tree", || {
            this.backend.prune(&this.name, this.id)
        })?;
        Ok(())
    }
}

impl Drop for Scratch {
    /// A scratch checkout left by an error is removed on the way out.
    fn drop(&mut self) {
        if let Err(e) = self.backend.prune(&self.name, self.id) {
            tracing::warn!(dir = %self.dir.display(), "could not remove scratch checkout: {e}");
        }
    }
}

/// Clones Root's cached layers into a scratch checkout, for a resolver or
/// test gate to use. Missing layers are not built.
fn link_layers(ctx: &Ctx, head: &CommitId, dir: &Path) -> anyhow::Result<()> {
    if let Ok(planned) = crate::layers::plan(
        &ctx.located.repo,
        &ctx.located.store,
        &ctx.layers,
        head,
        None,
        None,
        &[],
    ) {
        time("integrate: link layers", || {
            crate::layers::link_ready(&planned, &ctx.layers, dir)
        })?;
    }
    Ok(())
}

/// The `[integrate]` table at `head`, with this call's `--test` in place
/// of its gate for the grafts it names (then `true`). The replaced gate
/// never runs and needs no approval: a command the operator typed is not
/// repository input. The resolver still needs its approval.
fn call_spec(
    ctx: &Ctx,
    head: &CommitId,
    entry: &Integration,
    args: &IntegrateArgs,
) -> anyhow::Result<(IntegrateSpec, bool)> {
    let mut spec = spec_at(&ctx.located.repo, head)?;
    let caller = match &args.test {
        Some(test) if args.names.contains(&entry.graft_name) => {
            spec.test = Some(test.clone());
            true
        }
        _ => false,
    };
    Ok((spec, caller))
}

fn replay_parent(
    dir: &Path,
    source: &Path,
    original: &str,
    mapped: &HashMap<String, String>,
    head: &str,
) -> anyhow::Result<String> {
    let ancestry = git(source, &["rev-list", "--parents", "-n", "1", original])?;
    let mut parents = ancestry.split_whitespace().skip(1);
    let parent = parents.next().context("cannot replay a root commit")?;
    let onto = mapped.get(parent).map_or(head, String::as_str);
    git(dir, &["checkout", "--detach", onto])?;
    let others: Vec<_> = parents
        .filter_map(|p| mapped.get(p).map(String::as_str))
        .collect();
    if !others.is_empty() {
        let mut args = vec!["merge", "--no-ff", "--no-commit", "-s", "ours"];
        args.extend(others);
        git(dir, &args)?;
    }
    Ok(parent.to_owned())
}

/// Preserves the graft's history in scratch, reusing objects or replaying.
fn replay(
    ctx: &Ctx,
    record: &GraftRecord,
    scratch: &Scratch,
    originals: &[String],
    spec: &IntegrateSpec,
    args: &IntegrateArgs,
    log: &mut String,
) -> anyhow::Result<Result<(Vec<String>, bool), Outcome>> {
    let dir = &scratch.dir;
    let head = &scratch.head;
    let source = ctx.backend.path(&record.name);
    if originals.is_empty() || args.squash {
        return Ok(Ok((Vec::new(), false)));
    }
    git(
        dir,
        &["fetch", "--no-tags", &source.display().to_string(), "HEAD"],
    )?;
    if head == &record.base_commit {
        git(dir, &["checkout", "--detach", "FETCH_HEAD"])?;
        return Ok(Ok((originals.to_vec(), false)));
    }
    let merger = Merger::detect();
    let mut mapped = HashMap::new();
    let mut commits = Vec::new();
    let mut ran = false;
    for original in originals {
        let parent = replay_parent(dir, &source, original, &mapped, head.as_str())?;
        let part = commit_diffs(&source, &parent, original)?;
        let applied = apply(dir, &part, &merger)?;
        log_structural(&applied, log);
        if !applied.conflicts.is_empty() {
            if !ran {
                link_layers(ctx, head, dir)?;
                ran = true;
            }
            if let Some(stop) = resolve(ctx, record, dir, head, &part, &applied, spec, args, log)? {
                return Ok(Err(stop));
            }
        }
        git(dir, &["add", "-A"])?;
        git(
            dir,
            &[
                "commit",
                "--allow-empty",
                "--cleanup=verbatim",
                "-q",
                "-C",
                original,
            ],
        )?;
        let commit = git(dir, &["rev-parse", "HEAD"])?;
        mapped.insert(original.clone(), commit.clone());
        commits.push(commit);
    }
    Ok(Ok((commits, ran)))
}

/// Runs one queued request, testing its final tree before landing.
fn attempt(
    ctx: &mut Ctx,
    entry: &Integration,
    record: &GraftRecord,
    args: &IntegrateArgs,
    out: &mut Output,
    kept: &mut Option<Scratch>,
) -> anyhow::Result<Outcome> {
    let start = start_of(ctx, &entry.target, args.create_branch)?;
    let head = &start.head;
    let (spec, caller_gate) = call_spec(ctx, head, entry, args)?;
    let diffs = match preflight(ctx, record, &spec, caller_gate, out)? {
        Ok(d) => d,
        Err(stop) => return Ok(stop),
    };
    if let Some(stop) = target_refusal(ctx, entry, record, &spec, args)? {
        return Ok(stop);
    }
    let mut scratch = Scratch::for_head(ctx, kept.take(), head)?;
    // Whether a resolver or test gate ran in the checkout: then it is
    // never reused.
    let mut ran = false;
    let result = (|| -> anyhow::Result<Outcome> {
        let dir = scratch.dir.clone();
        let merger = Merger::detect();
        let source = ctx.backend.path(&record.name);
        let originals = original_commits(ctx, record)?;
        let mut log = String::new();
        let (mut commits, replay_ran) =
            match replay(ctx, record, &scratch, &originals, &spec, args, &mut log)? {
                Ok(done) => done,
                Err(stop) => return Ok(stop),
            };
        ran |= replay_ran;
        let pending = if args.squash || originals.is_empty() {
            diffs.clone()
        } else {
            GraftRepo::open(&source)?.diff_since(&git(&source, &["rev-parse", "HEAD"])?.parse()?)?
        };
        let applied = time("integrate: apply changes", || {
            apply(&dir, &pending, &merger)
        })?;
        log_structural(&applied, &mut log);
        if !applied.conflicts.is_empty() {
            if spec.resolver.is_some() && !ran {
                link_layers(ctx, head, &dir)?;
                ran = true;
            }
            if let Some(stop) = resolve(
                ctx, record, &dir, head, &diffs, &applied, &spec, args, &mut log,
            )? {
                return Ok(stop);
            }
        }
        if spec.test.is_some() && !ran {
            link_layers(ctx, head, &dir)?;
            ran = true;
        }
        if let Some(stop) = gate(&dir, &spec, &mut log)? {
            return Ok(stop);
        }
        let needed = args.squash
            || !pending.is_empty()
            || !git(&dir, &["status", "--porcelain"])?.is_empty();
        let (msg_path, extra) = commit_pending(ctx, record, entry, &dir, needed)?;
        if let Some(extra) = extra {
            commits.push(extra);
        }
        let new = git(&dir, &["rev-parse", "HEAD"])?;
        log.push_str(&describe_commits(
            &commits,
            args.squash.then_some(originals.len()),
            entry.dry_run,
        ));
        if entry.dry_run {
            let mut outcome = Outcome::passed(IntegrationState::Checked, log, None, &spec);
            outcome.commits = commits;
            return Ok(outcome);
        }
        time("integrate: land", || {
            land(ctx, &dir, &start, &new, &entry.target, record, &spec, args)
        })?;
        let _ = writeln!(
            log,
            "landed {} on {}",
            &new[..12],
            landed_ref(&spec, &entry.target, record)
        );
        if spec.mode == IntegrateMode::Push {
            publish(ctx, &spec, &entry.target, record, &msg_path, &mut log)?;
        }
        let mut outcome = Outcome::passed(IntegrationState::Landed, log, Some(new), &spec);
        outcome.commits = commits;
        Ok(outcome)
    })();
    // Reuse only a checkout that holds exactly what just landed on the
    // target, and only in commit mode, where that is the target's head.
    let reusable = !ran
        && spec.mode == IntegrateMode::Commit
        && matches!(&result, Ok(o) if o.state == IntegrationState::Landed);
    if reusable
        && let Ok(Outcome {
            commit: Some(c), ..
        }) = &result
    {
        scratch.head = c.parse()?;
        *kept = Some(scratch);
    } else {
        scratch.remove()?;
    }
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

/// Why landing on `target`, which Root has checked out, is refused: Root's
/// working tree would fall behind (no `--update-checkout`), or has
/// changes so it cannot be fast-forwarded. Checked before any work and
/// again when landing, so a dry run predicts it.
fn checkout_refusal(
    root: &Path,
    target: &str,
    args: &IntegrateArgs,
) -> anyhow::Result<Option<Outcome>> {
    if !args.update_checkout {
        let how = crate::agent::suggest(
            "land elsewhere with --onto, or pass --update-checkout to fast-forward Root",
            "land elsewhere with onto, or pass update_checkout: true to fast-forward Root",
        );
        return Ok(Some(
            Outcome::stop(
                IntegrationState::Failed,
                format!(
                    "{target} is checked out in Root, so its working tree would fall behind; {how}"
                ),
            )
            .with_code("checkout_would_fall_behind"),
        ));
    }
    // Untracked files are left alone by a fast-forward, so only tracked
    // changes stop it.
    let dirty = git(root, &["status", "--porcelain", "--untracked-files=no"])?;
    if !dirty.is_empty() {
        return Ok(Some(
            Outcome::stop(
                IntegrationState::Failed,
                format!(
                    "Root's working tree has changes, so it cannot be fast-forwarded:\n{dirty}"
                ),
            )
            .with_code("root_not_clean"),
        ));
    }
    Ok(None)
}

/// [`checkout_refusal`] when `entry` lands on the branch Root has checked
/// out.
fn target_refusal(
    ctx: &Ctx,
    entry: &Integration,
    record: &GraftRecord,
    spec: &IntegrateSpec,
    args: &IntegrateArgs,
) -> anyhow::Result<Option<Outcome>> {
    let checked_out = spec.mode == IntegrateMode::Commit
        && ctx.located.repo.head_ref()?.as_deref()
            == Some(landed_ref(spec, &entry.target, record).as_str());
    if !checked_out {
        return Ok(None);
    }
    checkout_refusal(&ctx.located.record.path, &entry.target, args)
}

/// Copies commit `new` into Root and moves the ref from `head` to it, or
/// creates it for a new branch (if it still does not exist).
#[allow(clippy::too_many_arguments)] // one call site
fn land(
    ctx: &Ctx,
    scratch: &Path,
    start: &Start,
    new: &str,
    target: &str,
    record: &GraftRecord,
    spec: &IntegrateSpec,
    args: &IntegrateArgs,
) -> anyhow::Result<()> {
    let (head, new_branch) = (&start.head, start.new_branch);
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
            // An all-zero old value makes update-ref refuse if the branch
            // appeared meanwhile.
            let zero = "0".repeat(head.as_str().len());
            let old = if new_branch {
                zero.as_str()
            } else {
                head.as_str()
            };
            return git(&root, &["update-ref", &refname, new, old]).map(drop);
        }
        if let Some(stop) = checkout_refusal(&root, target, args)? {
            bail!(stop.output);
        }
        if git(&root, &["rev-parse", "HEAD"])? != head.as_str() {
            bail!("{target} moved while integrating");
        }
        git(&root, &["merge", "--ff-only", "-q", new]).map(drop)
    })();
    let _ = git(&root, &["update-ref", "-d", &staging]);
    moved
}

/// The end of a queue run: the JSON or the no-gate note, and under
/// `graft mcp` the error line when a request did not land.
fn finish(
    args: &IntegrateArgs,
    out: &mut Output,
    results: &[serde_json::Value],
    ungated: bool,
    first_failure: Option<&'static str>,
    overlaps: &[serde_json::Value],
) {
    if args.json {
        out.stdout = serde_json::json!({"results": results, "gated": !ungated, "radar": overlaps})
            .to_string()
            + "\n";
    } else if ungated {
        out.stdout.push_str(
            "note: no test gate ran: set [integrate] test in graft.toml to test before landing\n",
        );
    }
    if let Some(code) = first_failure {
        let failed = results
            .iter()
            .filter(|r| r["code"] != "landed" && r["code"] != "checked")
            .count();
        crate::agent::report_failure(
            code,
            &format!(
                "{failed} of {} requests did not land; see output",
                results.len()
            ),
            None,
        );
    }
}

/// Processes the queue in order, holding `integrate.lock`.
fn process(
    ctx: &mut Ctx,
    args: &IntegrateArgs,
    out: &mut Output,
    overlaps: &[serde_json::Value],
) -> anyhow::Result<bool> {
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
    let mut ungated = false;
    let mut kept: Option<Scratch> = None;
    let mut results = Vec::new();
    let mut first_failure: Option<&'static str> = None;
    while let Some(entry) = ctx.located.store.next_queued()? {
        ctx.located
            .store
            .set_integration(entry.id, IntegrationState::Running, "", None)?;
        let record = ctx.located.store.graft_by_id(entry.graft_id)?;
        let outcome = match record {
            Some(r) if r.state == GraftState::Live => {
                let mut o = attempt(ctx, &entry, &r, args, out, &mut kept);
                // The target moved under us: retry once from its new head.
                if matches!(&o, Err(e) if format!("{e:#}").contains("update-ref")) {
                    o = attempt(ctx, &entry, &r, args, out, &mut kept);
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
        ungated |= outcome.ungated;
        results.push(serde_json::json!({
            "graft": entry.graft_name.as_str(),
            "target": entry.target,
            "state": outcome.state.as_str(),
            "code": outcome.code(),
            "commit": outcome.commit,
            "commits": outcome.commits,
            "commit_count": outcome.commits.len(),
            "output": outcome.output.trim_end(),
        }));
        let short = outcome.commit.as_deref().map_or("", |c| &c[..12]);
        let _ = writeln!(
            out.stdout,
            "{:<8} {} → {} {short}",
            outcome.state.as_str(),
            entry.graft_name,
            entry.target
        );
        out.stdout.push_str(&outcome.summary());
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
            first_failure.get_or_insert(outcome.code());
            for line in outcome.output.lines() {
                let _ = writeln!(out.stdout, "    {line}");
            }
        }
    }
    if let Some(s) = kept {
        s.remove()?;
    }
    finish(args, out, &results, ungated, first_failure, overlaps);
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
    start_of(&ctx, &target, args.create_branch)?;
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
    // Flag overlapping live grafts before anything lands (Radar, ADR 0016).
    let (overlaps, lines) = crate::radar::overlaps_involving(&mut ctx, &args.names, &mut out)?;
    out.warnings.extend(lines);
    let ok = process(&mut ctx, args, &mut out, &overlaps)?;
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

/// Each file `diffs` changed, merged against `head`, as it should be after
/// a rebase; and the conflicts, for people.
fn rebased_files(
    ctx: &Ctx,
    record: &GraftRecord,
    head: &CommitId,
    diffs: &[FileDiff],
) -> anyhow::Result<(Vec<graft_fs::RebasedFile>, Vec<String>)> {
    use graft_store::delta::Entry;
    let delta = ctx.located.store.delta(record.id)?;
    let scratch = ctx.root_dir().join("integrate");
    std::fs::create_dir_all(&scratch)?;
    let merger = Merger::detect();
    let mut files = Vec::new();
    let mut conflicts = Vec::new();
    for d in diffs {
        let path = graft_core::path_to_bytes(&d.change.path);
        let ours = ctx.located.repo.read_file_at(head, &d.change.path)?;
        let (link, mode) = match delta.get(&path) {
            Some(Entry::Symlink { .. }) => (true, 0),
            Some(Entry::File { mode, .. }) => (false, *mode),
            _ => (false, 0o644),
        };
        let contents = if link {
            if ours.as_deref() != d.old.as_deref() && ours.as_deref() != d.new.as_deref() {
                conflicts.push(format!(
                    "{}: a symlink changed in both Root and the graft (the graft's is kept)",
                    d.change.path.display()
                ));
            }
            d.new.clone()
        } else {
            match merger.merge(
                &d.change.path,
                d.old.as_deref(),
                ours.as_deref(),
                d.new.as_deref(),
                &scratch,
            )? {
                Merged::Clean { contents, .. } => contents,
                Merged::Conflict { markers, reason } => {
                    conflicts.push(format!("{}: {reason}", d.change.path.display()));
                    markers.or_else(|| d.new.clone())
                }
            }
        };
        files.push(graft_fs::RebasedFile {
            path,
            contents,
            link,
            mode,
        });
    }
    Ok((files, conflicts))
}

/// `graft rebase` of a virtual graft: its changes are merged against the
/// new head and journaled as a new op log epoch over it (ADR 0017).
fn rebase_virtual(
    ctx: &mut Ctx,
    record: &GraftRecord,
    target: &str,
    head: &CommitId,
) -> anyhow::Result<Output> {
    let name = &record.name;
    let mut out = Output::default();
    let mut client = crate::grafts::daemon(&ctx.home, &mut out)?;
    let root_id = ctx.located.record.id;
    let mounted = client
        .mounts()?
        .iter()
        .any(|m| m.root == root_id.to_string() && m.graft == name.as_str());
    if mounted {
        client.sync(&root_id, name)?;
    }
    let diffs = changes(ctx, record, &mut out)?;
    let (files, conflicts) = rebased_files(ctx, record, head, &diffs)?;
    // The old view must go before the delta changes under it.
    let unmounted = client.unmount(&root_id, name)?;
    if unmounted.lazy {
        let remounted = client.mount(&root_id, name);
        bail!(
            "graft {name} is in use (files are open in it), so it was not rebased{}; close them and retry",
            remounted
                .err()
                .map(|e| format!(" and could not be remounted: {e}"))
                .unwrap_or_default()
        );
    }
    let result = (|| -> anyhow::Result<()> {
        let objects = graft_root::tree::ObjectStore::open(&ctx.located.repo);
        let layers = graft_fs::layer_mounts(&ctx.located.store, &ctx.root_dir(), record.id)?;
        let blobs = graft_store::blobs::BlobStore::new(ctx.root_dir().join("blobs"));
        let now = graft_core::Timestamp::now();
        let ops = graft_fs::rebase_ops(&objects, head, layers, &files, &blobs, now)?;
        ctx.located.store.rebase(record.id, head, now, &ops)?;
        ctx.located.repo.pin(record.id, head, name)?;
        Ok(())
    })();
    let mount = client.mount(&root_id, name);
    result?;
    let mount = mount.with_context(|| format!("remounting graft {name} after the rebase"))?;
    // The graft's .git (if it has one yet) moves to the new head too.
    if crate::virtual_grafts::has_git(&ctx.located, &ctx.root_dir(), &mount, record) {
        git(&mount, &["reset", "-q", "--mixed", head.as_str()])?;
    } else if ctx.located.repo.registration(name, &mount).is_some() {
        // Registered, but its index is not built yet: it will be built
        // from the new base, so its branch must be there too (ADR 0036).
        ctx.located.repo.move_live_branch(name, head)?;
    }
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

/// `graft rebase`: moves a graft onto the target's head.
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
    let target = target_branch(&ctx, args.onto.as_deref())?;
    let head = branch_head(&ctx, &target)?;
    if head == record.base_commit {
        return Ok(Output::stdout(format!(
            "graft {name} is already based on {target} ({})\n",
            head.short(7)
        )));
    }
    if record.backend.is_virtual() {
        return rebase_virtual(&mut ctx, &record, &target, &head);
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
