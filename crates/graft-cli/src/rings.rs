//! `graft bud`, `ring` and `rewind` (ADR 0013).
//!
//! All three work on virtual grafts, whose changes are a delta in the
//! store. They are metadata operations: no file contents are copied.

use std::fmt::Write as _;

use anyhow::{Context, bail};
use graft_core::{GraftState, RingLabel, Timestamp};
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
}

/// Arguments for `graft ring`.
#[derive(Debug, clap::Args)]
pub(crate) struct RingArgs {
    /// The graft to checkpoint.
    name: String,
    /// The ring's label (lowercase letters, digits and hyphens). Defaults
    /// to `ring-<seq>`, after the graft's last op.
    label: Option<String>,
    /// List the graft's rings instead of taking one.
    #[arg(long, conflicts_with = "label")]
    list: bool,
}

/// Arguments for `graft rewind`.
#[derive(Debug, clap::Args)]
pub(crate) struct RewindArgs {
    /// The graft to rewind.
    name: String,
    /// The ring to return to.
    ring: String,
}

/// Refuses reflink grafts, whose changes live in a working tree rather than
/// a delta (ADR 0013).
fn require_virtual(record: &GraftRecord, what: &str) -> anyhow::Result<()> {
    if !record.backend.is_virtual() {
        bail!(
            "graft {} uses the reflink backend, which cannot {what} yet; \
             create a virtual graft with `graft new --backend {}` (ADR 0013)",
            record.name,
            if cfg!(target_os = "macos") {
                "nfs"
            } else {
                "fuse"
            }
        );
    }
    Ok(())
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
        let created = (|| -> anyhow::Result<_> {
            ctx.located.repo.pin(record.id, &record.base_commit, &dst)?;
            Ok(timing::time("mount", || {
                client.mount(&ctx.located.record.id, &dst)
            })?)
        })();
        let path = match created {
            Ok(p) => p,
            Err(e) => {
                let _ = client.unmount(&ctx.located.record.id, &dst);
                let _ = std::fs::remove_dir(ctx.backend.path(&dst));
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
        Ok(out)
    })
}

/// Takes or lists a graft's rings.
pub(crate) fn ring(args: &RingArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let mut ctx = open(env)?;
    let record = find(&ctx.located.store, &name)?;
    if args.list {
        let rings = ctx.located.store.rings(record.id)?;
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
    require_virtual(&record, "take rings")?;
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
    let ring = ctx.located.store.ring(record.id, label)?;
    let _ = writeln!(
        out.stdout,
        "Ringed graft {name} as {} (op {}, {} delta entries)",
        ring.label, ring.seq, ring.paths
    );
    Ok(out)
}

/// Returns a graft to one of its rings.
pub(crate) fn rewind(args: &RewindArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let label: RingLabel = args.ring.parse()?;
    let mut ctx = open(env)?;
    let record = find(&ctx.located.store, &name)?;
    require_virtual(&record, "be rewound")?;
    if ctx.located.store.ring_named(record.id, &label)?.is_none() {
        bail!("graft {name} has no ring named {label}; see `graft ring {name} --list`");
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
    let _ = writeln!(
        out.stdout,
        "Rewound graft {name} to ring {label} (op {}). To undo: graft rewind {name} {}",
        rewound.seq, rewound.undo
    );
    Ok(out)
}
