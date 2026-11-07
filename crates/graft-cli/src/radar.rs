//! `graft radar`: overlapping work across live grafts (ADR 0016).
//!
//! Path and symbol overlaps come from `graft-radar`, fed with each live
//! graft's changes. Behaviour checks merge two overlapping grafts in
//! scratch checkouts under `$GRAFT_HOME/roots/<root-id>/radar/` and run the
//! `[radar] test` command, confined, alone and merged; results are cached
//! by fingerprint.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use graft_blueprint::{BLUEPRINT_FILE, RadarSpec, parse_radar};
use graft_core::{CommitId, GraftId, GraftName, GraftState};
use graft_fs::{FsBackend, GraftSpec, ReflinkBackend};
use graft_radar::{FileChange, GraftChanges, Level, Overlap, overlaps};
use graft_root::Repo;
use graft_root::graft::GraftRepo;
use graft_store::{CheckOutcome, GraftRecord, Store};

use crate::grafts::{Ctx, open};
use crate::{Environment, Output};

/// Arguments for `graft radar`.
#[derive(Debug, clap::Args)]
pub(crate) struct RadarArgs {
    /// Run the behaviour checks (speculative merges with the `[radar] test`
    /// command) for every overlapping pair before printing.
    #[arg(long)]
    check: bool,
    /// Keep watching: print overlaps as they appear (`+`) and go (`-`),
    /// running behaviour checks in the background.
    #[arg(long)]
    watch: bool,
    /// Seconds between scans with --watch.
    #[arg(long, default_value_t = 1.0, value_name = "SECS")]
    interval: f64,
    /// Print JSON (one object per line with --watch).
    #[arg(long)]
    json: bool,
}

/// One row of the report.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
struct Row {
    level: Level,
    grafts: [String; 2],
    path: Option<PathBuf>,
    symbols: Vec<String>,
    detail: String,
}

/// A whole report: rows, highest level first, and notes.
#[derive(Debug, Default, PartialEq, Eq, serde::Serialize)]
struct Report {
    overlaps: Vec<Row>,
    notes: Vec<String>,
}

/// The `[radar]` table at `base`, and whether its command is approved.
fn radar_spec(repo: &Repo, store: &Store, base: &CommitId) -> anyhow::Result<(RadarSpec, bool)> {
    let text = repo
        .read_file_at(base, Path::new(BLUEPRINT_FILE))?
        .map(String::from_utf8)
        .transpose()
        .context("graft.toml is not valid UTF-8")?
        .unwrap_or_default();
    let spec = parse_radar(&text, BLUEPRINT_FILE)?;
    let approved = match &spec.test {
        Some(t) => store.is_trusted(&approval_hash(t))?,
        None => false,
    };
    Ok((spec, approved))
}

/// The trust hash of a `[radar] test` command.
pub(crate) fn approval_hash(test: &str) -> graft_core::CommandHash {
    graft_root::layer::blueprint_item_hash("radar", &format!("test `{test}`"))
}

/// A live graft and what it changed. Contents are read only when needed:
/// a scan lists paths, and reads files that another graft also changed.
struct Live {
    record: GraftRecord,
    /// Changed paths; for virtual grafts, the full changes are already
    /// here (computed from the delta, without a tree scan).
    paths: Vec<PathBuf>,
    full: Option<GraftChanges>,
}

impl Live {
    fn changes(&self, files: Vec<FileChange>) -> GraftChanges {
        GraftChanges {
            graft: self.record.name.to_string(),
            base: self.record.base_commit.to_string(),
            files,
        }
    }

    /// The changes to `only` (every path if `None`), with contents.
    fn contents(
        &mut self,
        ctx: &Ctx,
        only: Option<&BTreeSet<PathBuf>>,
    ) -> anyhow::Result<GraftChanges> {
        let keep = |p: &PathBuf| only.is_none_or(|o| o.contains(p));
        if let Some(full) = &self.full {
            let files = full
                .files
                .iter()
                .filter(|f| keep(&f.path))
                .cloned()
                .collect();
            return Ok(self.changes(files));
        }
        let repo = GraftRepo::open(&ctx.backend.path(&self.record.name))?;
        let changes = repo
            .changes_since(&self.record.base_commit)?
            .into_iter()
            .filter(|c| keep(&c.path))
            .collect();
        let files: Vec<FileChange> = repo
            .diff_of(&self.record.base_commit, changes)?
            .into_iter()
            .map(|d| FileChange {
                path: d.change.path,
                old: d.old,
                new: d.new,
            })
            .collect();
        let g = self.changes(files);
        if only.is_none() {
            self.full = Some(g.clone());
        }
        Ok(g)
    }
}

/// Every live graft's changed paths.
fn collect(ctx: &Ctx, out: &mut Output) -> anyhow::Result<Vec<Live>> {
    let root_dir = ctx.root_dir();
    let mut all = Vec::new();
    for g in ctx.located.store.grafts()? {
        if g.state != GraftState::Live {
            continue;
        }
        let path = ctx.backend.path(&g.name);
        let found = if g.backend.is_virtual() {
            crate::virtual_grafts::diffs(
                &ctx.home,
                &ctx.located,
                &root_dir,
                &path,
                &g,
                &mut out.warnings,
            )
            .map(|diffs| {
                let files: Vec<FileChange> = diffs
                    .into_iter()
                    .map(|d| FileChange {
                        path: d.change.path,
                        old: d.old,
                        new: d.new,
                    })
                    .collect();
                (files.iter().map(|f| f.path.clone()).collect(), Some(files))
            })
        } else {
            GraftRepo::open(&path)
                .and_then(|r| r.changes_since(&g.base_commit))
                .map(|c| (c.into_iter().map(|c| c.path).collect(), None))
                .map_err(Into::into)
        };
        match found {
            Ok((paths, files)) => {
                let mut live = Live {
                    record: g,
                    paths,
                    full: None,
                };
                live.full = files.map(|f| live.changes(f));
                all.push(live);
            }
            Err(e) => out
                .warnings
                .push(format!("graft {}: cannot read its changes ({e:#})", g.name)),
        }
    }
    Ok(all)
}

/// A fingerprint of what a check runs: the command, base and changes.
fn fingerprint(test: &str, base: &str, sets: &[&GraftChanges]) -> String {
    let mut h = blake3::Hasher::new_derive_key("graft radar check v1");
    let mut field = |b: &[u8]| {
        h.update(&(b.len() as u64).to_le_bytes());
        h.update(b);
    };
    field(test.as_bytes());
    field(base.as_bytes());
    for s in sets {
        field(s.graft.as_bytes());
        let mut files: Vec<&FileChange> = s.files.iter().collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        for f in files {
            field(&graft_core::path_to_bytes(&f.path));
            match &f.new {
                Some(n) => field(n),
                None => field(b"\0deleted"),
            }
        }
    }
    h.finalize().to_hex().to_string()
}

/// Writes `changes` (one graft, or two merged) over a checkout of their
/// base. `Err` with the conflicting paths if a three-way merge fails.
fn apply_changes(dir: &Path, sets: &[&GraftChanges]) -> anyhow::Result<Result<(), Vec<PathBuf>>> {
    let mut merged: BTreeMap<&PathBuf, Vec<&FileChange>> = BTreeMap::new();
    for s in sets {
        for f in &s.files {
            merged.entry(&f.path).or_default().push(f);
        }
    }
    let mut conflicts = Vec::new();
    for (path, versions) in merged {
        let target = dir.join(path);
        let content = match versions.as_slice() {
            [one] => one.new.clone(),
            [a, b] if a.new == b.new => a.new.clone(),
            [a, b] => {
                let merged = three_way(dir, a.old.as_deref(), a.new.as_deref(), b.new.as_deref())?;
                if merged.is_none() {
                    conflicts.push(path.clone());
                    continue;
                }
                merged
            }
            _ => bail!("{} changed by more than two grafts", path.display()),
        };
        match content {
            Some(bytes) => {
                if let Some(p) = target.parent() {
                    std::fs::create_dir_all(p)
                        .with_context(|| format!("creating {}", p.display()))?;
                }
                std::fs::write(&target, bytes)
                    .with_context(|| format!("writing {}", target.display()))?;
            }
            None => match std::fs::remove_file(&target) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("removing {}", target.display())),
            },
        }
    }
    Ok(if conflicts.is_empty() {
        Ok(())
    } else {
        Err(conflicts)
    })
}

/// `git merge-file` of `ours` and `theirs` against `base`; `None` on a
/// conflict, or when one side deleted the file and the other changed it.
fn three_way(
    scratch: &Path,
    base: Option<&[u8]>,
    ours: Option<&[u8]>,
    theirs: Option<&[u8]>,
) -> anyhow::Result<Option<Vec<u8>>> {
    let (Some(ours), Some(theirs)) = (ours, theirs) else {
        return Ok(None);
    };
    let tmp = scratch.join(format!(".graft-merge-{}", GraftId::random()?));
    std::fs::create_dir(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
    let files = [
        ("ours", ours),
        ("base", base.unwrap_or_default()),
        ("theirs", theirs),
    ];
    for (n, c) in files {
        std::fs::write(tmp.join(n), c)?;
    }
    let out = Command::new("git")
        .args(["merge-file", "-p", "ours", "base", "theirs"])
        .current_dir(&tmp)
        .output()
        .context("running git merge-file");
    std::fs::remove_dir_all(&tmp).with_context(|| format!("removing {}", tmp.display()))?;
    let out = out?;
    match out.status.code() {
        Some(0) => Ok(Some(out.stdout)),
        Some(n) if n > 0 => Ok(None),
        _ => bail!(
            "git merge-file failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
    }
}

/// Runs one check: a scratch checkout of `base` with `sets` applied, then
/// the test command, confined, with a timeout.
fn run_check(
    ctx: &Ctx,
    base: &CommitId,
    sets: &[&GraftChanges],
    spec: &RadarSpec,
    test: &str,
) -> anyhow::Result<(CheckOutcome, String)> {
    let radar_dir = ctx.root_dir().join("radar");
    let backend = ReflinkBackend::new(radar_dir);
    let id = GraftId::random()?;
    let name: GraftName = format!("check-{}", &id.to_string()[..12]).parse()?;
    let created = backend.create(
        &ctx.located.repo,
        &GraftSpec {
            id,
            name: &name,
            base,
        },
    )?;
    let result = (|| {
        let dir = created.path.clone();
        if let Ok(planned) = crate::layers::plan(
            &ctx.located.repo,
            &ctx.located.store,
            &ctx.layers,
            base,
            None,
            None,
        ) {
            crate::layers::link_ready(&planned, &ctx.layers, &dir)?;
        }
        if let Err(conflicts) = apply_changes(&dir, sets)? {
            let list: Vec<String> = conflicts.iter().map(|p| p.display().to_string()).collect();
            return Ok((CheckOutcome::Conflict, list.join("\n")));
        }
        let tmp = dir.join(".graft-radar-tmp");
        crate::run::create_private_dir(&tmp)?;
        let policy = graft_sandbox::Policy::new([dir.as_path()])?;
        let log_path = tmp.join("output");
        let log = std::fs::File::create(&log_path)?;
        let command = ["-c", test].map(OsString::from);
        let mut cmd = crate::run::confined(&policy, &OsString::from("sh"), &command)?;
        cmd.current_dir(&dir)
            .env("TMPDIR", &tmp)
            .env("GRAFT_RADAR", "1")
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        let mut child = cmd.spawn().context("starting the radar test command")?;
        let deadline = Instant::now() + Duration::from_secs(spec.timeout_secs);
        let status = loop {
            if let Some(s) = child.try_wait()? {
                break Some(s);
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let output = std::fs::read(&log_path).unwrap_or_default();
        let tail =
            String::from_utf8_lossy(&output[output.len().saturating_sub(4000)..]).into_owned();
        Ok(match status {
            Some(s) if s.success() => (CheckOutcome::Pass, tail),
            Some(_) => (CheckOutcome::Fail, tail),
            None => (CheckOutcome::Timeout, tail),
        })
    })();
    backend.prune(&name, id)?;
    result
}

/// A cached check, or a fresh one if `run` (recorded either way).
fn check(
    ctx: &mut Ctx,
    base: &CommitId,
    sets: &[&GraftChanges],
    spec: &RadarSpec,
    test: &str,
    run: bool,
) -> anyhow::Result<Option<(CheckOutcome, String)>> {
    let fp = fingerprint(test, base.as_str(), sets);
    if let Some(c) = ctx.located.store.radar_check(&fp)? {
        return Ok(Some((c.outcome, c.output)));
    }
    if !run {
        return Ok(None);
    }
    let (outcome, output) = run_check(ctx, base, sets, spec, test)
        .unwrap_or_else(|e| (CheckOutcome::Error, format!("{e:#}")));
    ctx.located
        .store
        .record_radar_check(&fp, outcome, &output)?;
    Ok(Some((outcome, output)))
}

fn last_line(s: &str) -> &str {
    s.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
}

/// Every live graft, and the changes (with contents) of files more than
/// one of them changed.
fn scan(ctx: &Ctx, out: &mut Output) -> anyhow::Result<(Vec<Live>, Vec<GraftChanges>)> {
    let mut live = collect(ctx, out)?;
    let mut seen: BTreeMap<&PathBuf, usize> = BTreeMap::new();
    for l in &live {
        for p in &l.paths {
            *seen.entry(p).or_default() += 1;
        }
    }
    let shared: BTreeSet<PathBuf> = seen
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(p, _)| p.clone())
        .collect();
    let mut changes = Vec::new();
    for l in &mut live {
        if l.paths.iter().any(|p| shared.contains(p)) {
            changes.push(l.contents(ctx, Some(&shared))?);
        }
    }
    Ok((live, changes))
}

/// The behaviour-level rows for one overlapping pair with a common base.
/// `true` if a check it needs has not run yet.
#[allow(clippy::too_many_arguments)] // one call site; a struct would only rename them
fn behaviour_rows(
    ctx: &mut Ctx,
    base: &CommitId,
    [a, b]: [&GraftChanges; 2],
    pair: &[String; 2],
    spec: &RadarSpec,
    test: &str,
    run: bool,
    rows: &mut Vec<Row>,
    notes: &mut Vec<String>,
) -> anyhow::Result<bool> {
    let merged = check(ctx, base, &[a, b], spec, test, run)?;
    match merged {
        None => return Ok(true),
        Some((CheckOutcome::Conflict, files)) => {
            for f in files.lines() {
                rows.push(Row {
                    level: Level::Conflict,
                    grafts: pair.clone(),
                    path: Some(PathBuf::from(f)),
                    symbols: Vec::new(),
                    detail: "three-way merge conflicts".to_owned(),
                });
            }
        }
        Some((CheckOutcome::Fail | CheckOutcome::Timeout, output)) => {
            let alone_a = check(ctx, base, &[a], spec, test, run)?;
            let alone_b = check(ctx, base, &[b], spec, test, run)?;
            match (alone_a, alone_b) {
                (Some((CheckOutcome::Pass, _)), Some((CheckOutcome::Pass, _))) => {
                    rows.push(Row {
                        level: Level::Behaviour,
                        grafts: pair.clone(),
                        path: None,
                        symbols: Vec::new(),
                        detail: format!(
                            "`{test}` fails merged, passes alone: {}",
                            last_line(&output)
                        ),
                    });
                }
                (None, _) | (_, None) => return Ok(true),
                _ => notes.push(format!(
                    "{} + {}: `{test}` already fails in one of them alone",
                    pair[0], pair[1]
                )),
            }
        }
        Some((CheckOutcome::Error, why)) => {
            notes.push(format!(
                "{} + {}: behaviour check failed to run: {why}",
                pair[0], pair[1]
            ));
        }
        Some((CheckOutcome::Pass, _)) => {}
    }
    Ok(false)
}

/// Builds the report; runs behaviour checks that are not cached if `run`.
fn report(ctx: &mut Ctx, run: bool, out: &mut Output) -> anyhow::Result<Report> {
    let (mut live, changes) = scan(ctx, out)?;
    let found = overlaps(&changes);
    let mut rows: Vec<Row> = found
        .iter()
        .map(|o: &Overlap| Row {
            level: o.level,
            grafts: o.grafts.clone(),
            path: Some(o.path.clone()),
            symbols: o.symbols.clone(),
            detail: if o.symbols.is_empty() {
                String::new()
            } else {
                o.symbols.join(", ")
            },
        })
        .collect();
    let mut notes = Vec::new();
    let pairs: BTreeSet<[String; 2]> = found.iter().map(|o| o.grafts.clone()).collect();
    let index: BTreeMap<String, usize> = live
        .iter()
        .enumerate()
        .map(|(i, l)| (l.record.name.to_string(), i))
        .collect();
    let mut pending = 0;
    for pair in &pairs {
        let (ia, ib) = (index[&pair[0]], index[&pair[1]]);
        if live[ia].record.base_commit != live[ib].record.base_commit {
            notes.push(format!(
                "{} + {}: different bases, so no behaviour check",
                pair[0], pair[1]
            ));
            continue;
        }
        let base = live[ia].record.base_commit.clone();
        let (spec, approved) = radar_spec(&ctx.located.repo, &ctx.located.store, &base)?;
        let Some(test) = spec.test.clone() else {
            notes.push("behaviour checks are off: graft.toml has no [radar] test".to_owned());
            continue;
        };
        if !approved {
            notes.push(format!(
                "behaviour checks are off: the radar test `{test}` is not approved (graft trust)"
            ));
            continue;
        }
        let a = live[ia].contents(ctx, None)?;
        let b = live[ib].contents(ctx, None)?;
        if behaviour_rows(
            ctx,
            &base,
            [&a, &b],
            pair,
            &spec,
            &test,
            run,
            &mut rows,
            &mut notes,
        )? {
            pending += 1;
        }
    }
    if pending > 0 {
        notes.push(format!(
            "{pending} behaviour check(s) not run yet; use `graft radar --check`"
        ));
    }
    notes.dedup();
    rows.sort_by(|x, y| y.level.cmp(&x.level).then_with(|| x.cmp(y)));
    Ok(Report {
        overlaps: rows,
        notes,
    })
}

fn row_text(r: &Row) -> String {
    let path = r
        .path
        .as_ref()
        .map_or("—".to_owned(), |p| p.display().to_string());
    format!(
        "{:<10} {:<30} {:<30} {}",
        r.level.as_str(),
        format!("{} + {}", r.grafts[0], r.grafts[1]),
        path,
        r.detail
    )
    .trim_end()
    .to_owned()
}

fn print_report(r: &Report, json: bool) -> anyhow::Result<String> {
    if json {
        return Ok(serde_json::to_string(r)? + "\n");
    }
    let mut s = String::new();
    if r.overlaps.is_empty() {
        s.push_str("No overlaps between live grafts.\n");
    } else {
        let _ = writeln!(s, "{:<10} {:<30} {:<30} DETAIL", "LEVEL", "GRAFTS", "PATH");
        for row in &r.overlaps {
            let _ = writeln!(s, "{}", row_text(row));
        }
    }
    for n in &r.notes {
        let _ = writeln!(s, "note: {n}");
    }
    Ok(s)
}

/// `graft radar`.
pub(crate) fn radar(args: &RadarArgs, env: &Environment) -> anyhow::Result<Output> {
    let mut ctx = open(env)?;
    let mut out = Output::default();
    if !args.watch {
        let r = report(&mut ctx, args.check, &mut out)?;
        out.stdout = print_report(&r, args.json)?;
        return Ok(out);
    }
    watch(args, env, &mut ctx)
}

/// `graft radar --watch`: prints rows as they appear and go. Behaviour
/// checks run on a second thread, whose results the next scan picks up.
fn watch(args: &RadarArgs, env: &Environment, ctx: &mut Ctx) -> anyhow::Result<Output> {
    use std::io::Write as _;
    let interval = Duration::from_secs_f64(args.interval.max(0.1));
    let home = env.require_home()?.clone();
    let cwd = std::env::current_dir()?;
    let mut checker: Option<std::thread::JoinHandle<()>> = None;
    let mut shown: BTreeSet<Row> = BTreeSet::new();
    let mut notes_shown: BTreeSet<String> = BTreeSet::new();
    loop {
        let mut scratch = Output::default();
        let r = report(ctx, false, &mut scratch)?;
        let now: BTreeSet<Row> = r.overlaps.iter().cloned().collect();
        let mut lines = String::new();
        for row in now.difference(&shown) {
            if args.json {
                let _ = writeln!(
                    lines,
                    "{}",
                    serde_json::json!({"event": "added", "overlap": row})
                );
            } else {
                let _ = writeln!(lines, "+ {}", row_text(row));
            }
        }
        for row in shown.difference(&now) {
            if args.json {
                let _ = writeln!(
                    lines,
                    "{}",
                    serde_json::json!({"event": "removed", "overlap": row})
                );
            } else {
                let _ = writeln!(lines, "- {}", row_text(row));
            }
        }
        for n in r.notes.iter().filter(|n| !notes_shown.contains(*n)) {
            if !args.json {
                let _ = writeln!(lines, "note: {n}");
            }
        }
        notes_shown = r.notes.iter().cloned().collect();
        if !lines.is_empty() {
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(lines.as_bytes())?;
            stdout.flush()?;
        }
        shown = now;
        // Start the pending behaviour checks in the background.
        if checker
            .as_ref()
            .is_none_or(std::thread::JoinHandle::is_finished)
            && r.notes.iter().any(|n| n.contains("not run yet"))
        {
            let (home, cwd) = (home.clone(), cwd.clone());
            checker = Some(std::thread::spawn(move || {
                let run = || -> anyhow::Result<()> {
                    let located = graft_store::registry::locate(&home, &cwd)?;
                    let root_dir = home.root_dir(&located.record.id);
                    let mut c = Ctx {
                        backend: ReflinkBackend::new(root_dir.join("mounts")),
                        layers: graft_root::layer::LayerCache::new(root_dir.join("layers")),
                        home,
                        located,
                    };
                    report(&mut c, true, &mut Output::default())?;
                    Ok(())
                };
                if let Err(e) = run() {
                    tracing::warn!(error = %format!("{e:#}"), "radar behaviour checks failed");
                }
            }));
        }
        std::thread::sleep(interval);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn changes(name: &str, files: &[(&str, &str)]) -> GraftChanges {
        GraftChanges {
            graft: name.into(),
            base: "b".into(),
            files: files
                .iter()
                .map(|(p, c)| FileChange {
                    path: (*p).into(),
                    old: Some(b"one\ntwo\nthree\n".to_vec()),
                    new: Some(c.as_bytes().to_vec()),
                })
                .collect(),
        }
    }

    #[test]
    fn merges_disjoint_edits_and_reports_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let a = changes("a", &[("f", "ONE\ntwo\nthree\n"), ("only-a", "x")]);
        let b = changes("b", &[("f", "one\ntwo\nTHREE\n")]);
        apply_changes(dir.path(), &[&a, &b]).unwrap().unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("f")).unwrap(),
            "ONE\ntwo\nTHREE\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("only-a")).unwrap(),
            "x"
        );
        let c = changes("c", &[("f", "uno\ntwo\nthree\n")]);
        let conflicts = apply_changes(dir.path(), &[&a, &c]).unwrap().unwrap_err();
        assert_eq!(conflicts, [PathBuf::from("f")]);
    }

    #[test]
    fn fingerprints_follow_content() {
        let a = changes("a", &[("f", "x")]);
        let b = changes("a", &[("f", "y")]);
        assert_ne!(fingerprint("t", "b", &[&a]), fingerprint("t", "b", &[&b]));
        assert_ne!(fingerprint("t", "b", &[&a]), fingerprint("u", "b", &[&a]));
        assert_eq!(
            fingerprint("t", "b", &[&a]),
            fingerprint("t", "b", &[&a.clone()])
        );
    }
}
