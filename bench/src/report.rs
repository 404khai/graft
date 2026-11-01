//! Results: one CSV row per measured cell, and a Markdown summary.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::Context;

/// What one cell (fixture × method × agents, in one run) measured.
#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    /// Median time to create one workspace.
    pub create_median_s: f64,
    /// Time to create all of them, one after another.
    pub create_total_s: f64,
    /// For the first agent: create + install (if needed) + passing test.
    pub ready_first_s: f64,
    /// The same, median over agents 2..N (equal to the first for N = 1).
    pub ready_median_s: f64,
    /// Drop in the filesystem's free space with all workspaces present.
    pub disk_added_bytes: i64,
    /// Median time to remove one workspace.
    pub prune_median_s: f64,
}

/// One row of results.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// 1-based run number.
    pub run: usize,
    /// Fixture name.
    pub fixture: String,
    /// Tracked files in the fixture.
    pub files: usize,
    /// Method name.
    pub method: String,
    /// Number of agents (workspaces).
    pub agents: usize,
    /// The measurements, or why the cell was not measured.
    pub outcome: Result<Metrics, String>,
}

/// CSV header, matching [`Row::to_csv`].
pub const CSV_HEADER: &str = "run,fixture,tracked_files,method,agents,create_median_s,create_total_s,ready_first_s,ready_median_s,disk_added_bytes,prune_median_s,status";

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

impl Row {
    /// The row as a CSV line (no newline).
    #[must_use]
    pub fn to_csv(&self) -> String {
        let head = format!(
            "{},{},{},{},{}",
            self.run, self.fixture, self.files, self.method, self.agents
        );
        match &self.outcome {
            Ok(m) => format!(
                "{head},{:.4},{:.4},{:.4},{:.4},{},{:.4},ok",
                m.create_median_s,
                m.create_total_s,
                m.ready_first_s,
                m.ready_median_s,
                m.disk_added_bytes,
                m.prune_median_s
            ),
            Err(why) => format!("{head},,,,,,,{}", csv_field(why)),
        }
    }

    /// Parses a line written by [`Row::to_csv`].
    ///
    /// # Errors
    ///
    /// If the line is malformed.
    pub fn from_csv(line: &str) -> anyhow::Result<Self> {
        let fields = split_csv(line);
        anyhow::ensure!(
            fields.len() == 12,
            "expected 12 fields, got {}: {line}",
            fields.len()
        );
        let num = |i: usize| -> anyhow::Result<f64> {
            fields[i]
                .parse()
                .with_context(|| format!("field {i} of {line}"))
        };
        let outcome = if fields[11] == "ok" {
            Ok(Metrics {
                create_median_s: num(5)?,
                create_total_s: num(6)?,
                ready_first_s: num(7)?,
                ready_median_s: num(8)?,
                disk_added_bytes: fields[9].parse()?,
                prune_median_s: num(10)?,
            })
        } else {
            Err(fields[11].clone())
        };
        Ok(Self {
            run: fields[0].parse()?,
            fixture: fields[1].clone(),
            files: fields[2].parse()?,
            method: fields[3].clone(),
            agents: fields[4].parse()?,
            outcome,
        })
    }
}

fn split_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Reads every row of a results CSV.
///
/// # Errors
///
/// If the file cannot be read or a row is malformed.
pub fn read_csv(path: &Path) -> anyhow::Result<Vec<Row>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    text.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(Row::from_csv)
        .collect()
}

/// Values below these are reported but not held to the ±10% criterion:
/// at that scale, scheduler and filesystem jitter dominate.
pub const TIME_FLOOR_S: f64 = 0.1;
/// See [`TIME_FLOOR_S`].
pub const DISK_FLOOR_BYTES: f64 = 1024.0 * 1024.0;

/// The reproducibility tolerance from AGENTS.md Phase 4.
pub const TOLERANCE: f64 = 0.10;

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n == 0 {
        return f64::NAN;
    }
    if n % 2 == 1 {
        values[n / 2]
    } else {
        f64::midpoint(values[n / 2 - 1], values[n / 2])
    }
}

/// Median and relative spread ((max - min) / median) of a metric over runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stat {
    /// Median over runs.
    pub median: f64,
    /// (max - min) / median; 0 for a single run.
    pub spread: f64,
    /// Number of runs.
    pub runs: usize,
}

impl Stat {
    /// Summarises `values`.
    #[must_use]
    pub fn of(values: &[f64]) -> Self {
        let mut v = values.to_vec();
        let median = median(&mut v);
        let (min, max) = v
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| {
                (a.min(x), b.max(x))
            });
        let spread = if v.len() < 2 || median == 0.0 {
            0.0
        } else {
            (max - min) / median.abs()
        };
        Self {
            median,
            spread,
            runs: v.len(),
        }
    }

    /// Whether this value is held to the tolerance (above the noise floor).
    #[must_use]
    pub fn held(&self, floor: f64) -> bool {
        self.runs > 1 && self.median.abs() >= floor
    }
}

fn fmt_secs(s: f64) -> String {
    if s < 1.0 {
        format!("{:.0} ms", s * 1000.0)
    } else {
        format!("{s:.2} s")
    }
}

fn fmt_bytes(b: f64) -> String {
    let neg = b < 0.0;
    // Display only; rounding is intended.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let s = graft_fs::human_bytes(b.abs().round() as u64);
    if neg { format!("−{s}") } else { s }
}

fn cell(stat: &Stat, floor: f64, fmt: fn(f64) -> String) -> String {
    let mut s = fmt(stat.median);
    if stat.runs > 1 {
        let pct = stat.spread * 100.0;
        if stat.held(floor) {
            let _ = write!(s, " ±{pct:.0}%");
            if stat.spread > TOLERANCE {
                s.push_str(" ⚠");
            }
        } else {
            let _ = write!(s, " (±{pct:.0}%, below floor)");
        }
    }
    s
}

#[derive(Default)]
struct Group {
    create: Vec<f64>,
    ready_first: Vec<f64>,
    ready_median: Vec<f64>,
    disk: Vec<f64>,
    prune: Vec<f64>,
    errors: Vec<String>,
    files: usize,
}

fn method_order(m: &str) -> usize {
    ["graft", "worktree", "clone"]
        .iter()
        .position(|x| *x == m)
        .unwrap_or(9)
}

fn fixture_order(f: &str) -> usize {
    ["small", "medium", "large"]
        .iter()
        .position(|x| *x == f)
        .unwrap_or(9)
}

/// How many summary values are held to the tolerance, and how they fared.
#[derive(Default)]
struct Counts {
    held: usize,
    within: usize,
    below: usize,
}

/// One table row for a measured group.
fn render_row(out: &mut String, method: &str, agents: usize, g: &Group, counts: &mut Counts) {
    let stats = [
        (Stat::of(&g.create), TIME_FLOOR_S),
        (Stat::of(&g.ready_first), TIME_FLOOR_S),
        (Stat::of(&g.ready_median), TIME_FLOOR_S),
        (Stat::of(&g.disk), DISK_FLOOR_BYTES),
        (Stat::of(&g.prune), TIME_FLOOR_S),
    ];
    for (s, floor) in &stats {
        if s.held(*floor) {
            counts.held += 1;
            if s.spread <= TOLERANCE {
                counts.within += 1;
            }
        } else if s.runs > 1 {
            counts.below += 1;
        }
    }
    #[allow(clippy::cast_precision_loss)] // agent counts are tiny
    let per_agent = stats[3].0.median / agents as f64;
    let _ = writeln!(
        out,
        "| {method} | {agents} | {} | {} | {} | {} | {} | {} |",
        cell(&stats[0].0, TIME_FLOOR_S, fmt_secs),
        cell(&stats[1].0, TIME_FLOOR_S, fmt_secs),
        cell(&stats[2].0, TIME_FLOOR_S, fmt_secs),
        cell(&stats[3].0, DISK_FLOOR_BYTES, fmt_bytes),
        fmt_bytes(per_agent),
        cell(&stats[4].0, TIME_FLOOR_S, fmt_secs),
    );
    if !g.errors.is_empty() {
        let _ = writeln!(out, "| | | errors: {} | | | | | |", g.errors.join("; "));
    }
}

/// Renders the Markdown summary for `rows`, with `header` (machine and
/// method details) at the top.
#[must_use]
pub fn summary(rows: &[Row], header: &str) -> String {
    let mut groups: BTreeMap<(usize, String, usize, usize), Group> = BTreeMap::new();
    for r in rows {
        let key = (
            fixture_order(&r.fixture),
            r.fixture.clone(),
            method_order(&r.method),
            r.agents,
        );
        let g = groups.entry(key).or_default();
        g.files = r.files;
        match &r.outcome {
            Ok(m) => {
                g.create.push(m.create_median_s);
                g.ready_first.push(m.ready_first_s);
                g.ready_median.push(m.ready_median_s);
                #[allow(clippy::cast_precision_loss)] // bytes of disk: far below 2^53
                g.disk.push(m.disk_added_bytes as f64);
                g.prune.push(m.prune_median_s);
            }
            Err(e) => g.errors.push(format!("run {}: {e}", r.run)),
        }
    }
    let method_name = |order: usize| {
        ["graft", "worktree", "clone"]
            .get(order)
            .copied()
            .unwrap_or("?")
    };

    let mut out = String::from(header);
    let mut counts = Counts::default();
    let mut current_fixture = String::new();
    for ((_, fixture, method, agents), g) in &groups {
        if *fixture != current_fixture {
            current_fixture.clone_from(fixture);
            let _ = write!(
                out,
                "\n## {fixture} ({} tracked files)\n\n\
                 | method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |\n\
                 | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
                g.files
            );
        }
        if g.create.is_empty() {
            let _ = writeln!(
                out,
                "| {} | {agents} | not measured: {} | | | | | |",
                method_name(*method),
                g.errors.join("; ")
            );
            continue;
        }
        render_row(&mut out, method_name(*method), *agents, g, &mut counts);
    }

    let runs = rows.iter().map(|r| r.run).max().unwrap_or(0);
    let _ = write!(out, "\n## Reproducibility\n\n");
    if runs < 2 {
        out.push_str("Single run: no spread to report.\n");
    } else {
        let _ = writeln!(
            out,
            "{} of {} values are within ±{:.0}% across {runs} runs ({} outside, marked ⚠). \
             {} values below the noise floor ({:.0} ms, {}) are shown but not counted.",
            counts.within,
            counts.held,
            TOLERANCE * 100.0,
            counts.held - counts.within,
            counts.below,
            TIME_FLOOR_S * 1000.0,
            graft_fs::human_bytes(1 << 20),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(run: usize, create: f64, disk: i64) -> Row {
        Row {
            run,
            fixture: "small".into(),
            files: 1005,
            method: "graft".into(),
            agents: 4,
            outcome: Ok(Metrics {
                create_median_s: create,
                create_total_s: create * 4.0,
                ready_first_s: create + 1.0,
                ready_median_s: create + 0.5,
                disk_added_bytes: disk,
                prune_median_s: 0.05,
            }),
        }
    }

    #[test]
    fn csv_round_trips() {
        let r = row(2, 0.25, 4096);
        assert_eq!(Row::from_csv(&r.to_csv()).unwrap(), r);
        let skipped = Row {
            outcome: Err("needs ~13 GiB, 9 GiB available, \"tight\"".into()),
            ..r
        };
        assert_eq!(Row::from_csv(&skipped.to_csv()).unwrap(), skipped);
        assert_eq!(CSV_HEADER.split(',').count(), 12);
    }

    #[test]
    fn stats() {
        let s = Stat::of(&[1.0, 1.1, 0.9]);
        assert!((s.median - 1.0).abs() < 1e-9);
        assert!((s.spread - 0.2).abs() < 1e-9);
        assert_eq!(Stat::of(&[2.0]).spread, 0.0);
        assert!((Stat::of(&[1.0, 3.0]).median - 2.0).abs() < 1e-9);
        assert!(!Stat::of(&[0.01, 0.02, 0.03]).held(TIME_FLOOR_S));
    }

    #[test]
    fn summary_flags_spreads_over_tolerance() {
        let rows = vec![
            row(1, 1.0, 5 << 20),
            row(2, 1.05, 5 << 20),
            row(3, 1.5, 5 << 20),
        ];
        let md = summary(&rows, "# Bench\n");
        assert!(md.contains("## small (1005 tracked files)"), "{md}");
        assert!(md.contains("1.05 s ±48% ⚠"), "{md}");
        assert!(md.contains("Reproducibility"), "{md}");
        assert!(md.contains("outside, marked ⚠"), "{md}");
    }

    #[test]
    fn summary_reports_skipped_cells() {
        let mut r = row(1, 1.0, 1);
        r.outcome = Err("needs ~13 GiB".into());
        let md = summary(&[r], "");
        assert!(md.contains("not measured: run 1: needs ~13 GiB"), "{md}");
    }
}
