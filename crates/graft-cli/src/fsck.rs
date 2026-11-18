//! `graft fsck`: check that a Root's stored state is consistent (ADR 0032).
//!
//! Read-only. It never repairs anything; each finding says what to do.
//! Checks, for the Root of the current directory:
//!
//! - the database: SQLite `integrity_check` and the schema version;
//! - each virtual graft: replaying its op log from empty reproduces its
//!   stored delta exactly (the Phase 6 invariant);
//! - every blob a delta or ring references exists and, unless `--quick`,
//!   hashes to its name;
//! - each unpruned graft's layers exist in the layer cache;
//! - each reflink graft has its working tree, and `mounts/` holds nothing
//!   that belongs to no graft.

use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

use graft_core::{BackendKind, BlobHash, GraftState};
use graft_fs::FsBackend as _;
use graft_root::tree::ObjectStore;
use graft_store::GraftRecord;
use graft_store::blobs::BlobStore;
use graft_store::delta::Delta;
use serde::Serialize;

use crate::{Environment, Output};

/// Arguments for `graft fsck`.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Check that blobs exist without re-hashing their contents.
    #[arg(long)]
    quick: bool,
    /// Print the findings as JSON.
    #[arg(long)]
    json: bool,
}

/// How bad a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
enum Severity {
    /// Data is missing or inconsistent.
    Error,
    /// Not wrong, but worth cleaning up.
    Warning,
}

/// One problem found.
#[derive(Debug, Serialize)]
struct Finding {
    severity: Severity,
    /// Which check found it.
    check: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    graft: Option<String>,
    message: String,
    /// What to do about it.
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

#[derive(Default)]
struct Findings(Vec<Finding>);

impl Findings {
    fn add(
        &mut self,
        severity: Severity,
        check: &'static str,
        graft: Option<&GraftRecord>,
        message: String,
        hint: Option<&str>,
    ) {
        self.0.push(Finding {
            severity,
            check,
            graft: graft.map(|g| g.name.to_string()),
            message,
            hint: hint.map(str::to_owned),
        });
    }
}

/// Counts of what was checked.
#[derive(Debug, Default, Serialize)]
struct Checked {
    grafts: usize,
    ops_replayed: usize,
    blobs: usize,
    blobs_hashed: usize,
    unreferenced_blobs: usize,
}

/// Runs `graft fsck`. Fails (exit 1) when any error is found.
///
/// # Errors
///
/// If the Root cannot be opened at all.
pub(crate) fn run(args: &Args, env: &Environment) -> anyhow::Result<(Output, ExitCode)> {
    let ctx = crate::grafts::open(env)?;
    let store = &ctx.located.store;
    let root_dir = ctx.home.root_dir(&ctx.located.record.id);
    let mut found = Findings::default();
    let mut checked = Checked::default();

    check_database(store, &mut found);
    let grafts: Vec<GraftRecord> = store
        .grafts()?
        .into_iter()
        .filter(|g| g.state != GraftState::Pruned)
        .collect();
    checked.grafts = grafts.len();
    let objects = ObjectStore::open(&ctx.located.repo);
    let mut warnings = Vec::new();
    for g in &grafts {
        if g.backend == BackendKind::Reflink {
            if !ctx.backend.path(&g.name).is_dir() {
                found.add(
                    Severity::Error,
                    "working_tree",
                    Some(g),
                    format!(
                        "its working tree {} is missing",
                        ctx.backend.path(&g.name).display()
                    ),
                    Some("graft prune --force NAME removes the record; its changes are gone"),
                );
            }
        } else {
            crate::virtual_grafts::sync(&ctx.home, &ctx.located, g, &mut warnings);
            checked.ops_replayed += check_replay(store, &objects, &root_dir, g, &mut found);
        }
        check_layers(store, &ctx.layers, g, &mut found);
    }
    check_blobs(store, &root_dir, args.quick, &mut checked, &mut found);
    check_mounts(&root_dir.join("mounts"), &grafts, &mut found);

    found
        .0
        .sort_by(|a, b| (a.severity, &a.graft).cmp(&(b.severity, &b.graft)));
    let errors = found
        .0
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    let text = render(&ctx.located.record, &checked, &found.0, args.quick, errors);
    if errors > 0 {
        crate::agent::report_failure(
            "fsck_failed",
            &format!("graft fsck found {errors} error(s)"),
            Some("see output; graft doctor --report bundles the details for a bug report"),
        );
    }

    let json = serde_json::json!({
        "root": ctx.located.record.id.to_string(),
        "ok": errors == 0,
        "checked": checked,
        "findings": found.0,
    });
    let mut out = Output::stdout(text).json_if(args.json, || json);
    out.warnings = warnings;
    let code = if errors == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    };
    Ok((out, code))
}

/// The text report.
fn render(
    root: &graft_store::RootRecord,
    checked: &Checked,
    found: &[Finding],
    quick: bool,
    errors: usize,
) -> String {
    let warns = found.len() - errors;
    let mut text = format!("graft fsck: Root {} ({})\n", root.id, root.path.display());
    let _ = writeln!(
        text,
        "checked {} graft(s), {} op(s) replayed, {} blob(s){}",
        checked.grafts,
        checked.ops_replayed,
        checked.blobs,
        if quick { " (not re-hashed)" } else { "" }
    );
    if checked.unreferenced_blobs > 0 {
        let _ = writeln!(
            text,
            "{} blob(s) no longer referenced (graft gc removes them)",
            checked.unreferenced_blobs
        );
    }
    for f in found {
        let sev = match f.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let who = f
            .graft
            .as_ref()
            .map(|g| format!("graft {g}: "))
            .unwrap_or_default();
        let _ = writeln!(text, "{sev}: {who}{}", f.message);
        if let Some(h) = &f.hint {
            let _ = writeln!(text, "  hint: {h}");
        }
    }
    let _ = writeln!(
        text,
        "{}",
        if found.is_empty() {
            "no problems found".to_owned()
        } else {
            format!("{errors} error(s), {warns} warning(s)")
        }
    );
    text
}

fn check_database(store: &graft_store::Store, found: &mut Findings) {
    match store.integrity_check() {
        Ok(problems) => {
            for p in problems {
                found.add(
                    Severity::Error,
                    "database",
                    None,
                    format!("graft.db: {p}"),
                    Some("graft doctor --report and open a bug report"),
                );
            }
        }
        Err(e) => found.add(
            Severity::Error,
            "database",
            None,
            format!("graft.db integrity check failed: {e}"),
            None,
        ),
    }
    if let Ok(v) = store.schema_version()
        && v != graft_store::SCHEMA_VERSION
    {
        found.add(
            Severity::Error,
            "database",
            None,
            format!(
                "graft.db is at schema v{v}, this graft expects v{}",
                graft_store::SCHEMA_VERSION
            ),
            None,
        );
    }
}

/// Replays `g`'s op log and compares it with its stored delta; returns the
/// number of ops replayed.
fn check_replay(
    store: &graft_store::Store,
    objects: &ObjectStore,
    root_dir: &Path,
    g: &GraftRecord,
    found: &mut Findings,
) -> usize {
    let read = || -> anyhow::Result<(Vec<graft_store::delta::Record>, Delta, Delta)> {
        let layers = graft_fs::layer_mounts(store, root_dir, g.id)?;
        let records = store.op_log(g.id)?;
        let stored = store.delta(g.id)?;
        let replayed = graft_fs::replayed_delta(objects, &g.base_commit, layers, &records)?;
        Ok((records, stored, replayed))
    };
    // A write can land between reading the op log and the delta: read
    // again once before reporting a mismatch.
    let mut result = read();
    if matches!(&result, Ok((_, stored, replayed)) if stored != replayed) {
        result = read();
    }
    match result {
        Ok((records, stored, replayed)) => {
            if stored != replayed {
                let differing = differing_paths(&stored, &replayed);
                found.add(
                    Severity::Error,
                    "op_log_replay",
                    Some(g),
                    format!(
                        "replaying its op log ({} ops) does not reproduce its delta at {} path(s): {}",
                        records.len(),
                        differing.len(),
                        preview(&differing)
                    ),
                    Some("graft doctor --report and open a bug report; do not prune or integrate it yet"),
                );
            }
            records.len()
        }
        Err(e) => {
            found.add(
                Severity::Error,
                "op_log_replay",
                Some(g),
                format!("cannot replay its op log: {e:#}"),
                None,
            );
            0
        }
    }
}

/// Paths whose entries differ between two deltas.
fn differing_paths(a: &Delta, b: &Delta) -> Vec<String> {
    let paths: BTreeSet<&Vec<u8>> = a.entries().chain(b.entries()).map(|(p, _)| p).collect();
    paths
        .into_iter()
        .filter(|p| a.get(p) != b.get(p))
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect()
}

fn preview(paths: &[String]) -> String {
    const SHOWN: usize = 5;
    let mut s = paths
        .iter()
        .take(SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if paths.len() > SHOWN {
        let _ = write!(s, ", and {} more", paths.len() - SHOWN);
    }
    s
}

fn check_layers(
    store: &graft_store::Store,
    cache: &graft_root::layer::LayerCache,
    g: &GraftRecord,
    found: &mut Findings,
) {
    let keys = match store.graft_layers(g.id) {
        Ok(k) => k,
        Err(e) => {
            found.add(
                Severity::Error,
                "layers",
                Some(g),
                format!("cannot list its layers: {e}"),
                None,
            );
            return;
        }
    };
    for key in keys {
        if !cache.path(&key).is_dir() {
            found.add(
                Severity::Error,
                "layers",
                Some(g),
                format!("layer {key} is referenced but missing from the layer cache"),
                Some("graft prune and recreate the graft; graft new rebuilds missing layers"),
            );
        }
    }
}

fn check_blobs(
    store: &graft_store::Store,
    root_dir: &Path,
    quick: bool,
    checked: &mut Checked,
    found: &mut Findings,
) {
    let blobs = BlobStore::new(root_dir.join("blobs"));
    let referenced = match store.referenced_blobs() {
        Ok(r) => r,
        Err(e) => {
            found.add(
                Severity::Error,
                "blobs",
                None,
                format!("cannot list referenced blobs: {e}"),
                None,
            );
            return;
        }
    };
    let mut sorted: Vec<&BlobHash> = referenced.iter().collect();
    sorted.sort();
    checked.blobs = sorted.len();
    let mut missing = Vec::new();
    let mut corrupt = Vec::new();
    for h in sorted {
        let path = blobs.path(h);
        if !path.is_file() {
            missing.push(h.to_string());
        } else if !quick {
            checked.blobs_hashed += 1;
            match graft_store::blobs::hash_file(&path) {
                Ok(actual) if actual == *h => {}
                Ok(_) | Err(_) => corrupt.push(h.to_string()),
            }
        }
    }
    let hint =
        "the files that use them cannot be read; graft doctor --report and open a bug report";
    if !missing.is_empty() {
        found.add(
            Severity::Error,
            "blobs",
            None,
            format!(
                "{} referenced blob(s) are missing: {}",
                missing.len(),
                preview(&missing)
            ),
            Some(hint),
        );
    }
    if !corrupt.is_empty() {
        found.add(
            Severity::Error,
            "blobs",
            None,
            format!(
                "{} blob(s) do not match their hash: {}",
                corrupt.len(),
                preview(&corrupt)
            ),
            Some(hint),
        );
    }
    // Superseded contents wait for `graft gc`: normal, so only counted.
    let stored = stored_blobs(blobs.dir());
    checked.unreferenced_blobs = stored.difference(&referenced).count();
}

/// The blobs under `dir` (`<aa>/<rest>` files; `tmp/` is skipped).
fn stored_blobs(dir: &Path) -> HashSet<BlobHash> {
    let mut out = HashSet::new();
    let Ok(shards) = std::fs::read_dir(dir) else {
        return out;
    };
    for shard in shards.flatten() {
        let name = shard.file_name();
        let Some(prefix) = name.to_str().filter(|n| n.len() == 2) else {
            continue;
        };
        let Ok(files) = std::fs::read_dir(shard.path()) else {
            continue;
        };
        for f in files.flatten() {
            if let Some(rest) = f.file_name().to_str()
                && let Ok(h) = format!("{prefix}{rest}").parse()
            {
                out.insert(h);
            }
        }
    }
    out
}

/// Entries in `mounts/` that belong to no unpruned graft. A pruned reflink
/// graft's tree awaiting deletion (`.trash-*`) is expected.
fn check_mounts(mounts: &Path, grafts: &[GraftRecord], found: &mut Findings) {
    let Ok(entries) = std::fs::read_dir(mounts) else {
        return;
    };
    let names: HashSet<String> = grafts.iter().map(|g| g.name.to_string()).collect();
    let mut orphans: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter(|n| !n.starts_with(".trash-") && !names.contains(n))
        .collect();
    orphans.sort();
    if !orphans.is_empty() {
        found.add(
            Severity::Warning,
            "mounts",
            None,
            format!(
                "{} entr(ies) in {} belong to no graft: {}",
                orphans.len(),
                mounts.display(),
                preview(&orphans)
            ),
            Some("left by an interrupted create or prune; check nothing you need is inside, then delete them"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_at_most_five() {
        let p: Vec<String> = (0..7).map(|i| i.to_string()).collect();
        assert_eq!(preview(&p), "0, 1, 2, 3, 4, and 2 more");
        assert_eq!(preview(&p[..2]), "0, 1");
    }

    #[test]
    fn lists_stored_blobs_and_skips_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::new(dir.path().to_owned());
        let h = store.put_bytes(b"hello").unwrap();
        std::fs::create_dir_all(dir.path().join("tmp")).unwrap();
        std::fs::write(dir.path().join("tmp/partial"), "x").unwrap();
        assert_eq!(stored_blobs(dir.path()), HashSet::from([h]));
    }
}
