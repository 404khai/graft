//! `graft new`, `ls`, `path`, `status`, `diff` and `prune`.
//!
//! These orchestrate the store (records), the Root (pin refs) and the
//! filesystem backend (working trees). Virtual (FUSE) grafts are mounted
//! by `graftd`, which these commands start on demand (ADR 0007).

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, bail};
use graft_core::{BackendKind, GraftHome, GraftName, GraftState};
use graft_daemon::Client;
use graft_fs::{FsBackend, GraftSpec, ReflinkBackend, human_bytes};
use graft_root::graft::{GraftRepo, unified_diff};
use graft_root::layer::LayerCache;
use graft_root::worktree::{BranchOutcome, BranchPolicy, Layout};
use graft_store::registry::{Located, locate};
use graft_store::{GraftRecord, Store};

use crate::{Environment, Output};
use crate::{layers, timing};

/// Arguments for `graft new`.
#[derive(Debug, clap::Args)]
pub(crate) struct NewArgs {
    /// Name of the graft: lowercase letters, digits and hyphens.
    name: String,
    /// Revision to start from (branch, tag or commit). Defaults to Root's
    /// HEAD.
    #[arg(long, value_name = "REF")]
    from: Option<String>,
    /// Filesystem backend: `reflink` (a real working tree), or a virtual
    /// tree served by graftd: `fuse` on Linux, `nfs` on macOS. Defaults to
    /// `$GRAFT_FS`, then `fs.backend` in `config.toml`, then `reflink`.
    #[arg(long, value_name = "BACKEND")]
    backend: Option<BackendKind>,
    /// Provision this blueprint of graft.toml: ports, services, env files
    /// and setup commands (see docs/blueprint.md).
    #[arg(long, value_name = "NAME")]
    blueprint: Option<String>,
    /// Show the full output of layer builds as they run (by default only
    /// a failed build's output is shown).
    #[arg(long, short)]
    verbose: bool,
    /// Reflink these git-ignored directories from Root's working tree into
    /// the graft (e.g. `target,web/node_modules`), with no build: a graft
    /// starts with Root's build outputs and dependencies as they are.
    /// Reflink grafts only.
    #[arg(long, value_name = "PATHS", value_delimiter = ',')]
    clone_ignored: Vec<String>,
    /// Then run this command in the new graft, confined like `graft run`
    /// (e.g. `graft new auth -- claude`). Its exit code is graft's.
    #[arg(last = true, value_name = "COMMAND")]
    command: Vec<std::ffi::OsString>,
    /// Print JSON: `{name, path, base, backend, layers: [{path, status,
    /// key}], cloned: [{path, reflinked, copied}]}`.
    #[arg(long)]
    json: bool,
}

impl NewArgs {
    /// The graft's name as given.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The command to start in the new graft, if any.
    pub(crate) fn command(&self) -> &[std::ffi::OsString] {
        &self.command
    }

    fn build_output(&self) -> layers::BuildOutput {
        if self.verbose {
            layers::BuildOutput::Stream
        } else {
            layers::BuildOutput::Quiet
        }
    }

    /// `graft new <name> [--backend B]` at Root's HEAD, for commands that
    /// create grafts themselves (`graft plan --apply`).
    pub(crate) fn named(name: String, backend: Option<BackendKind>) -> Self {
        Self {
            name,
            from: None,
            backend,
            blueprint: None,
            verbose: false,
            clone_ignored: Vec::new(),
            command: Vec::new(),
            json: false,
        }
    }
}

/// The `--blueprint` of `graft new`, checked for approval before anything
/// is created.
fn blueprint_for(
    args: &NewArgs,
    located: &Located,
    base: &graft_core::CommitId,
) -> anyhow::Result<Option<graft_blueprint::BlueprintSpec>> {
    let Some(name) = &args.blueprint else {
        return Ok(None);
    };
    let spec = crate::blueprints::spec_at(&located.repo, base, name)?;
    crate::blueprints::check_approved(&located.store, &spec, args.from.as_deref())?;
    Ok(Some(spec))
}

/// Arguments for commands that take one graft name.
#[derive(Debug, clap::Args)]
pub(crate) struct NameArgs {
    /// The graft's name.
    name: String,
}

/// Arguments for `graft path`.
#[derive(Debug, clap::Args)]
pub(crate) struct PathArgs {
    /// The graft's name.
    name: String,
    /// Print JSON: `{name, path}`.
    #[arg(long)]
    json: bool,
}

/// Arguments for `graft status`.
#[derive(Debug, clap::Args)]
pub(crate) struct StatusArgs {
    /// The graft's name. Defaults to the graft containing the current
    /// directory; outside a graft, summarises every graft.
    name: Option<String>,
    /// Print JSON: a list of grafts, each with its changes.
    #[arg(long)]
    json: bool,
}

/// Arguments for `graft ls`.
#[derive(Debug, clap::Args)]
pub(crate) struct LsArgs {
    /// Print JSON: one object per graft (`changed` and `disk_bytes` are
    /// null when unknown, with the reason in `note`).
    #[arg(long)]
    json: bool,
    /// Skip measuring disk use, which walks every file (slow for a large
    /// build tree such as `target/`). `disk_bytes` is then null.
    #[arg(long)]
    no_disk: bool,
}

/// Arguments for `graft prune`.
#[derive(Debug, clap::Args)]
pub(crate) struct PruneArgs {
    /// The graft's name.
    name: String,
    /// Prune even if the graft has uncommitted changes or commits Root does
    /// not have. That work is lost.
    #[arg(long)]
    force: bool,
    /// Print JSON: `{name, pruned}`.
    #[arg(long)]
    json: bool,
    /// Delete the working tree before returning. By default it is moved
    /// aside at once and deleted in the background (`graft gc` finishes
    /// the job if that is interrupted).
    #[arg(long)]
    wait: bool,
}

impl PruneArgs {
    /// `graft prune <name> --force`, for commands that prune a graft whose
    /// work has landed elsewhere.
    pub(crate) fn forced(name: String) -> Self {
        Self {
            name,
            force: true,
            json: false,
            wait: false,
        }
    }
}

pub(crate) struct Ctx {
    pub(crate) home: GraftHome,
    pub(crate) located: Located,
    pub(crate) backend: ReflinkBackend,
    pub(crate) layers: LayerCache,
}

pub(crate) fn open(env: &Environment) -> anyhow::Result<Ctx> {
    let home = env.require_home()?;
    let located = locate(home, Path::new("."))?;
    let root_dir = home.root_dir(&located.record.id);
    Ok(Ctx {
        home: home.clone(),
        located,
        backend: ReflinkBackend::new(root_dir.join("mounts")),
        layers: LayerCache::new(root_dir.join("layers")),
    })
}

impl Ctx {
    /// `$GRAFT_HOME/roots/<root-id>`.
    pub(crate) fn root_dir(&self) -> std::path::PathBuf {
        self.home.root_dir(&self.located.record.id)
    }
}

pub(crate) fn parse_name(name: &str) -> anyhow::Result<GraftName> {
    Ok(name.parse()?)
}

pub(crate) fn find(store: &Store, name: &GraftName) -> anyhow::Result<GraftRecord> {
    store.graft(name)?.ok_or_else(|| {
        crate::agent::coded(
            "not_found",
            format!("no graft named {name}"),
            Some(format!(
                "see {}",
                crate::agent::suggest("`graft ls`", "graft_list")
            )),
        )
    })
}

fn short(record: &GraftRecord) -> &str {
    record.base_commit.short(7)
}

/// The backend for a new graft: `--backend`, `$GRAFT_FS`, config, default.
fn choose_backend(args: &NewArgs, env: &Environment) -> anyhow::Result<BackendKind> {
    if let Some(b) = args.backend {
        return Ok(b);
    }
    if let Some(v) = std::env::var_os(BackendKind::ENV).filter(|v| !v.is_empty()) {
        let v = v.to_string_lossy();
        return v.parse().with_context(|| format!("${}", BackendKind::ENV));
    }
    Ok(env
        .config
        .as_ref()
        .and_then(|c| c.as_ref().ok())
        .and_then(|c| c.fs.backend)
        .unwrap_or_default())
}

/// Whether new grafts are registered as git worktrees of Root (`[git]
/// worktrees` in `config.toml`, ADR 0036).
pub(crate) fn register_worktrees(env: &Environment) -> bool {
    env.config
        .as_ref()
        .and_then(|c| c.as_ref().ok())
        .is_some_and(|c| c.git.worktrees)
}

/// Registers the new graft `name` at `workdir` as a git worktree of Root
/// when `[git] worktrees` is on (ADR 0036). Returns whether it did, which
/// is what a failed create must undo with [`undo_register`].
pub(crate) fn register_new(
    env: &Environment,
    repo: &graft_root::Repo,
    name: &GraftName,
    workdir: &Path,
    base: &graft_core::CommitId,
    layout: Layout,
    layers: &[&Path],
) -> anyhow::Result<bool> {
    if !register_worktrees(env) {
        return Ok(false);
    }
    if let Some(parent) = workdir.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    timing::time("register worktree", || {
        repo.register(name, workdir, base, layout, layers)
    })?;
    Ok(true)
}

/// Undoes [`register_new`] for a create that failed afterwards. When
/// registering was refused (`registered` is false), nothing is touched:
/// the worktree or branch that stood in the way is not this graft's.
pub(crate) fn undo_register(
    repo: &graft_root::Repo,
    name: &GraftName,
    workdir: &Path,
    registered: bool,
) {
    if registered {
        let _ = repo.unregister(name, workdir, BranchPolicy::Delete);
    }
}

/// Removes graft `name`'s git worktree registration, if it has one, and
/// its `graft-live/<name>/live` branch. `checked` means the branch's work was
/// checked by the lost-work check, integrated, or discarded with
/// `--force`; otherwise a branch with commits on no other ref is kept.
pub(crate) fn unregister(
    ctx: &Ctx,
    name: &GraftName,
    checked: bool,
    out: &mut Output,
) -> anyhow::Result<()> {
    let policy = if checked {
        BranchPolicy::Delete
    } else {
        BranchPolicy::KeepIfUnique
    };
    let outcome = ctx
        .located
        .repo
        .unregister(name, &ctx.backend.path(name), policy)
        .with_context(|| format!("removing graft {name}'s git worktree registration"))?;
    if let BranchOutcome::Kept { reason } = outcome {
        out.warnings.push(format!(
            "kept branch {} in Root: {reason}",
            graft_root::worktree::live_branch(name)
        ));
    }
    Ok(())
}

/// Connects to graftd, starting it if needed. A graftd from another
/// version (left running across an upgrade) is restarted when it serves no
/// mounts, which loses nothing; otherwise this warns, since stopping it
/// would unmount grafts in use.
pub(crate) fn daemon(home: &GraftHome, out: &mut Output) -> anyhow::Result<Client> {
    let ours = env!("CARGO_PKG_VERSION");
    let mut client = Client::connect_or_start(home)?;
    let mut ping = client.ping()?;
    if ping.version != ours && client.mounts()?.is_empty() {
        let old = ping.version.clone();
        client.shutdown()?;
        drop(client);
        let _ = Client::wait_until_stopped(home, std::time::Duration::from_secs(10));
        client = Client::connect_or_start(home)?;
        ping = client.ping()?;
        if ping.version == ours {
            tracing::info!(
                old,
                new = ours,
                "restarted an idle graftd from another version"
            );
        }
    }
    if ping.version != ours {
        out.warnings.push(format!(
            "graftd {} is running but this is graft {ours}; stop it with `graft daemon stop` once no grafts are in use",
            ping.version
        ));
    }
    Ok(client)
}

/// Creates a graft.
pub(crate) fn new(args: &NewArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let kind = choose_backend(args, env)?;
    if kind.is_virtual() && !args.clone_ignored.is_empty() {
        return Err(crate::agent::coded(
            "backend_unsupported",
            format!("--clone-ignored needs a reflink graft, not {kind}"),
            Some("drop --clone-ignored, or declare the directory as a layer in graft.toml".into()),
        ));
    }
    let mut out = timing::time("graft new", || {
        if kind.is_virtual() {
            new_virtual(args, env, &name, kind)
        } else {
            new_reflink(args, env, &name)
        }
    })?;
    if let Some(bp) = &args.blueprint
        && let Err(e) = timing::time("provision blueprint", || {
            provision(env, &name, bp, kind, &mut out)
        })
    {
        // Release what was provisioned and remove the graft.
        let undo = prune(&PruneArgs::forced(name.to_string()), env);
        let note = match undo {
            Ok(_) => format!("graft {name} was removed again"),
            Err(u) => format!(
                "removing graft {name} again also failed: {u:#}; run `graft prune {name} --force`"
            ),
        };
        return Err(e.context(format!("provisioning blueprint {bp} ({note})")));
    }
    Ok(out)
}

fn provision(
    env: &Environment,
    name: &GraftName,
    blueprint: &str,
    kind: BackendKind,
    out: &mut Output,
) -> anyhow::Result<()> {
    let mut ctx = open(env)?;
    let record = find(&ctx.located.store, name)?;
    let spec = crate::blueprints::spec_at(&ctx.located.repo, &record.base_commit, blueprint)?;
    let graft_dir = if kind.is_virtual() {
        daemon(&ctx.home, out)?.mount(&ctx.located.record.id, name)?
    } else {
        ctx.backend.path(name)
    };
    crate::blueprints::provision(&mut ctx, &record, &spec, &graft_dir, out)?;
    let _ = writeln!(out.stdout, "  blueprint {blueprint}: provisioned");
    for (k, v) in crate::blueprints::environment(&ctx, &record, &graft_dir)? {
        let v = if crate::blueprints::is_secret(&k) {
            "<redacted>".to_owned()
        } else {
            v
        };
        let _ = writeln!(out.stdout, "    {k}={v}");
    }
    Ok(())
}

/// [`register_new`] for a reflink graft, which has no layers to exclude
/// yet.
fn register_reflink(
    env: &Environment,
    repo: &graft_root::Repo,
    name: &GraftName,
    path: &Path,
    base: &graft_core::CommitId,
) -> anyhow::Result<bool> {
    register_new(env, repo, name, path, base, Layout::Reflink, &[])
}

/// Creates a reflink graft: a real working tree (ADR 0003).
fn new_reflink(args: &NewArgs, env: &Environment, name: &GraftName) -> anyhow::Result<Output> {
    let Ctx {
        mut located,
        backend,
        layers: cache,
        ..
    } = open(env)?;
    let rev = args.from.as_deref().unwrap_or("HEAD");
    let base = located.repo.resolve_commit(rev)?;
    let blueprint = blueprint_for(args, &located, &base)?;
    // Refuse unapproved layer commands before anything is created.
    let planned = timing::time("plan layers", || {
        layers::plan(
            &located.repo,
            &located.store,
            &cache,
            &base,
            args.from.as_deref(),
            blueprint.as_ref().and_then(|b| b.layers.as_deref()),
        )
    })?;
    let seeds = check_clone_ignored(&located.record.path, &args.clone_ignored)?;

    // The record reserves the name before any files exist.
    let record = located
        .store
        .create_graft(name, &base, BackendKind::Reflink)?;
    let spec = GraftSpec {
        id: record.id,
        name,
        base: &base,
    };
    let mut registered = false;
    let created = timing::time("check out tree", || backend.create(&located.repo, &spec))
        .map_err(anyhow::Error::from)
        .and_then(|report| {
            timing::time("pin base", || located.repo.pin(record.id, &base, name))?;
            registered = register_reflink(env, &located.repo, name, &report.path, &base)?;
            let applied = layers::apply(
                &planned,
                &cache,
                &mut located.store,
                &report.path,
                name,
                record.id,
                true,
                args.build_output(),
            )?;
            let cloned = timing::time("clone ignored", || {
                clone_ignored(&located.record.path, &report.path, &seeds)
            })?;
            Ok((report, applied, cloned))
        });
    let (report, applied, cloned) = match created {
        Ok(created) => created,
        Err(e) => {
            let _ = backend.prune(name, record.id);
            undo_register(&located.repo, name, &backend.path(name), registered);
            let _ = located.repo.unpin(record.id);
            let _ = located.store.delete_graft(record.id);
            return Err(e.context(format!("creating graft {name}")));
        }
    };
    located.store.set_state(record.id, GraftState::Live)?;

    let mut out = Output::default();
    let c = &report.checkout;
    let _ = writeln!(
        out.stdout,
        "Created graft {name} at {} (base {})",
        report.path.display(),
        base.short(7)
    );
    let _ = writeln!(
        out.stdout,
        "  {} files reflinked, {} copied, {} written from git objects",
        report.clone.reflinked, report.clone.copied, c.written
    );
    report_layers(&applied, &mut out);
    report_cloned(&cloned, report.clone.fallback.is_some(), &mut out);
    if let Some(reason) = &report.clone.fallback {
        out.warnings.push(format!(
            "copied {} files instead of reflinking them: {reason}. This graft uses its full size on disk.",
            report.clone.copied
        ));
    }
    if !c.collisions.is_empty() {
        let shown: Vec<_> = c.collisions.iter().take(5).map(String::as_str).collect();
        out.warnings.push(format!(
            "{} paths collide on this case-insensitive filesystem and were not checked out: {}{}",
            c.collisions.len(),
            shown.join(", "),
            if c.collisions.len() > shown.len() {
                ", ..."
            } else {
                ""
            }
        ));
    }
    Ok(out.json_if(args.json, || {
        let mut v = created_json(name, &report.path, &base, BackendKind::Reflink, &applied);
        v["cloned"] = cloned_json(&cloned);
        v
    }))
}

/// A git-ignored directory `--clone-ignored` brought in from Root.
struct Cloned {
    path: std::path::PathBuf,
    reflinked: u64,
    copied: u64,
    fallback: Option<String>,
}

/// One line per cloned directory, and a warning for one that was copied
/// (unless the whole tree was, which has its own warning).
fn report_cloned(cloned: &[Cloned], tree_copied: bool, out: &mut Output) {
    for c in cloned {
        let _ = writeln!(
            out.stdout,
            "  {} cloned from Root: {} files reflinked, {} copied",
            c.path.display(),
            c.reflinked,
            c.copied
        );
        if !tree_copied && let Some(reason) = &c.fallback {
            out.warnings.push(format!(
                "copied {} instead of reflinking it: {reason}. It uses its full size on disk.",
                c.path.display()
            ));
        }
    }
}

fn cloned_json(cloned: &[Cloned]) -> serde_json::Value {
    cloned
        .iter()
        .map(|c| serde_json::json!({"path": c.path, "reflinked": c.reflinked, "copied": c.copied}))
        .collect()
}

/// Checks `--clone-ignored` before anything is created: each path is
/// relative with plain components, a real directory in Root's working
/// tree (not reached through a symlink), ignored by git there, and not
/// inside another one.
fn check_clone_ignored(root: &Path, raw: &[String]) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let mut paths = Vec::new();
    for r in raw {
        let p = graft_blueprint::safe_relative(r.trim_end_matches('/'))
            .map_err(|why| anyhow::anyhow!("--clone-ignored {r}: the path {why}"))?;
        paths.push(p);
    }
    if paths.is_empty() {
        return Ok(paths);
    }
    let top = root
        .canonicalize()
        .with_context(|| format!("resolving {}", root.display()))?;
    for p in &paths {
        let src = root.join(p);
        let meta = std::fs::symlink_metadata(&src).with_context(|| {
            format!(
                "--clone-ignored {}: not in Root's working tree",
                p.display()
            )
        })?;
        if !meta.is_dir() {
            bail!("--clone-ignored {}: not a directory in Root", p.display());
        }
        if src.canonicalize()? != top.join(p) {
            bail!(
                "--clone-ignored {}: reached through a symlink in Root",
                p.display()
            );
        }
        if let Some(outer) = paths.iter().find(|o| *o != p && p.starts_with(o)) {
            bail!(
                "--clone-ignored {}: inside {}, which is already cloned",
                p.display(),
                outer.display()
            );
        }
    }
    // A child probe makes every real component a directory for git's
    // matching, so dir-only patterns such as `/target/` apply.
    let probes: Vec<_> = paths.iter().map(|p| p.join(".graft-probe")).collect();
    let ignored = GraftRepo::open(root)?.ignored(&probes)?;
    for (p, probe) in paths.iter().zip(&probes) {
        if !ignored.contains(probe) {
            bail!(
                "--clone-ignored {}: not ignored by git in Root; its tracked files are in the graft already",
                p.display()
            );
        }
    }
    Ok(paths)
}

/// Reflinks each checked path from Root into the new graft at `graft`.
fn clone_ignored(
    root: &Path,
    graft: &Path,
    paths: &[std::path::PathBuf],
) -> anyhow::Result<Vec<Cloned>> {
    let mut out = Vec::new();
    for p in paths {
        let dst = graft.join(p);
        if std::fs::symlink_metadata(&dst).is_ok() {
            bail!(
                "--clone-ignored {}: already in the graft (a layer of graft.toml?)",
                p.display()
            );
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let cloner = graft_fs::ReflinkCloner::new();
        graft_fs::clone_tree(&root.join(p), &dst, &cloner)
            .with_context(|| format!("cloning {} from Root", p.display()))?;
        let r = cloner.report();
        out.push(Cloned {
            path: p.clone(),
            reflinked: r.reflinked,
            copied: r.copied,
            fallback: r.fallback,
        });
    }
    Ok(out)
}

/// Creates a FUSE graft: a record, a pin and layer references, then graftd
/// mounts it. No files are written, whatever the size of the repo.
fn new_virtual(
    args: &NewArgs,
    env: &Environment,
    name: &GraftName,
    kind: BackendKind,
) -> anyhow::Result<Output> {
    let Ctx {
        home,
        mut located,
        backend,
        layers: cache,
    } = open(env)?;
    let rev = args.from.as_deref().unwrap_or("HEAD");
    let base = located.repo.resolve_commit(rev)?;
    let blueprint = blueprint_for(args, &located, &base)?;
    let planned = timing::time("plan layers", || {
        layers::plan(
            &located.repo,
            &located.store,
            &cache,
            &base,
            args.from.as_deref(),
            blueprint.as_ref().and_then(|b| b.layers.as_deref()),
        )
    })?;
    let mut out = Output::default();
    // Reach graftd before recording anything, so an unsupported platform
    // leaves nothing behind.
    let was_running = Client::connect(&home).is_ok();
    let mut client = timing::time("reach graftd", || daemon(&home, &mut out))?;
    let record = located.store.create_graft(name, &base, kind)?;
    // Layers that are not cached yet are built in a throwaway reflink
    // checkout of the same base (ADR 0006).
    let builder = ReflinkBackend::new(backend.mounts().join(".layer-build"));
    let mut registered = false;
    let created = (|| -> anyhow::Result<_> {
        located.repo.pin(record.id, &base, name)?;
        let applied = if layers::needs_build(&planned, &cache) {
            let spec = GraftSpec {
                id: record.id,
                name,
                base: &base,
            };
            let scratch = timing::time("check out scratch tree", || {
                builder.create(&located.repo, &spec)
            })?;
            let applied = layers::apply(
                &planned,
                &cache,
                &mut located.store,
                &scratch.path,
                name,
                record.id,
                false,
                args.build_output(),
            );
            timing::time("remove scratch tree", || builder.prune(name, record.id))?;
            applied?
        } else {
            layers::apply(
                &planned,
                &cache,
                &mut located.store,
                &backend.path(name),
                name,
                record.id,
                false,
                args.build_output(),
            )?
        };
        let layer_paths: Vec<&Path> = applied.iter().map(|a| a.path.as_path()).collect();
        registered = register_new(
            env,
            &located.repo,
            name,
            &backend.path(name),
            &base,
            Layout::Virtual,
            &layer_paths,
        )?;
        let path = timing::time("mount", || client.mount(&located.record.id, name))?;
        Ok((path, applied))
    })();
    let (path, applied) = match created {
        Ok(c) => c,
        Err(e) => {
            let _ = client.unmount(&located.record.id, name);
            let _ = std::fs::remove_dir(backend.path(name));
            let _ = builder.prune(name, record.id);
            undo_register(&located.repo, name, &backend.path(name), registered);
            let _ = located.repo.unpin(record.id);
            let _ = located.store.delete_graft(record.id);
            // A graftd this command started for nothing is stopped again.
            if !was_running && client.mounts().is_ok_and(|m| m.is_empty()) {
                let _ = client.shutdown();
            }
            return Err(e.context(format!("creating graft {name}")));
        }
    };
    located.store.set_state(record.id, GraftState::Live)?;
    let _ = writeln!(
        out.stdout,
        "Created graft {name} at {} (base {}, virtual: served by graftd)",
        path.display(),
        base.short(7)
    );
    report_layers(&applied, &mut out);
    Ok(out.json_if(args.json, || {
        created_json(name, &path, &base, kind, &applied)
    }))
}

/// `graft new --json`.
fn created_json(
    name: &GraftName,
    path: &Path,
    base: &graft_core::CommitId,
    backend: BackendKind,
    applied: &[layers::Applied],
) -> serde_json::Value {
    serde_json::json!({
        "name": name.as_str(),
        "path": path,
        "base": base.as_str(),
        "backend": backend.to_string(),
        "layers": applied
            .iter()
            .map(|l| serde_json::json!({
                "path": l.path,
                "status": if l.built { "built" } else { "reused" },
                "key": l.key.to_string(),
            }))
            .collect::<Vec<_>>(),
    })
}

/// One line per layer of a new graft, and a warning for each one copied.
fn report_layers(applied: &[layers::Applied], out: &mut Output) {
    for layer in applied {
        let how = if layer.built { "built" } else { "reused" };
        let _ = writeln!(
            out.stdout,
            "  layer {}: {how} ({})",
            layer.path.display(),
            &layer.key.to_string()[..12]
        );
        if let Some(reason) = &layer.copied_reason {
            out.warnings.push(format!(
                "layer {} was copied instead of reflinked: {reason}",
                layer.path.display()
            ));
        }
    }
}

/// What `graft ls` shows for one graft: changed files and unshared disk
/// bytes, or why either is unknown.
fn measure(
    ctx: &Ctx,
    g: &GraftRecord,
    path: &std::path::Path,
    mounted: Option<&[GraftName]>,
    want_disk: bool,
    warnings: &mut Vec<String>,
) -> (Option<usize>, Option<u64>, Option<String>) {
    let Ctx {
        home,
        located,
        backend,
        ..
    } = ctx;
    if g.backend.is_virtual() {
        let up = mounted.is_some_and(|m| m.contains(&g.name));
        if up {
            let root_dir = ctx.root_dir();
            let changed = crate::virtual_grafts::diffs(home, located, &root_dir, path, g, warnings)
                .map(|d| d.len())
                .map_err(|e| format!("error: {e:#}"));
            let disk = if want_disk {
                crate::virtual_grafts::disk(located, &root_dir, g).map(Some)
            } else {
                Ok(None)
            }
            .map_err(|e| format!("error: {e:#}"));
            let note = changed.as_ref().err().or(disk.as_ref().err()).cloned();
            (changed.ok(), disk.ok().flatten(), note)
        } else {
            (None, None, Some("unmounted".to_owned()))
        }
    } else if path.is_dir() {
        let changed = backend
            .diff(&g.name, &g.base_commit)
            .map(|c| c.len())
            .map_err(|e| format!("error: {e}"));
        let disk = if want_disk {
            backend.disk_usage(&g.name).map(Some)
        } else {
            Ok(None)
        }
        .map_err(|e| format!("error: {e}"));
        let note = changed.as_ref().err().or(disk.as_ref().err()).cloned();
        (changed.ok(), disk.ok().flatten(), note)
    } else {
        (None, None, Some("missing".to_owned()))
    }
}

/// `graft ls --json` and `graft status --json`: the Root they describe,
/// this binary's version, and one object per graft.
fn listing_json(ctx: &Ctx, grafts: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "root": ctx.located.record.path,
        "version": env!("CARGO_PKG_VERSION"),
        "grafts": serde_json::Value::Array(grafts),
    })
}

/// Graft `g`'s registration as a git worktree, if any, and its disk use
/// with the registration's (mostly its index, which lives in Root's
/// `.git`; ADR 0036).
fn with_registration(
    repo: &graft_root::Repo,
    g: &GraftRecord,
    path: &Path,
    disk: Option<u64>,
) -> (Option<std::path::PathBuf>, Option<u64>) {
    let registration = repo.registration(&g.name, path);
    let disk = disk.map(|d| {
        d + registration
            .as_deref()
            .map_or(0, graft_root::Repo::registration_bytes)
    });
    (registration, disk)
}

/// Lists grafts.
pub(crate) fn ls(args: &LsArgs, env: &Environment) -> anyhow::Result<Output> {
    let ctx = open(env)?;
    let Ctx {
        home,
        located,
        backend,
        ..
    } = &ctx;
    let grafts = located.store.grafts()?;
    let mut out = Output::default();
    // Asking graftd also restarts it (and so remounts) after a crash.
    let mounted: Option<Vec<GraftName>> = if grafts.iter().any(|g| g.backend.is_virtual()) {
        match daemon(home, &mut out).and_then(|mut c| Ok(c.mounts()?)) {
            Ok(list) => Some(
                list.into_iter()
                    .filter(|m| m.root == located.record.id.to_string())
                    .filter_map(|m| m.graft.parse().ok())
                    .collect(),
            ),
            Err(e) => {
                out.warnings.push(format!("cannot reach graftd: {e:#}"));
                Some(Vec::new())
            }
        }
    } else {
        None
    };
    if grafts.is_empty() && !args.json {
        let _ = writeln!(
            out.stdout,
            "No grafts. Create one with {}.",
            crate::agent::suggest("`graft new <name>`", "graft_new")
        );
        return Ok(out);
    }
    let mut listed = Vec::new();
    let mut rows = vec![[
        "NAME".to_owned(),
        "BASE".to_owned(),
        "STATE".to_owned(),
        "CHANGED".to_owned(),
        "DISK".to_owned(),
    ]];
    for g in &grafts {
        let path = backend.path(&g.name);
        let (up, want_disk) = (mounted.as_deref(), !args.no_disk);
        let (changed, disk, note) = measure(&ctx, g, &path, up, want_disk, &mut out.warnings);
        let (registration, disk) = with_registration(&located.repo, g, &path, disk);
        let shown = |v: Option<String>| v.or_else(|| note.clone()).unwrap_or_default();
        rows.push([
            g.name.to_string(),
            short(g).to_owned(),
            g.state.to_string(),
            changed.map_or_else(|| "-".to_owned(), |c| c.to_string()),
            if args.no_disk {
                "-".to_owned()
            } else {
                shown(disk.map(human_bytes))
            },
        ]);
        listed.push(serde_json::json!({
            "name": g.name.as_str(),
            "base": g.base_commit.as_str(),
            "state": g.state.to_string(),
            "backend": g.backend.to_string(),
            "path": path,
            "changed": changed,
            "disk_bytes": disk,
            "note": note.clone().or_else(|| args.no_disk.then(|| "disk not measured".to_owned())),
            "git_worktree": registration.is_some(),
        }));
    }
    if args.json {
        out.json = Some(listing_json(&ctx, listed));
        return Ok(out);
    }
    let widths: Vec<usize> = (0..5)
        .map(|i| rows.iter().map(|r| r[i].len()).max().unwrap_or(0))
        .collect();
    for row in rows {
        let line = format!(
            "{:<w0$}  {:<w1$}  {:<w2$}  {:>w3$}  {:>w4$}",
            row[0],
            row[1],
            row[2],
            row[3],
            row[4],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
            w3 = widths[3],
            w4 = widths[4],
        );
        out.stdout.push_str(line.trim_end());
        out.stdout.push('\n');
    }
    Ok(out)
}

/// Prints a graft's working-tree path.
pub(crate) fn path(args: &PathArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let Ctx {
        located, backend, ..
    } = open(env)?;
    find(&located.store, &name)?;
    let path = backend.path(&name);
    Ok(Output::stdout(format!("{}\n", path.display())).json_if(
        args.json,
        || serde_json::json!({"name": name.to_string(), "path": path}),
    ))
}

/// What `graft status` reports for one graft.
struct StatusView {
    name: String,
    state: String,
    path: std::path::PathBuf,
    base: String,
    /// `(code, path)`, e.g. `("M", "src/lib.rs")`.
    changes: Vec<(char, String)>,
    /// Why the changes could not be listed.
    note: Option<&'static str>,
}

impl StatusView {
    fn text(&self) -> String {
        let mut out = format!(
            "graft {} ({}) at {}, base {}\n",
            self.name,
            self.state,
            self.path.display(),
            &self.base[..7.min(self.base.len())]
        );
        if let Some(note) = self.note {
            let _ = writeln!(out, "  {note}");
        } else if self.changes.is_empty() {
            out.push_str("  nothing changed since base\n");
        }
        for (code, path) in &self.changes {
            let _ = writeln!(out, "  {code} {path}");
        }
        out
    }

    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "state": self.state,
            "path": self.path,
            "base": self.base,
            "changes": self
                .changes
                .iter()
                .map(|(code, path)| serde_json::json!({"kind": code.to_string(), "path": path}))
                .collect::<Vec<_>>(),
            "note": self.note,
        })
    }
}

fn status_one(
    ctx: &Ctx,
    record: &GraftRecord,
    warnings: &mut Vec<String>,
) -> anyhow::Result<StatusView> {
    let path = ctx.backend.path(&record.name);
    let mut view = StatusView {
        name: record.name.to_string(),
        state: record.state.to_string(),
        path: path.clone(),
        base: record.base_commit.to_string(),
        changes: Vec::new(),
        note: None,
    };
    if record.backend.is_virtual() {
        let diffs = crate::virtual_grafts::diffs(
            &ctx.home,
            &ctx.located,
            &ctx.root_dir(),
            &path,
            record,
            warnings,
        )?;
        view.changes = diffs
            .into_iter()
            .map(|d| (d.change.kind.code(), d.change.path.display().to_string()))
            .collect();
        return Ok(view);
    }
    if !path.is_dir() {
        view.note = Some("working tree is missing; `graft prune --force` removes the record");
        return Ok(view);
    }
    view.changes = ctx
        .backend
        .diff(&record.name, &record.base_commit)?
        .into_iter()
        .map(|c| (c.kind.code(), c.path.display().to_string()))
        .collect();
    Ok(view)
}

/// Shows what changed in one graft, or summarises all of them.
pub(crate) fn status(args: &StatusArgs, env: &Environment) -> anyhow::Result<Output> {
    let ctx = open(env)?;
    let name = match &args.name {
        Some(n) => Some(parse_name(n)?),
        None => ctx.located.inside_graft.clone(),
    };
    let mut out = Output::default();
    let grafts = match name {
        Some(name) => vec![find(&ctx.located.store, &name)?],
        None => ctx.located.store.grafts()?,
    };
    if grafts.is_empty() {
        return Ok(Output::stdout("No grafts.\n".to_owned())
            .json_if(args.json, || listing_json(&ctx, Vec::new())));
    }
    // Reaching graftd remounts virtual grafts after a crash.
    if grafts.iter().any(|g| g.backend.is_virtual()) {
        daemon(&ctx.home, &mut out)?;
    }
    let mut views = Vec::new();
    for g in grafts {
        views.push(status_one(&ctx, &g, &mut out.warnings)?);
    }
    out.stdout = views.iter().map(StatusView::text).collect();
    Ok(out.json_if(args.json, || {
        listing_json(&ctx, views.iter().map(StatusView::json).collect())
    }))
}

/// Prints a unified diff of a graft against its base.
pub(crate) fn diff(args: &NameArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let Ctx {
        home,
        located,
        backend,
        ..
    } = open(env)?;
    let record = find(&located.store, &name)?;
    if record.backend.is_virtual() {
        let mut out = Output::default();
        daemon(&home, &mut out)?;
        let root_dir = home.root_dir(&located.record.id);
        let diffs = crate::virtual_grafts::diffs(
            &home,
            &located,
            &root_dir,
            &backend.path(&name),
            &record,
            &mut out.warnings,
        )?;
        out.stdout = diffs.iter().map(unified_diff).collect();
        return Ok(out);
    }
    let graft = GraftRepo::open(&backend.path(&name))?;
    let text: String = graft
        .diff_since(&record.base_commit)?
        .iter()
        .map(unified_diff)
        .collect();
    Ok(Output::stdout(text))
}

/// The refusal to prune graft `name`, whose work `why` would be lost.
fn would_lose(name: &GraftName, why: &str) -> anyhow::Error {
    let discard = crate::agent::suggest(
        "rerun with --force to discard it",
        "call graft_prune with force: true and confirm: true to discard it",
    );
    crate::agent::coded(
        "work_would_be_lost",
        format!(
            "graft {name} has work that pruning would lose:\n{}",
            why.trim_end()
        ),
        Some(format!(
            "commit it and integrate it into Root, or {discard}"
        )),
    )
}

/// One line on a commit pruning would lose.
pub(crate) fn describe_lost(lost: &graft_root::graft::Unintegrated) -> String {
    let reference = lost
        .reference
        .strip_prefix("refs/heads/")
        .unwrap_or(&lost.reference);
    if lost.in_root {
        format!(
            "commit {} on {reference} is on no other branch in Root",
            lost.commit.short(7)
        )
    } else {
        format!(
            "commit {} on {reference} is not in Root",
            lost.commit.short(7)
        )
    }
}

/// Why pruning `path` would lose work, if it would.
fn work_at_risk(ctx: &Ctx, path: &Path) -> anyhow::Result<Option<String>> {
    let graft = match GraftRepo::open(path) {
        Ok(g) => g,
        Err(e) => {
            return Ok(Some(format!(
                "  cannot inspect the graft's repository: {e}\n"
            )));
        }
    };
    let mut why = String::new();
    let uncommitted = graft.uncommitted_paths()?;
    if !uncommitted.is_empty() {
        let shown: Vec<_> = uncommitted
            .iter()
            .take(5)
            .map(|p| p.display().to_string())
            .collect();
        let more = uncommitted.len() - shown.len();
        let _ = write!(why, "  uncommitted changes: {}", shown.join(", "));
        if more > 0 {
            let _ = write!(why, " (and {more} more)");
        }
        why.push('\n');
    }
    for lost in graft.unintegrated(&ctx.located.repo)? {
        let _ = writeln!(why, "  {}", describe_lost(&lost));
    }
    Ok((!why.is_empty()).then_some(why))
}

/// Removes a graft: its working tree, its pin ref in Root and its record.
pub(crate) fn prune(args: &PruneArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let mut ctx = open(env)?;
    let record = match ctx.located.store.graft(&name)? {
        Some(r) => r,
        // Resume a prune that was interrupted after marking the record.
        None => ctx
            .located
            .store
            .pruned_graft(&name)?
            .with_context(|| format!("no graft named {name}; see `graft ls`"))?,
    };
    let path = ctx.backend.path(&name);
    if record.backend.is_virtual() {
        return prune_virtual(&mut ctx, &record, args.force, args.json);
    }
    // The lost-work check below covers the graft's branch only when its
    // working tree is still there to check.
    let checked = args.force
        || matches!(record.state, GraftState::Pruned | GraftState::Integrated)
        || path.is_dir();
    // An integrated graft's changes are in Root already.
    if !matches!(record.state, GraftState::Pruned | GraftState::Integrated)
        && !args.force
        && path.is_dir()
        && let Some(why) = work_at_risk(&ctx, &path)?
    {
        return Err(would_lose(&name, &why));
    }
    if record.state != GraftState::Pruned {
        ctx.located.store.set_state(record.id, GraftState::Pruned)?;
    }
    let mut out = Output::default();
    crate::blueprints::release(&mut ctx, &record, &mut out)?;
    // The registration names the working tree's path, so it goes first,
    // while the tree is still where it names (ADR 0036).
    unregister(&ctx, &name, checked, &mut out)?;
    // Deleting a large build tree takes seconds: move it aside, then
    // delete it after returning.
    let retired = ctx.backend.retire(&name, record.id)?;
    if retired && (args.wait || !spawn_trash_sweeper(&ctx)) {
        sweep_trash(&ctx, true)?;
    }
    remove_tmp(&ctx, record.id)?;
    ctx.located.repo.unpin(record.id)?;
    ctx.located.store.delete_graft(record.id)?;
    out.stdout = format!("Pruned graft {name}\n");
    Ok(out.json_if(
        args.json,
        || serde_json::json!({"name": name.as_str(), "pruned": true}),
    ))
}

/// Starts `graft __empty-trash` for this Root, detached, with no stdio
/// (so `graft mcp` does not wait for it). `false` if it could not start.
fn spawn_trash_sweeper(ctx: &Ctx) -> bool {
    use std::process::{Command, Stdio};
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let mut cmd = Command::new(exe);
    cmd.arg("__empty-trash")
        .current_dir(&ctx.located.record.path)
        .env("GRAFT_HOME", ctx.home.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(id) = graft_daemon::client::request_id() {
        cmd.env(graft_core::REQUEST_ID_ENV, id);
    }
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    match cmd.spawn() {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!(error = %e, "starting the background delete");
            false
        }
    }
}

/// `graft __empty-trash`: deletes the working trees prune moved aside,
/// unless another sweeper already is.
pub(crate) fn empty_trash(env: &Environment) -> anyhow::Result<Output> {
    sweep_trash(&open(env)?, false)?;
    Ok(Output::default())
}

/// Empties the trash under `trash.lock`, waiting for it if `wait`, else
/// leaving it to the sweeper that holds it. How many trees went.
pub(crate) fn sweep_trash(ctx: &Ctx, wait: bool) -> anyhow::Result<usize> {
    let path = ctx.root_dir().join("trash.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    if wait {
        lock.lock()
            .with_context(|| format!("locking {}", path.display()))?;
    } else if lock.try_lock().is_err() {
        return Ok(0);
    }
    Ok(ctx.backend.empty_trash()?)
}

/// Removes the graft's `graft run` temp dir, `$GRAFT_HOME/roots/<root-id>/tmp/<graft-id>/`.
fn remove_tmp(ctx: &Ctx, id: graft_core::GraftId) -> anyhow::Result<()> {
    let dir = crate::run::tmp_dir(&ctx.root_dir(), id);
    match std::fs::symlink_metadata(&dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("inspecting {}", dir.display())),
        Ok(m) if !m.is_dir() => bail!("{} is not a directory; not removing it", dir.display()),
        Ok(_) => {}
    }
    std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))
}

/// Prunes a FUSE graft: graftd unmounts it (lazily if busy), then the empty
/// mount point, pin and record go. A virtual graft is read-only, so no work
/// can be lost.
fn prune_virtual(
    ctx: &mut Ctx,
    record: &GraftRecord,
    force: bool,
    json: bool,
) -> anyhow::Result<Output> {
    let name = &record.name;
    let mut out = Output::default();
    let mut client = daemon(&ctx.home, &mut out)?;
    if !matches!(record.state, GraftState::Pruned | GraftState::Integrated) && !force {
        let mount = ctx.backend.path(name);
        if let Some(why) = crate::virtual_grafts::work_at_risk(
            &ctx.home,
            &ctx.located,
            &ctx.root_dir(),
            &mount,
            record,
        )? {
            return Err(would_lose(name, &why));
        }
    }
    if record.state != GraftState::Pruned {
        ctx.located.store.set_state(record.id, GraftState::Pruned)?;
    }
    crate::blueprints::release(ctx, record, &mut out)?;
    // graftd unmounts and removes the mount point, staging files, .git,
    // delta and blobs nothing else uses.
    let pruned = client.prune(&ctx.located.record.id, name)?;
    if pruned.lazy {
        out.warnings.push(format!(
            "graft {name} was in use, so it was {}",
            if cfg!(target_os = "macos") {
                "unmounted forcibly: processes with open files in it get errors"
            } else {
                "unmounted lazily: processes with open files keep them until they close them"
            }
        ));
    }
    ReflinkBackend::new(ctx.backend.mounts().join(".layer-build")).prune(name, record.id)?;
    // A registered virtual graft is locked, so its registration is always
    // there for the lost-work check above.
    unregister(ctx, name, true, &mut out)?;
    remove_tmp(ctx, record.id)?;
    ctx.located.repo.unpin(record.id)?;
    ctx.located.store.delete_graft(record.id)?;
    out.stdout = format!("Pruned graft {name}\n");
    Ok(out.json_if(
        json,
        || serde_json::json!({"name": name.as_str(), "pruned": true}),
    ))
}

/// Arguments for `graft log`.
#[derive(Debug, clap::Args)]
pub(crate) struct LogArgs {
    /// The graft's name.
    name: String,
}

/// Prints a graft's op log, oldest first.
pub(crate) fn log(args: &LogArgs, env: &Environment) -> anyhow::Result<Output> {
    use graft_store::delta::Op;
    let name = parse_name(&args.name)?;
    let ctx = open(env)?;
    let record = find(&ctx.located.store, &name)?;
    let mut warnings = Vec::new();
    if record.backend.is_virtual() {
        crate::virtual_grafts::sync(&ctx.home, &ctx.located, &record, &mut warnings);
    }
    let records = ctx.located.store.op_log(record.id)?;
    if records.is_empty() {
        let note = if record.backend.is_virtual() {
            "nothing written yet"
        } else {
            "only virtual (fuse or nfs) grafts keep an op log"
        };
        return Ok(Output::stdout(format!("graft {name}: {note}\n")));
    }
    let show = |p: &[u8]| String::from_utf8_lossy(p).into_owned();
    let mut out = String::new();
    for r in records {
        let detail = match &r.op {
            Op::Write {
                path, blob, mode, ..
            } => {
                format!("{} ({}, {mode:o})", show(path), &blob.to_string()[..12])
            }
            Op::Rename { from, to } => format!("{} -> {}", show(from), show(to)),
            Op::Symlink { path, target } => format!("{} -> {}", show(path), show(target)),
            Op::Mkdir { path, mode } | Op::Chmod { path, mode } => {
                format!("{} ({mode:o})", show(path))
            }
            Op::Touch { path, mtime } => format!("{} (mtime {mtime})", show(path)),
            Op::Unlink { path } | Op::Rmdir { path } => show(path),
            Op::Rewind { to } => format!("to op {to}"),
            Op::Rebase { base } => format!("onto {}", &base[..base.len().min(12)]),
        };
        let _ = write!(out, "{:>6}  {}  {:<8}  {detail}", r.seq, r.at, r.op.kind());
        if let Some(actor) = &r.actor {
            let _ = write!(out, "  [{actor}]");
        }
        out.push('\n');
    }
    Ok(Output {
        stdout: out,
        warnings,
        json: None,
    })
}
