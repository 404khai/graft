//! The regression guard of AGENTS.md Phase 8: every graft method against
//! the `worktree` baseline, on every metric the harness recorded.
//!
//! A graft **beats** the baseline when its median is lower and the
//! difference is larger than the run-to-run spread of either. Where both
//! medians are below the measurement floor, it must simply not be higher.
//! It **loses** when it is higher by more than either spread (or higher at
//! all, below the floor). Anything else is a **tie**.
//!
//! The gate fails when the default backend's method does not beat the
//! baseline everywhere, or when any graft method loses anywhere.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::report::{DISK_FLOOR_BYTES, Row, Stat, TIME_FLOOR_S};
use crate::workloads::{self, WorkloadRow};

/// How one graft value compares with the baseline's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Lower by more than the noise.
    Beats,
    /// Within the noise.
    Ties,
    /// Higher by more than the noise.
    Loses,
}

impl Verdict {
    const fn label(self) -> &'static str {
        match self {
            Self::Beats => "beats",
            Self::Ties => "ties ⚠",
            Self::Loses => "loses ✗",
        }
    }
}

/// Compares a graft value with the baseline's. Spreads are relative, as in
/// [`Stat`]; they are converted to absolute ranges here.
#[must_use]
pub fn compare(graft: &Stat, base: &Stat, floor: f64) -> Verdict {
    if graft.median.abs() < floor && base.median.abs() < floor {
        return if graft.median > base.median {
            Verdict::Loses
        } else {
            Verdict::Beats
        };
    }
    let noise = (graft.spread * graft.median.abs()).max(base.spread * base.median.abs());
    let diff = base.median - graft.median;
    if diff > noise {
        Verdict::Beats
    } else if -diff > noise {
        Verdict::Loses
    } else {
        Verdict::Ties
    }
}

/// One compared value.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    /// Fixture name.
    pub fixture: String,
    /// What was measured, e.g. `create x4` or `git-status`.
    pub metric: String,
    /// The graft method.
    pub method: String,
    /// The graft's value.
    pub graft: Stat,
    /// The baseline's value.
    pub base: Stat,
    /// Seconds or bytes.
    pub bytes: bool,
    /// The outcome.
    pub verdict: Verdict,
}

/// The baseline method.
pub const BASELINE: &str = "worktree";

fn is_graft(method: &str) -> bool {
    method == "graft" || method.starts_with("graft-")
}

/// Every check the create matrix supports.
#[must_use]
pub fn matrix_checks(rows: &[Row]) -> Vec<Check> {
    type Key = (String, String, usize);
    type Pick = fn(&crate::report::Metrics) -> f64;
    let mut values: BTreeMap<Key, Vec<&crate::report::Metrics>> = BTreeMap::new();
    for r in rows {
        if let Ok(m) = &r.outcome {
            values
                .entry((r.fixture.clone(), r.method.clone(), r.agents))
                .or_default()
                .push(m);
        }
    }
    #[allow(clippy::cast_precision_loss)] // bytes of disk: far below 2^53
    let metrics: [(&str, Pick, bool); 5] = [
        ("create", |m| m.create_median_s, false),
        ("ready, first agent", |m| m.ready_first_s, false),
        ("ready, later agents", |m| m.ready_median_s, false),
        ("disk added", |m| m.disk_added_bytes as f64, true),
        ("prune", |m| m.prune_median_s, false),
    ];
    let mut out = Vec::new();
    for ((fixture, method, agents), graft) in &values {
        if !is_graft(method) {
            continue;
        }
        let Some(base) = values.get(&(fixture.clone(), BASELINE.to_owned(), *agents)) else {
            continue;
        };
        for (name, pick, bytes) in &metrics {
            // A one-agent cell has no later agents.
            if *agents == 1 && *name == "ready, later agents" {
                continue;
            }
            let g = Stat::of(&graft.iter().map(|m| pick(m)).collect::<Vec<_>>());
            let b = Stat::of(&base.iter().map(|m| pick(m)).collect::<Vec<_>>());
            let floor = if *bytes {
                DISK_FLOOR_BYTES
            } else {
                TIME_FLOOR_S
            };
            out.push(Check {
                fixture: fixture.clone(),
                metric: format!("{name} x{agents}"),
                method: method.clone(),
                verdict: compare(&g, &b, floor),
                graft: g,
                base: b,
                bytes: *bytes,
            });
        }
    }
    out
}

/// Every check the in-graft workloads support.
#[must_use]
pub fn workload_checks(rows: &[WorkloadRow]) -> Vec<Check> {
    let mut cells: BTreeMap<(String, String, String), Vec<&WorkloadRow>> = BTreeMap::new();
    for r in rows {
        cells
            .entry((r.fixture.clone(), r.workload.clone(), r.method.clone()))
            .or_default()
            .push(r);
    }
    let mut out = Vec::new();
    for ((fixture, workload, method), graft) in &cells {
        if !is_graft(method) {
            continue;
        }
        let base = cells.get(&(fixture.clone(), workload.clone(), BASELINE.to_owned()));
        let (Some(g), Some(b)) = (
            workloads::stat(graft),
            base.and_then(|b| workloads::stat(b)),
        ) else {
            continue;
        };
        out.push(Check {
            fixture: fixture.clone(),
            metric: workload.clone(),
            method: method.clone(),
            verdict: compare(&g, &b, TIME_FLOOR_S),
            graft: g,
            base: b,
            bytes: false,
        });
    }
    out
}

/// Whether the gate passes: `default` beats everywhere, and no graft
/// method loses anywhere.
#[must_use]
pub fn passes(checks: &[Check], default: &str) -> bool {
    !checks.is_empty()
        && checks.iter().all(|c| {
            c.verdict != Verdict::Loses && (c.method != default || c.verdict == Verdict::Beats)
        })
}

fn fmt(v: f64, bytes: bool) -> String {
    if bytes {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // display only
        let b = graft_fs::human_bytes(v.abs().round() as u64);
        if v < 0.0 { format!("−{b}") } else { b }
    } else if v < 1.0 {
        format!("{:.0} ms", v * 1000.0)
    } else {
        format!("{v:.2} s")
    }
}

/// Renders `gate.md`.
#[must_use]
pub fn render(checks: &[Check], default: &str) -> String {
    let mut out = format!(
        "# Grafts vs worktrees\n\n\
         Default backend's method: `{default}`. It must **beat** `{BASELINE}` on every \
         value (lower, by more than either side's spread; below the floor of {:.0} ms or \
         1 MiB, not higher). No graft method may **lose** anywhere.\n",
        TIME_FLOOR_S * 1000.0
    );
    let verdict = if passes(checks, default) {
        "**pass**"
    } else {
        "**fail**"
    };
    let count = |method: &str, v: Verdict| {
        checks
            .iter()
            .filter(|c| c.method == method && c.verdict == v)
            .count()
    };
    let mut methods: Vec<&str> = checks.iter().map(|c| c.method.as_str()).collect();
    methods.sort_by_key(|m| crate::report::method_order(m));
    methods.dedup();
    let _ = writeln!(out, "\nVerdict: {verdict}\n");
    out.push_str("| method | beats | ties | loses |\n| --- | ---: | ---: | ---: |\n");
    for m in &methods {
        let _ = writeln!(
            out,
            "| {m}{} | {} | {} | {} |",
            if *m == default { " (default)" } else { "" },
            count(m, Verdict::Beats),
            count(m, Verdict::Ties),
            count(m, Verdict::Loses)
        );
    }
    let mut fixture = "";
    for c in checks {
        if c.fixture != fixture {
            fixture = &c.fixture;
            let _ = write!(
                out,
                "\n## {fixture}\n\n| metric | method | graft | {BASELINE} | ratio | verdict |\n\
                 | --- | --- | ---: | ---: | ---: | --- |\n"
            );
        }
        let ratio = if c.base.median == 0.0 {
            "—".to_owned()
        } else {
            format!("{:.2}×", c.graft.median / c.base.median)
        };
        let _ = writeln!(
            out,
            "| {} | {} | {} ±{:.0}% | {} ±{:.0}% | {ratio} | {} |",
            c.metric,
            c.method,
            fmt(c.graft.median, c.bytes),
            c.graft.spread * 100.0,
            fmt(c.base.median, c.bytes),
            c.base.spread * 100.0,
            c.verdict.label()
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(median: f64, spread: f64) -> Stat {
        Stat {
            median,
            spread,
            runs: 3,
        }
    }

    #[test]
    fn beats_only_by_more_than_the_noise() {
        // 1.0 ± 10% vs 2.0 ± 10%: a 1.0 gap, 0.2 noise.
        assert_eq!(compare(&s(1.0, 0.1), &s(2.0, 0.1), 0.1), Verdict::Beats);
        // 1.8 vs 2.0 with 20% spread on the baseline (0.4): within noise.
        assert_eq!(compare(&s(1.8, 0.05), &s(2.0, 0.2), 0.1), Verdict::Ties);
        assert_eq!(compare(&s(2.5, 0.05), &s(2.0, 0.05), 0.1), Verdict::Loses);
    }

    #[test]
    fn below_the_floor_must_not_be_higher() {
        assert_eq!(compare(&s(0.02, 2.0), &s(0.03, 0.0), 0.1), Verdict::Beats);
        assert_eq!(compare(&s(0.03, 0.0), &s(0.03, 0.0), 0.1), Verdict::Beats);
        assert_eq!(compare(&s(0.04, 0.0), &s(0.03, 0.9), 0.1), Verdict::Loses);
        // One side above the floor: the spread rule applies.
        assert_eq!(compare(&s(0.05, 0.1), &s(0.5, 0.1), 0.1), Verdict::Beats);
    }

    fn check(method: &str, verdict: Verdict) -> Check {
        Check {
            fixture: "small".into(),
            metric: "create x1".into(),
            method: method.into(),
            graft: s(1.0, 0.0),
            base: s(1.0, 0.0),
            bytes: false,
            verdict,
        }
    }

    #[test]
    fn the_default_must_beat_and_nothing_may_lose() {
        use Verdict::{Beats, Loses, Ties};
        assert!(passes(
            &[check("graft", Beats), check("graft-nfs", Ties)],
            "graft"
        ));
        assert!(!passes(&[check("graft", Ties)], "graft"));
        assert!(!passes(
            &[check("graft", Beats), check("graft-nfs", Loses)],
            "graft"
        ));
        assert!(!passes(&[], "graft"), "no data never passes");
    }

    #[test]
    fn workload_checks_pair_grafts_with_the_baseline() {
        let row = |method: &str, median: f64| WorkloadRow {
            run: 1,
            fixture: "medium".into(),
            method: method.into(),
            workload: "git-status".into(),
            outcome: Ok(workloads::Samples {
                median_s: median,
                min_s: median,
                max_s: median,
                count: 5,
            }),
        };
        let checks = workload_checks(&[row("worktree", 0.5), row("graft-nfs", 0.9)]);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].verdict, Verdict::Loses);
        let md = render(&checks, "graft-nfs");
        assert!(md.contains("Verdict: **fail**"), "{md}");
        assert!(
            md.contains("| git-status | graft-nfs | 900 ms ±0% | 500 ms ±0% | 1.80× | loses ✗ |"),
            "{md}"
        );
    }
}
