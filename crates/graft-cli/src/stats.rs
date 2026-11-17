//! `graft stats`: what graftd has served since it started (ADR 0033).

use std::fmt::Write as _;
use std::time::Duration;

use graft_daemon::Client;
use graft_daemon::protocol::Stats;

use crate::{Environment, Output};

/// Arguments for `graft stats`.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Print the counters as JSON.
    #[arg(long)]
    json: bool,
}

/// Runs `graft stats`. Never starts graftd.
///
/// # Errors
///
/// If `$GRAFT_HOME` is unusable, or graftd answers with an error.
pub(crate) fn run(args: &Args, env: &Environment) -> anyhow::Result<Output> {
    let home = env.require_home()?;
    let Ok(mut client) = Client::connect(home) else {
        let text =
            "graftd is not running: no virtual graft is mounted, so there is nothing to count\n"
                .to_owned();
        return Ok(
            Output::stdout(text).json_if(args.json, || serde_json::json!({"running": false}))
        );
    };
    let stats = client.stats()?;
    let text = render(&stats);
    Ok(Output::stdout(text).json_if(args.json, || {
        let mut v = serde_json::to_value(&stats).unwrap_or_default();
        v["running"] = true.into();
        v
    }))
}

/// `850µs`, `1.2ms`, `3.4s`.
fn duration(us: u64) -> String {
    if us < 1000 {
        format!("{us}µs")
    } else if us < 1_000_000 {
        format!("{:.1}ms", Duration::from_micros(us).as_secs_f64() * 1e3)
    } else {
        format!("{:.1}s", Duration::from_micros(us).as_secs_f64())
    }
}

fn uptime(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m {}s", s / 60, s % 60),
        _ => format!("{}h {}m", s / 3600, s % 3600 / 60),
    }
}

fn render(s: &Stats) -> String {
    let mut out = format!(
        "graftd {} (pid {}), up {}, {} graft(s) mounted",
        s.version,
        s.pid,
        uptime(s.uptime_ms),
        s.mounts
    );
    if let Some(rss) = s.rss_bytes {
        let _ = write!(out, ", memory {}", graft_fs::human_bytes(rss));
    }
    out.push('\n');
    let lookups = s.cache_hits + s.cache_misses;
    if lookups > 0 {
        #[allow(clippy::cast_precision_loss)] // a percentage for people
        let rate = s.cache_hits as f64 * 100.0 / lookups as f64;
        let _ = writeln!(
            out,
            "content cache: {} hit(s), {} miss(es), {rate:.0}% hits",
            s.cache_hits, s.cache_misses
        );
    }
    if s.ops.is_empty() {
        out.push_str("no filesystem operations yet\n");
        return out;
    }
    let _ = writeln!(
        out,
        "\n{:<10}{:>10}{:>9}{:>10}{:>10}{:>10}{:>10}",
        "op", "count", "errors", "mean", "p50", "p99", "max"
    );
    for o in &s.ops {
        let _ = writeln!(
            out,
            "{:<10}{:>10}{:>9}{:>10}{:>10}{:>10}{:>10}",
            o.op,
            o.count,
            o.errors,
            duration(o.mean_us),
            duration(o.p50_us),
            duration(o.p99_us),
            duration(o.max_us)
        );
    }
    out.push_str(
        "\np50 and p99 are bucket bounds (powers of two); errors include expected ones, such as lookups of missing names\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use graft_daemon::protocol::OpStats;

    #[test]
    fn formats_durations_and_uptime() {
        assert_eq!(duration(850), "850µs");
        assert_eq!(duration(1_200), "1.2ms");
        assert_eq!(duration(3_400_000), "3.4s");
        assert_eq!(uptime(5_000), "5s");
        assert_eq!(uptime(192_000), "3m 12s");
        assert_eq!(uptime(7_500_000), "2h 5m");
    }

    #[test]
    fn renders_a_table() {
        let s = Stats {
            version: "1.0.0".to_owned(),
            pid: 7,
            uptime_ms: 61_000,
            mounts: 2,
            rss_bytes: None,
            ops: vec![OpStats {
                op: "read".to_owned(),
                count: 40,
                errors: 1,
                mean_us: 90,
                p50_us: 64,
                p99_us: 2048,
                max_us: 3000,
            }],
            cache_hits: 3,
            cache_misses: 1,
        };
        let text = render(&s);
        assert!(
            text.starts_with("graftd 1.0.0 (pid 7), up 1m 1s, 2 graft(s) mounted\n"),
            "{text}"
        );
        assert!(text.contains("75% hits"), "{text}");
        assert!(text.contains("read") && text.contains("2.0ms"), "{text}");
    }
}
