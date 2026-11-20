//! `graft bud`, `ring` and `rewind` (ADR 0013).
//!
//! All three work on virtual grafts, whose changes are a delta in the
//! store. They are metadata operations: no file contents are copied.

use std::fmt::Write as _;

use anyhow::{Context, bail};
use graft_core::{GraftName, GraftState, RingLabel, Timestamp};
use graft_daemon::Client;
use graft_fs::FsBackend;
use graft_store::GraftRecord;

use crate::grafts::{Ctx, daemon, find, open, parse_name};
use crate::{Environment, Output, timing};

/// Arguments for `graft bud`.
#[derive(Debug, clap::Args)]
pub(crate) struct BudArgs {
    /// The graft to fork.
    src: String,
    /// The new graft's name.
    dst: String,
    /// Print JSON: `{name, src, path, base, entries}`.
    #[arg(long)]
    json: bool,
}

/// Arguments for `graft ring`.
#[derive(Debug, clap::Args)]
pub(crate) struct RingArgs {
    /// The graft to checkpoint.
    name: String,
    /// The ring's label (lowercase letters, digits and hyphens). Without
    /// one, a ring named `ring-<seq>` (after the graft's last op) is still
    /// taken; use `--list` to see the rings.
    label: Option<String>,
    /// List the graft's rings instead of taking one.
    #[arg(long, conflicts_with = "label")]
    list: bool,
    /// Print JSON: the ring `{graft, label, seq, entries, at}`, or with
    /// `--list` a list of them.
    #[arg(long)]
    json: bool,
}

/// Arguments for `graft rewind`.
#[derive(Debug, clap::Args)]
pub(crate) struct RewindArgs {
    /// The graft to rewind.
    name: String,
    /// The ring to return to.
    ring: String,
    /// Print JSON: `{name, ring, seq, undo}`.
    #[arg(long)]
    json: bool,
}

/// Refuses reflink grafts for bud, which would have to clone the whole
/// working tree (ADR 0013; rings and rewind work on them, ADR 0038).
fn require_virtual(record: &GraftRecord, what: &str) -> anyhow::Result<()> {
    if !record.backend.is_virtual() {
        let kind = if cfg!(target_os = "macos") {
            "nfs"
        } else {
            "fuse"
        };
        let how = if crate::agent::is_mcp() {
            format!("graft_new with backend: \"{kind}\"")
        } else {
            format!("`graft new --backend {kind}`")
        };
        return Err(crate::agent::coded(
            "backend_unsupported",
            format!(
                "graft {} uses the reflink backend, which cannot {what} yet (ADR 0013)",
                record.name
            ),
            Some(format!("create a virtual graft with {how}")),
        ));
    }
    Ok(())
}

/// The working tree of reflink graft `name`, which must exist.
fn reflink_dir(ctx: &Ctx, name: &GraftName) -> anyhow::Result<std::path::PathBuf> {
    let dir = ctx.backend.path(name);
    if !dir.is_dir() {
        bail!("graft {name}'s working tree is missing ({})", dir.display());
    }
    Ok(dir)
}

/// `graft ring` for a reflink graft: a git snapshot (ADR 0038).
fn reflink_ring(ctx: &Ctx, name: &GraftName, args: &RingArgs) -> anyhow::Result<Output> {
    let dir = reflink_dir(ctx, name)?;
    if args.list {
        let rings = crate::reflink_rings::list(&dir)?;
        if args.json {
            let list = rings.iter().map(|r| r.json(name.as_str())).collect();
            return Ok(Output::default().json_if(true, || serde_json::Value::Array(list)));
        }
        if rings.is_empty() {
            return Ok(Output::stdout(format!("graft {name}: no rings\n")));
        }
        return Ok(Output::stdout(crate::reflink_rings::list_text(&rings)));
    }
    let label = args
        .label
        .as_deref()
        .map(str::parse::<RingLabel>)
        .transpose()?;
    if label
        .as_ref()
        .is_some_and(|l| l.as_str().starts_with("undo-"))
    {
        bail!("ring labels starting with \"undo-\" are reserved for `graft rewind`");
    }
    let ring = crate::reflink_rings::ring(&dir, label.as_ref().map(RingLabel::as_str))?;
    let mut out = Output::stdout(format!(
        "Ringed graft {name} as {}: commit {} (snapshot of HEAD {} + {} uncommitted paths vs HEAD; untracked files included, ignored ones not)\n",
        ring.label,
        &ring.commit[..7],
        &ring.head[..7],
        ring.entries
    ));
    if label.is_none() {
        let _ = writeln!(
            out.stdout,
            "  (no label given, so it is named {}; `graft ring {name} --list` lists rings)",
            ring.label
        );
    }
    Ok(out.json_if(args.json, || ring.json(name.as_str())))
}

/// `graft rewind` for a reflink graft (ADR 0038).
fn reflink_rewind(
    ctx: &Ctx,
    name: &GraftName,
    label: &RingLabel,
    json: bool,
) -> anyhow::Result<Output> {
    let dir = reflink_dir(ctx, name)?;
    if crate::reflink_rings::list(&dir)?
        .iter()
        .all(|r| r.label != label.as_str())
    {
        let list = if crate::agent::is_mcp() {
            format!("graft_ring with name: \"{name}\", list: true")
        } else {
            format!("`graft ring {name} --list`")
        };
        return Err(crate::agent::coded(
            "not_found",
            format!("graft {name} has no ring named {label}"),
            Some(format!("see {list}")),
        ));
    }
    let undo = crate::reflink_rings::rewind(&dir, label.as_str())?;
    let how = if crate::agent::is_mcp() {
        format!("graft_rewind with name: \"{name}\", ring: \"{undo}\"")
    } else {
        format!("graft rewind {name} {undo}")
    };
    Ok(Output::stdout(format!(
        "Rewound graft {name} to ring {label}. To undo: {how}\n"
    ))
    .json_if(json, || {
        serde_json::json!({"name": name.as_str(), "ring": label.as_str(), "seq": null, "undo": undo})
    }))
}

/// Whether graftd has `record` mounted.
fn is_mounted(ctx: &Ctx, client: &mut Client, record: &GraftRecord) -> anyhow::Result<bool> {
    let root = ctx.located.record.id.to_string();
    Ok(client
        .mounts()?
        .iter()
        .any(|m| m.root == root && m.graft == record.name.as_str()))
}

/// Journals the graft's pending writes, so the delta read next is current.
fn sync(ctx: &Ctx, client: &mut Client, record: &GraftRecord) -> anyhow::Result<()> {
    if is_mounted(ctx, client, record)? {
        client
            .sync(&ctx.located.record.id, &record.name)
            .with_context(|| format!("journaling graft {}'s recent writes", record.name))?;
    }
    Ok(())
}

/// Forks a graft in constant time.
pub(crate) fn bud(args: &BudArgs, env: &Environment) -> anyhow::Result<Output> {
    let src = parse_name(&args.src)?;
    let dst = parse_name(&args.dst)?;
    timing::time("graft bud", || {
        let mut ctx = open(env)?;
        let source = find(&ctx.located.store, &src)?;
        require_virtual(&source, "be budded")?;
        let mut out = Output::default();
        let mut client = timing::time("reach graftd", || daemon(&ctx.home, &mut out))?;
        timing::time("sync source", || sync(&ctx, &mut client, &source))?;
        let record = timing::time("copy delta", || ctx.located.store.bud(source.id, &dst))?;
        let mut registered = false;
        let created = (|| -> anyhow::Result<_> {
            ctx.located.repo.pin(record.id, &record.base_commit, &dst)?;
            if crate::grafts::register_worktrees(env) {
                // The bud gets its own registration and branch at the base,
                // as a budded graft gets a fresh `.git` (ADR 0013).
                let layers =
                    graft_fs::layer_mounts(&ctx.located.store, &ctx.root_dir(), record.id)?;
                let paths: Vec<&std::path::Path> =
                    layers.iter().map(|l| l.path.as_path()).collect();
                registered = crate::grafts::register_new(
                    env,
                    &ctx.located.repo,
                    &dst,
                    &ctx.backend.path(&dst),
                    &record.base_commit,
                    graft_root::worktree::Layout::Virtual,
                    &paths,
                )?;
            }
            Ok(timing::time("mount", || {
                client.mount(&ctx.located.record.id, &dst)
            })?)
        })();
        let path = match created {
            Ok(p) => p,
            Err(e) => {
                let _ = client.unmount(&ctx.located.record.id, &dst);
                let _ = std::fs::remove_dir(ctx.backend.path(&dst));
                crate::grafts::undo_register(
                    &ctx.located.repo,
                    &dst,
                    &ctx.backend.path(&dst),
                    registered,
                );
                let _ = ctx.located.repo.unpin(record.id);
                let _ = ctx.located.store.delete_graft(record.id);
                return Err(e.context(format!("budding graft {dst}")));
            }
        };
        ctx.located.store.set_state(record.id, GraftState::Live)?;
        let paths = ctx.located.store.delta(record.id)?.entries().count();
        let _ = writeln!(
            out.stdout,
            "Budded graft {dst} from {src} at {} (base {}, {paths} delta entries, blobs shared)",
            path.display(),
            record.base_commit.short(7)
        );
        Ok(out.json_if(args.json, || {
            serde_json::json!({
                "name": dst.as_str(),
                "src": src.as_str(),
                "path": path,
                "base": record.base_commit.as_str(),
                "entries": paths,
            })
        }))
    })
}

/// Takes or lists a graft's rings.
pub(crate) fn ring(args: &RingArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let mut ctx = open(env)?;
    let record = find(&ctx.located.store, &name)?;
    if !record.backend.is_virtual() {
        return reflink_ring(&ctx, &name, args);
    }
    if args.list {
        let rings = ctx.located.store.rings(record.id)?;
        if args.json {
            let list: Vec<_> = rings.iter().map(|r| ring_json(&name, r)).collect();
            return Ok(Output::default().json_if(true, || serde_json::Value::Array(list)));
        }
        if rings.is_empty() {
            return Ok(Output::stdout(format!("graft {name}: no rings\n")));
        }
        let mut out = String::new();
        for r in rings {
            let _ = writeln!(
                out,
                "{:<40}  op {:>6}  {:>6} entries  {}",
                r.label, r.seq, r.paths, r.created_at
            );
        }
        return Ok(Output::stdout(out));
    }
    let label = args
        .label
        .as_deref()
        .map(str::parse::<RingLabel>)
        .transpose()?;
    if label
        .as_ref()
        .is_some_and(|l| l.as_str().starts_with("undo-"))
    {
        bail!("ring labels starting with \"undo-\" are reserved for `graft rewind`");
    }
    let mut out = Output::default();
    let mut client = daemon(&ctx.home, &mut out)?;
    sync(&ctx, &mut client, &record)?;
    let named = label.is_some();
    let ring = ctx.located.store.ring(record.id, label)?;
    let _ = writeln!(
        out.stdout,
        "Ringed graft {name} as {} (op {}, {} delta entries)",
        ring.label, ring.seq, ring.paths
    );
    if !named {
        // `graft ring <name>` reads like a listing; say what happened.
        let _ = writeln!(
            out.stdout,
            "  (no label given, so it is named {}; `graft ring {name} --list` lists rings)",
            ring.label
        );
    }
    Ok(out.json_if(args.json, || ring_json(&name, &ring)))
}

fn ring_json(graft: &graft_core::GraftName, r: &graft_store::Ring) -> serde_json::Value {
    serde_json::json!({
        "graft": graft.as_str(),
        "label": r.label.as_str(),
        "seq": r.seq,
        "entries": r.paths,
        "at": r.created_at.to_string(),
    })
}

/// Returns a graft to one of its rings.
pub(crate) fn rewind(args: &RewindArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let label: RingLabel = args.ring.parse()?;
    let mut ctx = open(env)?;
    let record = find(&ctx.located.store, &name)?;
    if !record.backend.is_virtual() {
        return reflink_rewind(&ctx, &name, &label, args.json);
    }
    if ctx.located.store.ring_named(record.id, &label)?.is_none() {
        let list = if crate::agent::is_mcp() {
            format!("graft_ring with name: \"{name}\", list: true")
        } else {
            format!("`graft ring {name} --list`")
        };
        return Err(crate::agent::coded(
            "not_found",
            format!("graft {name} has no ring named {label}"),
            Some(format!("see {list}")),
        ));
    }
    let mut out = Output::default();
    let mut client = daemon(&ctx.home, &mut out)?;
    sync(&ctx, &mut client, &record)?;
    // Unmounting drops graftd's view and the kernel's caches of the old
    // delta; the graft is remounted whatever happens next.
    let root = ctx.located.record.id;
    let unmounted = client.unmount(&root, &name)?;
    if unmounted.lazy {
        // Processes with open files still reach the old view, which would
        // journal on top of the rewound delta. Leave the delta alone.
        let remounted = client.mount(&root, &name);
        bail!(
            "graft {name} is in use (files are open in it), so it was not rewound{}; \
             close them and retry",
            remounted
                .err()
                .map(|e| format!(" and could not be remounted: {e}"))
                .unwrap_or_default()
        );
    }
    let rewound = ctx
        .located
        .store
        .rewind(record.id, &label, Timestamp::now());
    let remounted = client.mount(&root, &name);
    let rewound = rewound?;
    remounted.with_context(|| format!("remounting graft {name} after the rewind"))?;
    let undo = if crate::agent::is_mcp() {
        format!(
            "graft_rewind with name: \"{name}\", ring: \"{}\"",
            rewound.undo
        )
    } else {
        format!("graft rewind {name} {}", rewound.undo)
    };
    let _ = writeln!(
        out.stdout,
        "Rewound graft {name} to ring {label} (op {}). To undo: {undo}",
        rewound.seq
    );
    Ok(out.json_if(args.json, || {
        serde_json::json!({
            "name": name.as_str(),
            "ring": label.as_str(),
            "seq": rewound.seq,
            "undo": rewound.undo.as_str(),
        })
    }))
}
