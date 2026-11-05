//! Workloads run *inside* a workspace (AGENTS.md Phase 8, "Measure first").
//!
//! The create matrix measures how fast an agent gets a workspace; these
//! measure every minute it then spends in one: building, testing, running
//! git, searching, walking and writing. Each method gets one ready
//! workspace per run (setup is not timed), and every workload runs one
//! discarded warm-up and then `samples` timed repetitions. Methodology:
//! ADR 0010.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, bail};

use crate::methods::Method;
use crate::report::{Stat, TIME_FLOOR_S, TOLERANCE};

/// One in-graft workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workload {
    /// `npm run build` from an empty `dist/`, then `npm test`.
    BuildCold,
    /// `npm run build` after a one-file edit (one bundle rewritten).
    Rebuild,
    /// `npm test` alone.
    Test,
    /// `git status` with one modified file.
    GitStatus,
    /// `git diff` of that file.
    GitDiff,
    /// `git commit -a` of that change (undone, untimed, after each sample).
    GitCommit,
    /// `rg` over the whole tree.
    Search,
    /// `find` of every file, `node_modules` included.
    Find,
    /// `ls -lR`: a stat of every file.
    LsR,
    /// 10,000 files of 1 KiB in 100 directories.
    WriteSmall,
    /// One 1 GiB file, written in 1 MiB chunks and `fsync`ed.
    WriteLarge,
}

impl Workload {
    /// Every workload, in the order they run. Git workloads run before the
    /// write workloads, so their tree holds exactly one change.
    pub const ALL: [Self; 11] = [
        Self::BuildCold,
        Self::Rebuild,
        Self::Test,
        Self::GitStatus,
        Self::GitDiff,
        Self::GitCommit,
        Self::Search,
        Self::Find,
        Self::LsR,
        Self::WriteSmall,
        Self::WriteLarge,
    ];

    /// The name used in results and on the command line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::BuildCold => "build-cold",
            Self::Rebuild => "rebuild",
            Self::Test => "test",
            Self::GitStatus => "git-status",
            Self::GitDiff => "git-diff",
            Self::GitCommit => "git-commit",
            Self::Search => "rg",
            Self::Find => "find",
            Self::LsR => "ls-R",
            Self::WriteSmall => "write-10k-small",
            Self::WriteLarge => "write-1gib",
        }
    }

    /// Looks a workload up by [`Self::name`].
    ///
    /// # Errors
    ///
    /// If no workload has that name.
    pub fn parse(name: &str) -> anyhow::Result<Self> {
        Self::ALL
            .into_iter()
            .find(|w| w.name() == name)
            .with_context(|| {
                format!(
                    "unknown workload {name:?}; expected one of {}",
                    Self::ALL.map(Self::name).join(", ")
                )
            })
    }

    fn order(name: &str) -> usize {
        Self::ALL
            .iter()
            .position(|w| w.name() == name)
            .unwrap_or(usize::MAX)
    }
}

/// The file every edit-driven workload touches.
const EDITED: &str = "src/d0000/m000000.js";
const WRITES_DIR: &str = "bench-writes";

fn quiet(program: &str, dir: &Path, args: &[&str]) -> Command {
    let mut c = crate::command(program, dir);
    c.args(args).stdout(Stdio::null());
    c
}

fn ok(cmd: &mut Command, what: &str) -> anyhow::Result<()> {
    let out = cmd
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("running {what}"))?;
    if !out.status.success() {
        bail!(
            "{what} failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Appends a line to the edited module (its exports are unchanged, so
/// `npm test` still passes).
fn edit(ws: &Path, n: usize) -> anyhow::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(ws.join(EDITED))
        .with_context(|| format!("editing {EDITED}"))?;
    writeln!(f, "// graft-bench edit {n}")?;
    Ok(())
}

/// Puts `ws` in the state `w` expects. Not timed.
fn before(w: Workload, ws: &Path, n: usize) -> anyhow::Result<()> {
    match w {
        Workload::BuildCold => match std::fs::remove_dir_all(ws.join("dist")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        },
        Workload::Rebuild | Workload::GitCommit => edit(ws, n),
        Workload::GitStatus | Workload::GitDiff => {
            // Exactly one modified file: undo earlier edits, then edit once.
            ok(
                &mut quiet("git", ws, &["checkout", "--quiet", "--", EDITED]),
                "git checkout",
            )?;
            edit(ws, n)
        }
        _ => Ok(()),
    }
}

/// Undoes what a timed sample left behind. Not timed.
fn after(w: Workload, ws: &Path) -> anyhow::Result<()> {
    match w {
        Workload::GitCommit => ok(
            &mut quiet("git", ws, &["reset", "--quiet", "--soft", "HEAD~1"]),
            "git reset",
        ),
        Workload::WriteSmall | Workload::WriteLarge => {
            std::fs::remove_dir_all(ws.join(WRITES_DIR)).context("removing written files")
        }
        _ => Ok(()),
    }
}

fn write_small(ws: &Path) -> anyhow::Result<()> {
    let root = ws.join(WRITES_DIR);
    let data = [b'x'; 1024];
    for d in 0..100 {
        let dir = root.join(format!("d{d:02}"));
        std::fs::create_dir_all(&dir)?;
        for f in 0..100 {
            std::fs::write(dir.join(format!("f{f:02}")), data)?;
        }
    }
    Ok(())
}

fn write_large(ws: &Path) -> anyhow::Result<()> {
    let root = ws.join(WRITES_DIR);
    std::fs::create_dir_all(&root)?;
    let mut f = std::fs::File::create(root.join("large.bin"))?;
    let chunk = vec![0x5a_u8; 1 << 20];
    for _ in 0..1024 {
        f.write_all(&chunk)?;
    }
    f.sync_all()?;
    Ok(())
}

/// The timed part of `w`.
fn timed(w: Workload, ws: &Path) -> anyhow::Result<()> {
    match w {
        Workload::BuildCold => {
            ok(
                &mut quiet("npm", ws, &["run", "build", "--silent"]),
                "npm run build",
            )?;
            ok(&mut quiet("npm", ws, &["test", "--silent"]), "npm test")
        }
        Workload::Rebuild => ok(
            &mut quiet("npm", ws, &["run", "build", "--silent"]),
            "npm run build",
        ),
        Workload::Test => ok(&mut quiet("npm", ws, &["test", "--silent"]), "npm test"),
        Workload::GitStatus => ok(
            &mut quiet("git", ws, &["status", "--porcelain"]),
            "git status",
        ),
        Workload::GitDiff => ok(&mut quiet("git", ws, &["--no-pager", "diff"]), "git diff"),
        Workload::GitCommit => ok(
            &mut quiet("git", ws, &["commit", "--quiet", "-a", "-m", "graft-bench"]),
            "git commit",
        ),
        Workload::Search => {
            // Exit 1 means no match; both are a completed search.
            let status = quiet("rg", ws, &["--count-matches", "a1b2c3", "."])
                .status()
                .context("running rg")?;
            match status.code() {
                Some(0 | 1) => Ok(()),
                _ => bail!("rg failed ({status})"),
            }
        }
        Workload::Find => ok(
            &mut quiet(
                "find",
                ws,
                &[".", "-name", ".git", "-prune", "-o", "-type", "f", "-print"],
            ),
            "find",
        ),
        Workload::LsR => ok(&mut quiet("ls", ws, &["-lR"]), "ls -lR"),
        Workload::WriteSmall => write_small(ws),
        Workload::WriteLarge => write_large(ws),
    }
}

/// Why `w` cannot run on this machine, if it cannot.
fn unavailable(w: Workload) -> Option<String> {
    (w == Workload::Search && Command::new("rg").arg("--version").output().is_err())
        .then(|| "skipped: rg is not installed".to_owned())
}

/// One workload's samples in one run.
#[derive(Debug, Clone, PartialEq)]
pub struct Samples {
    /// Median over the timed samples.
    pub median_s: f64,
    /// Fastest sample.
    pub min_s: f64,
    /// Slowest sample.
    pub max_s: f64,
    /// Number of timed samples.
    pub count: usize,
}

fn measure_one(w: Workload, ws: &Path, samples: usize) -> anyhow::Result<Samples> {
    let mut times = Vec::with_capacity(samples);
    // Sample 0 is a discarded warm-up: the first `git status` after a
    // checkout refreshes the index, the first build fills caches, and so on.
    for n in 0..=samples {
        before(w, ws, n)?;
        let t0 = Instant::now();
        timed(w, ws)?;
        let t = t0.elapsed().as_secs_f64();
        after(w, ws)?;
        if n > 0 {
            times.push(t);
        }
    }
    let (min_s, max_s) = times
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| {
            (a.min(x), b.max(x))
        });
    Ok(Samples {
        median_s: crate::median(times),
        min_s,
        max_s,
        count: samples,
    })
}

/// One row of `workloads.csv`.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkloadRow {
    /// 1-based run number.
    pub run: usize,
    /// Fixture name.
    pub fixture: String,
    /// Method name.
    pub method: String,
    /// Workload name.
    pub workload: String,
    /// The samples, or why there are none.
    pub outcome: Result<Samples, String>,
}

/// CSV header, matching [`WorkloadRow::to_csv`].
pub const CSV_HEADER: &str = "run,fixture,method,workload,median_s,min_s,max_s,samples,status";

impl WorkloadRow {
    /// The row as a CSV line (no newline).
    #[must_use]
    pub fn to_csv(&self) -> String {
        let head = format!(
            "{},{},{},{}",
            self.run, self.fixture, self.method, self.workload
        );
        match &self.outcome {
            Ok(s) => format!(
                "{head},{:.4},{:.4},{:.4},{},ok",
                s.median_s, s.min_s, s.max_s, s.count
            ),
            Err(why) => format!("{head},,,,,{}", crate::report::csv_field(why)),
        }
    }

    /// Parses a line written by [`WorkloadRow::to_csv`].
    ///
    /// # Errors
    ///
    /// If the line is malformed.
    pub fn from_csv(line: &str) -> anyhow::Result<Self> {
        let f = crate::report::split_csv(line);
        anyhow::ensure!(f.len() == 9, "expected 9 fields, got {}: {line}", f.len());
        let outcome = if f[8] == "ok" {
            Ok(Samples {
                median_s: f[4].parse()?,
                min_s: f[5].parse()?,
                max_s: f[6].parse()?,
                count: f[7].parse()?,
            })
        } else {
            Err(f[8].clone())
        };
        Ok(Self {
            run: f[0].parse()?,
            fixture: f[1].clone(),
            method: f[2].clone(),
            workload: f[3].clone(),
            outcome,
        })
    }
}

/// Reads every row of a `workloads.csv`.
///
/// # Errors
///
/// If the file cannot be read or a row is malformed.
pub fn read_csv(path: &Path) -> anyhow::Result<Vec<WorkloadRow>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    text.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(WorkloadRow::from_csv)
        .collect()
}

/// A workspace ready for workloads: created, with `node_modules` installed
/// where the method needs it, and a passing test.
///
/// # Errors
///
/// If creating or preparing the workspace fails.
pub fn prepare(method: &mut dyn Method) -> anyhow::Result<PathBuf> {
    let ws = method.create(0)?;
    if method.needs_install() {
        ok(
            &mut quiet(
                "npm",
                &ws,
                &[
                    "ci",
                    "--offline",
                    "--no-audit",
                    "--no-fund",
                    "--ignore-scripts",
                ],
            ),
            "npm ci",
        )?;
    }
    ok(&mut quiet("npm", &ws, &["test", "--silent"]), "npm test")?;
    Ok(ws)
}

/// Runs `workloads` in the ready workspace `ws`, calling `emit` with each
/// result as it completes. A failing workload is recorded, and the rest
/// still run.
///
/// # Errors
///
/// Only if `emit` fails.
pub fn run_all(
    ws: &Path,
    workloads: &[Workload],
    samples: usize,
    mut emit: impl FnMut(Workload, Result<Samples, String>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    for &w in workloads {
        let outcome = match unavailable(w) {
            Some(why) => Err(why),
            None => measure_one(w, ws, samples).map_err(|e| format!("error: {e:#}")),
        };
        if outcome.is_err() {
            // Leave the next workload a clean tree.
            let _ = std::fs::remove_dir_all(ws.join(WRITES_DIR));
        }
        emit(w, outcome)?;
    }
    Ok(())
}

fn fmt_secs(s: f64) -> String {
    if s < 1.0 {
        format!("{:.0} ms", s * 1000.0)
    } else {
        format!("{s:.2} s")
    }
}

/// Medians over runs, with spreads, of one method's workload.
///
/// With one run, the spread is that run's sample range instead.
#[must_use]
pub fn stat(rows: &[&WorkloadRow]) -> Option<Stat> {
    let ok: Vec<&Samples> = rows
        .iter()
        .filter_map(|r| r.outcome.as_ref().ok())
        .collect();
    match ok.as_slice() {
        [] => None,
        [one] => Some(Stat {
            median: one.median_s,
            spread: if one.median_s > 0.0 {
                (one.max_s - one.min_s) / one.median_s
            } else {
                0.0
            },
            runs: 1,
        }),
        many => Some(Stat::of(
            &many.iter().map(|s| s.median_s).collect::<Vec<_>>(),
        )),
    }
}

/// Renders `workloads.md`: per fixture, one row per workload and one column
/// per method.
#[must_use]
pub fn summary(rows: &[WorkloadRow], header: &str) -> String {
    let mut out = String::from(header);
    let mut fixtures: Vec<&str> = rows.iter().map(|r| r.fixture.as_str()).collect();
    fixtures.sort_by_key(|f| crate::report::fixture_order(f));
    fixtures.dedup();
    let mut methods: Vec<&str> = rows.iter().map(|r| r.method.as_str()).collect();
    methods.sort_by_key(|m| crate::report::method_order(m));
    methods.dedup();
    let mut held = 0;
    let mut within = 0;
    for fixture in fixtures {
        let _ = write!(
            out,
            "\n## {fixture}\n\n| workload | {} |\n| --- |{}\n",
            methods.join(" | "),
            " ---: |".repeat(methods.len())
        );
        let mut workloads: Vec<&str> = rows
            .iter()
            .filter(|r| r.fixture == fixture)
            .map(|r| r.workload.as_str())
            .collect();
        workloads.sort_by_key(|w| Workload::order(w));
        workloads.dedup();
        for w in workloads {
            let _ = write!(out, "| {w} |");
            for m in &methods {
                let cell: Vec<&WorkloadRow> = rows
                    .iter()
                    .filter(|r| r.fixture == fixture && r.method == *m && r.workload == w)
                    .collect();
                if let Some(s) = stat(&cell) {
                    let mut text = fmt_secs(s.median);
                    let pct = s.spread * 100.0;
                    if s.held(TIME_FLOOR_S) {
                        held += 1;
                        if s.spread <= TOLERANCE {
                            within += 1;
                            let _ = write!(text, " ±{pct:.0}%");
                        } else {
                            let _ = write!(text, " ±{pct:.0}% ⚠");
                        }
                    } else {
                        let _ = write!(text, " (±{pct:.0}%)");
                    }
                    let _ = write!(out, " {text} |");
                } else {
                    let why = cell
                        .iter()
                        .find_map(|r| r.outcome.as_ref().err())
                        .map_or("—", String::as_str);
                    let _ = write!(out, " {why} |");
                }
            }
            out.push('\n');
        }
    }
    let runs = rows.iter().map(|r| r.run).max().unwrap_or(0);
    let _ = write!(out, "\n## Reproducibility\n\n");
    if runs < 2 {
        out.push_str("Single run: spreads in parentheses are the range of that run's samples.\n");
    } else {
        let _ = writeln!(
            out,
            "{within} of {held} values are within ±{:.0}% across {runs} runs (others marked ⚠). \
             Values below {:.0} ms, or from a single run, show the spread in parentheses \
             and are not counted.",
            TOLERANCE * 100.0,
            TIME_FLOOR_S * 1000.0
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(run: usize, method: &str, workload: &str, median: f64) -> WorkloadRow {
        WorkloadRow {
            run,
            fixture: "medium".into(),
            method: method.into(),
            workload: workload.into(),
            outcome: Ok(Samples {
                median_s: median,
                min_s: median * 0.9,
                max_s: median * 1.2,
                count: 5,
            }),
        }
    }

    #[test]
    fn names_round_trip() {
        for w in Workload::ALL {
            assert_eq!(Workload::parse(w.name()).unwrap(), w);
        }
        assert!(Workload::parse("nope").is_err());
    }

    #[test]
    fn csv_round_trips() {
        let r = row(1, "graft", "git-status", 0.25);
        assert_eq!(WorkloadRow::from_csv(&r.to_csv()).unwrap(), r);
        let skipped = WorkloadRow {
            outcome: Err("skipped: rg is not installed, \"really\"".into()),
            ..r
        };
        assert_eq!(WorkloadRow::from_csv(&skipped.to_csv()).unwrap(), skipped);
        assert_eq!(CSV_HEADER.split(',').count(), 9);
    }

    #[test]
    fn single_run_spread_is_the_sample_range() {
        let r = row(1, "graft", "test", 1.0);
        let s = stat(&[&r]).unwrap();
        assert!((s.spread - 0.3).abs() < 1e-9);
        assert_eq!(s.runs, 1);
    }

    #[test]
    fn summary_puts_methods_in_columns() {
        let rows = vec![
            row(1, "worktree", "test", 1.0),
            row(1, "graft", "test", 0.5),
            row(1, "graft", "build-cold", 2.0),
        ];
        let md = summary(&rows, "# W\n");
        assert!(md.contains("| workload | graft | worktree |"), "{md}");
        // build-cold sorts before test, whatever the input order.
        let build = md.find("| build-cold |").unwrap();
        assert!(build < md.find("| test |").unwrap(), "{md}");
        assert!(md.contains("| build-cold | 2.00 s (±30%) | — |"), "{md}");
    }

    /// The real workloads against a plain git checkout of a tiny Node
    /// project, so every `before`/`timed`/`after` step is exercised.
    #[test]
    fn every_workload_runs_and_leaves_the_tree_clean() {
        if Command::new("npm").arg("--version").output().is_err() {
            assert!(
                std::env::var_os("GRAFT_E2E_REQUIRE_NPM").is_none(),
                "npm is required but missing"
            );
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let fixture = crate::fixture::ensure(dir.path(), &crate::fixture::TINY).unwrap();
        ok(
            &mut quiet(
                "npm",
                &fixture,
                &[
                    "ci",
                    "--offline",
                    "--no-audit",
                    "--no-fund",
                    "--ignore-scripts",
                ],
            ),
            "npm ci",
        )
        .unwrap();
        // Skip the 1 GiB write in a unit test.
        let workloads: Vec<Workload> = Workload::ALL
            .into_iter()
            .filter(|w| *w != Workload::WriteLarge)
            .collect();
        let mut seen = Vec::new();
        run_all(&fixture, &workloads, 1, |w, outcome| {
            if w != Workload::Search || outcome.is_ok() {
                assert!(outcome.is_ok(), "{}: {outcome:?}", w.name());
            }
            seen.push(w);
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, workloads);
        assert!(!fixture.join(WRITES_DIR).exists());
        let log = Command::new("git")
            .args(["rev-list", "--count", "HEAD"])
            .current_dir(&fixture)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "1");
        assert!(fixture.join("dist/d0000.js").exists());
    }
}
