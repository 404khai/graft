//! `graft plan`, `graft claim` and `graft claims` (ADR 0018).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use graft_core::{BackendKind, CommitId, GraftState};
use graft_radar::symbols::Lang;
use graft_sched::admission::{Claim, Verdict, admit};
use graft_sched::graph::{Graph, build};
use graft_sched::plan::{Plan, plan, seeds};
use graft_store::{ClaimRow, Store};

use crate::grafts::{Ctx, find, open, parse_name};
use crate::{Environment, Output};

/// Arguments for `graft plan`.
#[derive(Debug, clap::Args)]
pub(crate) struct PlanArgs {
    /// The task, as Markdown: code spans and paths name the files and
    /// symbols to change.
    task: PathBuf,
    /// Create the grafts whose dependencies have landed (at most the
    /// plan's width at a time) and record their claims.
    #[arg(long)]
    apply: bool,
    /// Backend for the grafts `--apply` creates.
    #[arg(long, value_name = "BACKEND")]
    backend: Option<BackendKind>,
    /// Print JSON.
    #[arg(long)]
    json: bool,
}

/// Arguments for `graft claim`.
#[derive(Debug, clap::Args)]
pub(crate) struct ClaimArgs {
    /// The graft claiming.
    name: String,
    /// What it claims: `path` (the whole file) or `path::symbol`.
    #[arg(required = true)]
    claims: Vec<String>,
}

/// Arguments for `graft claims`.
#[derive(Debug, clap::Args)]
pub(crate) struct ClaimsArgs {
    /// How many recent refused writes to show.
    #[arg(long, default_value_t = 10)]
    refused: u32,
}

/// Every file Radar can parse at `commit`, plus `go.mod`.
fn tree_at(ctx: &Ctx, commit: &CommitId) -> anyhow::Result<BTreeMap<PathBuf, Vec<u8>>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(&ctx.located.record.path)
        .args(["ls-tree", "-r", "-z", "--name-only", commit.as_str()])
        .output()
        .context("running git ls-tree")?;
    if !out.status.success() {
        bail!(
            "git ls-tree failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let mut tree = BTreeMap::new();
    for raw in out.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let path = graft_core::path_from_bytes(raw.to_vec());
        if Lang::of(&path).is_none() && path != Path::new("go.mod") {
            continue;
        }
        if let Some(bytes) = ctx.located.repo.read_file_at(commit, &path)? {
            tree.insert(path, bytes);
        }
    }
    Ok(tree)
}

/// A short, stable ID for a plan: Root's HEAD and the task.
fn plan_id(head: &CommitId, task: &str) -> String {
    let mut h = blake3::Hasher::new_derive_key("graft plan v1");
    h.update(head.as_str().as_bytes());
    h.update(task.as_bytes());
    h.finalize().to_hex()[..16].to_owned()
}

/// Whether a node's graft has landed: integrated, or pruned after landing.
fn landed(ctx: &Ctx, name: &str) -> anyhow::Result<bool> {
    if let Ok(n) = name.parse()
        && let Some(g) = ctx.located.store.graft(&n)?
    {
        return Ok(g.state == GraftState::Integrated);
    }
    Ok(ctx
        .located
        .store
        .integrations(10_000)?
        .iter()
        .any(|i| i.graft_name == name && i.state == graft_store::IntegrationState::Landed))
}

fn apply(
    env: &Environment,
    ctx: &mut Ctx,
    p: &Plan,
    id: &str,
    backend: Option<BackendKind>,
    out: &mut Output,
) -> anyhow::Result<()> {
    let live_here: BTreeSet<String> = ctx
        .located
        .store
        .live_claims(None)?
        .into_iter()
        .filter(|c| c.plan.as_ref().is_some_and(|(pid, _)| pid == id))
        .map(|c| c.graft)
        .collect();
    let mut running = live_here.len();
    for node in &p.nodes {
        let name = parse_name(&node.name)?;
        if ctx.located.store.graft(&name)?.is_some() || landed(ctx, &node.name)? {
            continue;
        }
        let mut waiting = Vec::new();
        for d in &node.deps {
            if !landed(ctx, &p.nodes[*d].name)? {
                waiting.push(p.nodes[*d].name.clone());
            }
        }
        if !waiting.is_empty() {
            let _ = writeln!(
                out.stdout,
                "waiting  {} (after {})",
                node.name,
                waiting.join(", ")
            );
            continue;
        }
        if running >= p.width {
            let _ = writeln!(
                out.stdout,
                "waiting  {} (width {} reached)",
                node.name, p.width
            );
            continue;
        }
        let created = crate::grafts::new(
            &crate::grafts::NewArgs::named(node.name.clone(), backend),
            env,
        )?;
        out.warnings.extend(created.warnings);
        let record = find(&ctx.located.store, &name)?;
        let claims: Vec<(String, Option<String>)> = node
            .claims
            .iter()
            .map(|c| (c.path.display().to_string(), c.symbol.clone()))
            .collect();
        let order = u32::try_from(node.index).unwrap_or(u32::MAX);
        ctx.located
            .store
            .add_claims(record.id, &claims, Some((id, order)))?;
        running += 1;
        let _ = writeln!(
            out.stdout,
            "created  {} ({} claim(s))",
            node.name,
            claims.len()
        );
    }
    Ok(())
}

/// `graft plan`.
pub(crate) fn run_plan(args: &PlanArgs, env: &Environment) -> anyhow::Result<Output> {
    let mut ctx = open(env)?;
    let task = std::fs::read_to_string(&args.task)
        .with_context(|| format!("reading {}", args.task.display()))?;
    let head = ctx.located.repo.head_commit()?;
    let graph: Graph = build(&tree_at(&ctx, &head)?);
    let found = seeds(&task, &graph);
    let p = plan(&graph, &found);
    let id = plan_id(&head, &task);
    let mut out = Output::default();
    if args.json {
        out.stdout = serde_json::to_string(
            &serde_json::json!({ "plan": id, "base": head.as_str(), "nodes": p.nodes, "width": p.width }),
        )? + "\n";
    } else {
        let _ = writeln!(
            out.stdout,
            "Plan {id} at {} ({} files in the graph)",
            head.short(7),
            graph.files.len()
        );
        out.stdout.push_str(&p.render());
    }
    if args.apply {
        if p.nodes.is_empty() {
            bail!("nothing to apply: the task names no files or symbols of this repository");
        }
        apply(env, &mut ctx, &p, &id, args.backend, &mut out)?;
    }
    Ok(out)
}

/// Live claims as the scheduler sees them.
pub(crate) fn claims_for_admission(
    store: &Store,
    path: Option<&str>,
) -> anyhow::Result<Vec<Claim>> {
    Ok(store
        .live_claims(path)?
        .into_iter()
        .map(|c: ClaimRow| Claim {
            graft: c.graft,
            path: PathBuf::from(c.path),
            symbol: c.symbol,
            plan: c.plan,
        })
        .collect())
}

/// The plan position graft `name` holds claims under, if any.
pub(crate) fn plan_of(claims: &[Claim], name: &str) -> Option<(String, u32)> {
    claims
        .iter()
        .filter(|c| c.graft == name)
        .find_map(|c| c.plan.clone())
}

/// `graft claim`.
pub(crate) fn claim(
    args: &ClaimArgs,
    env: &Environment,
) -> anyhow::Result<(Output, std::process::ExitCode)> {
    let name = parse_name(&args.name)?;
    let mut ctx = open(env)?;
    let record = find(&ctx.located.store, &name)?;
    if record.state != GraftState::Live {
        bail!(
            "graft {name} is {}; only live grafts hold claims",
            record.state
        );
    }
    let all = claims_for_admission(&ctx.located.store, None)?;
    let mine = plan_of(&all, name.as_str());
    let mut out = Output::default();
    let mut ok = true;
    for spec in &args.claims {
        let (path, symbol) = match spec.split_once("::") {
            Some((p, s)) => (p.trim_start_matches("./").to_owned(), Some(s.to_owned())),
            None => (spec.trim_start_matches("./").to_owned(), None),
        };
        let changed: Option<BTreeSet<String>> = symbol.clone().map(|s| BTreeSet::from([s]));
        let verdict = admit(
            name.as_str(),
            mine.as_ref(),
            Path::new(&path),
            changed.as_ref(),
            &all,
        );
        if verdict == Verdict::Allowed {
            ctx.located
                .store
                .add_claims(record.id, &[(path, symbol)], None)?;
            let _ = writeln!(out.stdout, "claimed  {spec}");
        } else {
            ok = false;
            let text = verdict.describe();
            ctx.located
                .store
                .record_admission(name.as_str(), &path, &text)?;
            let _ = writeln!(out.stdout, "{text}");
        }
    }
    let code = if ok {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    };
    Ok((out, code))
}

/// `graft claims`.
pub(crate) fn claims(args: &ClaimsArgs, env: &Environment) -> anyhow::Result<Output> {
    let ctx = open(env)?;
    let mut s = String::new();
    let live = ctx.located.store.live_claims(None)?;
    if live.is_empty() {
        s.push_str("No live claims.\n");
    } else {
        for c in live {
            let what = c
                .symbol
                .map_or(c.path.clone(), |sym| format!("{}::{sym}", c.path));
            let plan = c
                .plan
                .map(|(p, o)| format!("  (plan {p}, node {})", o + 1))
                .unwrap_or_default();
            let _ = writeln!(s, "{:<20} {what}{plan}", c.graft);
        }
    }
    let refused = ctx.located.store.admissions(args.refused)?;
    if !refused.is_empty() {
        s.push_str("\nRefused writes:\n");
        for r in refused {
            let _ = writeln!(s, "{} {:<20} {}: {}", r.at, r.graft, r.path, r.verdict);
        }
    }
    Ok(Output::stdout(s))
}
