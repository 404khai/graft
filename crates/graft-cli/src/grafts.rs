//! `graft new`, `ls`, `path`, `status`, `diff` and `prune`.
//!
//! These orchestrate the store (records), the Root (pin refs) and the
//! filesystem backend (working trees). `graftd` takes this over when the
//! daemon arrives.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, bail};
use graft_core::{GraftName, GraftState};
use graft_fs::{FsBackend, GraftSpec, ReflinkBackend, human_bytes};
use graft_root::graft::{GraftRepo, unified_diff};
use graft_root::layer::LayerCache;
use graft_store::registry::{Located, locate};
use graft_store::{GraftRecord, Store};

use crate::layers;
use crate::{Environment, Output};

/// Arguments for `graft new`.
#[derive(Debug, clap::Args)]
pub(crate) struct NewArgs {
    /// Name of the graft: lowercase letters, digits and hyphens.
    name: String,
    /// Revision to start from (branch, tag or commit). Defaults to Root's
    /// HEAD.
    #[arg(long, value_name = "REF")]
    from: Option<String>,
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

struct Ctx {
    located: Located,
    backend: ReflinkBackend,
    layers: LayerCache,
}

fn open(env: &Environment) -> anyhow::Result<Ctx> {
    let home = env.require_home()?;
    let located = locate(home, Path::new("."))?;
    let root_dir = home.root_dir(&located.record.id);
    Ok(Ctx {
        located,
        backend: ReflinkBackend::new(root_dir.join("mounts")),
        layers: LayerCache::new(root_dir.join("layers")),
    })
}

fn parse_name(name: &str) -> anyhow::Result<GraftName> {
    Ok(name.parse()?)
}

fn find(store: &Store, name: &GraftName) -> anyhow::Result<GraftRecord> {
    store
        .graft(name)?
        .with_context(|| format!("no graft named {name}; see `graft ls`"))
}

fn short(record: &GraftRecord) -> &str {
    record.base_commit.short(7)
}

/// Creates a graft.
pub(crate) fn new(args: &NewArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let Ctx {
        mut located,
        backend,
        layers: cache,
    } = open(env)?;
    let rev = args.from.as_deref().unwrap_or("HEAD");
    let base = located.repo.resolve_commit(rev)?;
    // Refuse unapproved layer commands before anything is created.
    let planned = layers::plan(
        &located.repo,
        &located.store,
        &cache,
        &base,
        args.from.as_deref(),
    )?;

    // The record reserves the name before any files exist.
    let record = located.store.create_graft(&name, &base)?;
    let spec = GraftSpec {
        id: record.id,
        name: &name,
        base: &base,
    };
    let created = backend
        .create(&located.repo, &spec)
        .map_err(anyhow::Error::from)
        .and_then(|report| {
            located.repo.pin(record.id, &base, &name)?;
            let applied = layers::apply(
                &planned,
                &cache,
                &mut located.store,
                &report.path,
                &name,
                record.id,
            )?;
            Ok((report, applied))
        });
    let (report, applied) = match created {
        Ok(created) => created,
        Err(e) => {
            let _ = backend.prune(&name, record.id);
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

/// Lists grafts.
pub(crate) fn ls(env: &Environment) -> anyhow::Result<Output> {
    let Ctx {
        located, backend, ..
    } = open(env)?;
    let grafts = located.store.grafts()?;
    let mut out = Output::default();
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
        let (changed, disk) = if path.is_dir() {
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

fn status_one(ctx: &Ctx, record: &GraftRecord) -> anyhow::Result<String> {
    let path = ctx.backend.path(&record.name);
    let mut out = format!(
        "graft {} ({}) at {}, base {}\n",
        record.name,
        record.state,
        path.display(),
        short(record)
    );
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
    if let Some(name) = name {
        let record = find(&ctx.located.store, &name)?;
        return Ok(Output::stdout(status_one(&ctx, &record)?));
    }
    let grafts = ctx.located.store.grafts()?;
    if grafts.is_empty() {
        return Ok(Output::stdout("No grafts.\n".to_owned()));
    }
    let mut out = String::new();
    for g in grafts {
        out.push_str(&status_one(&ctx, &g)?);
    }
    Ok(Output::stdout(out))
}

/// Prints a unified diff of a graft against its base.
pub(crate) fn diff(args: &NameArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let Ctx {
        located, backend, ..
    } = open(env)?;
    let record = find(&located.store, &name)?;
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
