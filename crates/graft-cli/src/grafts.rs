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
}

/// Arguments for commands that take one graft name.
#[derive(Debug, clap::Args)]
pub(crate) struct NameArgs {
    /// The graft's name.
    name: String,
}

/// Arguments for `graft status`.
#[derive(Debug, clap::Args)]
pub(crate) struct StatusArgs {
    /// The graft's name. Defaults to the graft containing the current
    /// directory; outside a graft, summarises every graft.
    name: Option<String>,
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
    store
        .graft(name)?
        .with_context(|| format!("no graft named {name}; see `graft ls`"))
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

/// Connects to graftd, starting it if needed, and warns once if its version
/// differs from ours.
pub(crate) fn daemon(home: &GraftHome, out: &mut Output) -> anyhow::Result<Client> {
    let mut client = Client::connect_or_start(home)?;
    let ping = client.ping()?;
    if ping.version != env!("CARGO_PKG_VERSION") {
        out.warnings.push(format!(
            "graftd {} is running but this is graft {}; stop it with `graft daemon stop` once no grafts are in use",
            ping.version,
            env!("CARGO_PKG_VERSION")
        ));
    }
    Ok(client)
}

/// Creates a graft.
pub(crate) fn new(args: &NewArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let kind = choose_backend(args, env)?;
    timing::time("graft new", || {
        if kind.is_virtual() {
            new_virtual(args, env, &name, kind)
        } else {
            new_reflink(args, env, &name)
        }
    })
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
    // Refuse unapproved layer commands before anything is created.
    let planned = timing::time("plan layers", || {
        layers::plan(
            &located.repo,
            &located.store,
            &cache,
            &base,
            args.from.as_deref(),
        )
    })?;

    // The record reserves the name before any files exist.
    let record = located
        .store
        .create_graft(name, &base, BackendKind::Reflink)?;
    let spec = GraftSpec {
        id: record.id,
        name,
        base: &base,
    };
    let created = timing::time("check out tree", || backend.create(&located.repo, &spec))
        .map_err(anyhow::Error::from)
        .and_then(|report| {
            timing::time("pin base", || located.repo.pin(record.id, &base, name))?;
            let applied = layers::apply(
                &planned,
                &cache,
                &mut located.store,
                &report.path,
                name,
                record.id,
                true,
            )?;
            Ok((report, applied))
        });
    let (report, applied) = match created {
        Ok(created) => created,
        Err(e) => {
            let _ = backend.prune(name, record.id);
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
    for layer in &applied {
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
    let planned = timing::time("plan layers", || {
        layers::plan(
            &located.repo,
            &located.store,
            &cache,
            &base,
            args.from.as_deref(),
        )
    })?;
    let mut out = Output::default();
    // Reach graftd before recording anything, so an unsupported platform
    // leaves nothing behind.
    let mut client = timing::time("reach graftd", || daemon(&home, &mut out))?;
    let record = located.store.create_graft(name, &base, kind)?;
    // Layers that are not cached yet are built in a throwaway reflink
    // checkout of the same base (ADR 0006).
    let builder = ReflinkBackend::new(backend.mounts().join(".layer-build"));
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
            )?
        };
        let path = timing::time("mount", || client.mount(&located.record.id, name))?;
        Ok((path, applied))
    })();
    let (path, applied) = match created {
        Ok(c) => c,
        Err(e) => {
            let _ = client.unmount(&located.record.id, name);
            let _ = std::fs::remove_dir(backend.path(name));
            let _ = builder.prune(name, record.id);
            let _ = located.repo.unpin(record.id);
            let _ = located.store.delete_graft(record.id);
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
    for layer in &applied {
        let how = if layer.built { "built" } else { "reused" };
        let _ = writeln!(
            out.stdout,
            "  layer {}: {how} ({})",
            layer.path.display(),
            &layer.key.to_string()[..12]
        );
    }
    Ok(out)
}

/// Lists grafts.
pub(crate) fn ls(env: &Environment) -> anyhow::Result<Output> {
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
    if grafts.is_empty() {
        out.stdout
            .push_str("No grafts. Create one with `graft new <name>`.\n");
        return Ok(out);
    }
    let mut rows = vec![[
        "NAME".to_owned(),
        "BASE".to_owned(),
        "STATE".to_owned(),
        "CHANGED".to_owned(),
        "DISK".to_owned(),
    ]];
    for g in &grafts {
        let path = backend.path(&g.name);
        let (changed, disk) = if g.backend.is_virtual() {
            let up = mounted.as_ref().is_some_and(|m| m.contains(&g.name));
            if up {
                let root_dir = ctx.root_dir();
                let changed = crate::virtual_grafts::diffs(
                    home,
                    located,
                    &root_dir,
                    &path,
                    g,
                    &mut out.warnings,
                )
                .map_or_else(|e| format!("error: {e:#}"), |d| d.len().to_string());
                let disk = crate::virtual_grafts::disk(located, &root_dir, g)
                    .map_or_else(|e| format!("error: {e:#}"), human_bytes);
                (changed, disk)
            } else {
                ("-".to_owned(), "unmounted".to_owned())
            }
        } else if path.is_dir() {
            let changed = backend
                .diff(&g.name, &g.base_commit)
                .map_or_else(|e| format!("error: {e}"), |c| c.len().to_string());
            let disk = backend
                .disk_usage(&g.name)
                .map_or_else(|e| format!("error: {e}"), human_bytes);
            (changed, disk)
        } else {
            ("-".to_owned(), "missing".to_owned())
        };
        rows.push([
            g.name.to_string(),
            short(g).to_owned(),
            g.state.to_string(),
            changed,
            disk,
        ]);
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
pub(crate) fn path(args: &NameArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let Ctx {
        located, backend, ..
    } = open(env)?;
    find(&located.store, &name)?;
    Ok(Output::stdout(format!(
        "{}\n",
        backend.path(&name).display()
    )))
}

fn status_one(
    ctx: &Ctx,
    record: &GraftRecord,
    warnings: &mut Vec<String>,
) -> anyhow::Result<String> {
    let path = ctx.backend.path(&record.name);
    let mut out = format!(
        "graft {} ({}) at {}, base {}\n",
        record.name,
        record.state,
        path.display(),
        short(record)
    );
    if record.backend.is_virtual() {
        let diffs = crate::virtual_grafts::diffs(
            &ctx.home,
            &ctx.located,
            &ctx.root_dir(),
            &path,
            record,
            warnings,
        )?;
        if diffs.is_empty() {
            out.push_str("  nothing changed since base\n");
        }
        for d in diffs {
            let _ = writeln!(
                out,
                "  {} {}",
                d.change.kind.code(),
                d.change.path.display()
            );
        }
        return Ok(out);
    }
    if !path.is_dir() {
        out.push_str("  working tree is missing; `graft prune --force` removes the record\n");
        return Ok(out);
    }
    let changes = ctx.backend.diff(&record.name, &record.base_commit)?;
    if changes.is_empty() {
        out.push_str("  nothing changed since base\n");
    }
    for c in changes {
        let _ = writeln!(out, "  {} {}", c.kind.code(), c.path.display());
    }
    Ok(out)
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
        return Ok(Output::stdout("No grafts.\n".to_owned()));
    }
    // Reaching graftd remounts virtual grafts after a crash.
    if grafts.iter().any(|g| g.backend.is_virtual()) {
        daemon(&ctx.home, &mut out)?;
    }
    for g in grafts {
        let text = status_one(&ctx, &g, &mut out.warnings)?;
        out.stdout.push_str(&text);
    }
    Ok(out)
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
        let _ = writeln!(
            why,
            "  commit {} on {} is not in Root",
            lost.commit.short(7),
            lost.reference
        );
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
        return prune_virtual(&mut ctx, &record, args.force);
    }
    if record.state != GraftState::Pruned
        && !args.force
        && path.is_dir()
        && let Some(why) = work_at_risk(&ctx, &path)?
    {
        bail!(
            "graft {name} has work that pruning would lose:\n{why}\
             Commit it and integrate it into Root, or rerun with --force to discard it."
        );
    }
    if record.state != GraftState::Pruned {
        ctx.located.store.set_state(record.id, GraftState::Pruned)?;
    }
    ctx.backend.prune(&name, record.id)?;
    ctx.located.repo.unpin(record.id)?;
    ctx.located.store.delete_graft(record.id)?;
    Ok(Output::stdout(format!("Pruned graft {name}\n")))
}

/// Prunes a FUSE graft: graftd unmounts it (lazily if busy), then the empty
/// mount point, pin and record go. A virtual graft is read-only, so no work
/// can be lost.
fn prune_virtual(ctx: &mut Ctx, record: &GraftRecord, force: bool) -> anyhow::Result<Output> {
    let name = &record.name;
    let mut out = Output::default();
    let mut client = daemon(&ctx.home, &mut out)?;
    if record.state != GraftState::Pruned && !force {
        let mount = ctx.backend.path(name);
        if let Some(why) = crate::virtual_grafts::work_at_risk(
            &ctx.home,
            &ctx.located,
            &ctx.root_dir(),
            &mount,
            record,
        )? {
            bail!(
                "graft {name} has work that pruning would lose:\n{why}\
                 Commit it and integrate it into Root, or rerun with --force to discard it."
            );
        }
    }
    if record.state != GraftState::Pruned {
        ctx.located.store.set_state(record.id, GraftState::Pruned)?;
    }
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
    ctx.located.repo.unpin(record.id)?;
    ctx.located.store.delete_graft(record.id)?;
    out.stdout = format!("Pruned graft {name}\n");
    Ok(out)
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
    })
}
