//! `graft-bench`: the end-to-end benchmark harness (AGENTS.md Phase 4).
//!
//! For each fixture, method (`graft`, `worktree`, `clone`) and agent count,
//! it creates that many workspaces one after another and measures:
//!
//! - **create**: time for the workspace command alone;
//! - **ready**: create + install (`npm ci`, where the method needs one) +
//!   a passing `npm test`: when an agent could start work;
//! - **disk added**: the drop in the filesystem's free space with every
//!   workspace present (so reflinks, hardlinks and the layer cache are
//!   counted as they really are);
//! - **prune**: time to remove one workspace.
//!
//! Every cell is repeated `--runs` times and the summary reports the median
//! and the spread. Cells that would not fit in the free space are skipped and
//! reported, never attempted. Run it through `bench/run.sh`.

#![forbid(unsafe_code)]
// A command-line tool: progress goes to stderr, nothing else prints.
#![allow(clippy::print_stderr)]

mod fixture;
mod methods;
mod report;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};

use crate::methods::{CloneMethod, GraftMethod, Method, WorktreeMethod};
use crate::report::{CSV_HEADER, Metrics, Row};

/// A command with git isolated from the user's configuration.
pub(crate) fn command(program: impl AsRef<std::ffi::OsStr>, dir: &Path) -> Command {
    let mut c = Command::new(program);
    c.current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "graft-bench")
        .env("GIT_AUTHOR_EMAIL", "bench@graft.invalid")
        .env("GIT_COMMITTER_NAME", "graft-bench")
        .env("GIT_COMMITTER_EMAIL", "bench@graft.invalid");
    c
}

#[derive(Debug, Parser)]
#[command(name = "graft-bench", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Run the benchmark matrix and write results.csv and summary.md.
    Run(RunArgs),
    /// Regenerate summary.md from an existing results.csv.
    Summarize {
        /// The results directory (containing results.csv and header.md).
        dir: PathBuf,
    },
}

#[derive(Debug, clap::Args)]
struct RunArgs {
    /// The `graft` binary to measure.
    #[arg(long)]
    graft: PathBuf,
    /// Scratch directory for fixtures and workspaces. Its filesystem is the
    /// one measured; use a dedicated, quiet volume.
    #[arg(long)]
    work: PathBuf,
    /// Where to write results.csv and summary.md.
    #[arg(long)]
    out: PathBuf,
    /// Fixtures to run.
    #[arg(long, value_delimiter = ',', default_value = "small,medium,large")]
    fixtures: Vec<String>,
    /// Agent counts.
    #[arg(long, value_delimiter = ',', default_value = "1,4,8,16")]
    agents: Vec<usize>,
    /// Methods to compare.
    #[arg(long, value_delimiter = ',', default_value = "graft,worktree,clone")]
    methods: Vec<String>,
    /// Times to repeat the whole matrix.
    #[arg(long, default_value_t = 3)]
    runs: usize,
    /// Free space to leave untouched, in GiB. Cells needing more are skipped.
    #[arg(long, default_value_t = 2)]
    margin_gib: u64,
    /// Skip the discarded warm-up cell (one agent per fixture and method)
    /// that runs before the first measured run.
    #[arg(long)]
    no_warmup: bool,
    /// Short description of the machine for the summary header.
    #[arg(long, default_value = "")]
    machine: String,
}

fn secs(d: Duration) -> f64 {
    d.as_secs_f64()
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        f64::midpoint(v[n / 2 - 1], v[n / 2])
    }
}

fn sync() {
    let _ = Command::new("sync").status();
    // Let deferred filesystem accounting (APFS) catch up before reading
    // free space. Not part of any timed section.
    std::thread::sleep(Duration::from_millis(500));
}

fn run_ok(cmd: &mut Command, what: &str) -> anyhow::Result<()> {
    let out = cmd.output().with_context(|| format!("running {what}"))?;
    if !out.status.success() {
        bail!(
            "{what} failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Allocated bytes of the checked-out tree (excluding `.git`).
fn checkout_bytes(dir: &Path) -> anyhow::Result<u64> {
    let mut total = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name() == ".git" {
            continue;
        }
        let meta = entry.metadata()?;
        total += if meta.is_dir() {
            checkout_bytes(&entry.path())?
        } else {
            #[cfg(unix)]
            {
                std::os::unix::fs::MetadataExt::blocks(&meta) * 512
            }
            #[cfg(not(unix))]
            meta.len()
        };
    }
    Ok(total)
}

/// Rough upper bound on what a cell needs, so it can be skipped instead of
/// filling the disk.
fn estimate(
    method: &str,
    agents: usize,
    spec: &fixture::Spec,
    checkout: u64,
    reflinks: bool,
) -> u64 {
    const BLOCK: u64 = 4096;
    let n = agents as u64;
    let node_modules = (spec.deps * spec.dep_files) as u64 * BLOCK * 2;
    let index = spec.modules as u64 * 128;
    match method {
        "graft" if reflinks => n * (index + (1 << 20)) + node_modules,
        _ => n * (checkout + node_modules + index) + node_modules,
    }
}

/// Timed creations each cell collects at least: a single sample on a
/// desktop is mostly noise. A cell with fewer agents is repeated (all
/// workspaces pruned and the method reset in between) until it has this
/// many.
const MIN_SAMPLES: usize = 5;

/// One pass of a cell: N workspaces created, tested, measured, pruned.
struct Pass {
    creates: Vec<f64>,
    readies: Vec<f64>,
    disk: i64,
    prunes: Vec<f64>,
}

fn one_pass(method: &mut dyn Method, agents: usize, work_fs: &Path) -> anyhow::Result<Pass> {
    sync();
    let free_before = graft_fs::available_bytes(work_fs)?;
    let mut pass = Pass {
        creates: Vec::with_capacity(agents),
        readies: Vec::with_capacity(agents),
        disk: 0,
        prunes: Vec::with_capacity(agents),
    };
    let mut paths = Vec::with_capacity(agents);
    for i in 0..agents {
        let t0 = Instant::now();
        let path = method.create(i)?;
        pass.creates.push(secs(t0.elapsed()));
        if method.needs_install() {
            run_ok(
                command("npm", &path).args([
                    "ci",
                    "--offline",
                    "--no-audit",
                    "--no-fund",
                    "--ignore-scripts",
                ]),
                "npm ci",
            )?;
        }
        run_ok(command("npm", &path).args(["test", "--silent"]), "npm test")?;
        pass.readies.push(secs(t0.elapsed()));
        paths.push(path);
    }
    sync();
    let free_after = graft_fs::available_bytes(work_fs)?;
    pass.disk = i64::try_from(free_before).unwrap_or(i64::MAX)
        - i64::try_from(free_after).unwrap_or(i64::MAX);
    for (i, path) in paths.iter().enumerate() {
        let t0 = Instant::now();
        method.prune(i, path)?;
        pass.prunes.push(secs(t0.elapsed()));
    }
    method.reset()?;
    Ok(pass)
}

fn measure(method: &mut dyn Method, agents: usize, work_fs: &Path) -> anyhow::Result<Metrics> {
    let passes = MIN_SAMPLES.div_ceil(agents).max(1);
    let mut all = Vec::with_capacity(passes);
    for _ in 0..passes {
        all.push(one_pass(method, agents, work_fs)?);
    }
    method.finish()?;
    #[allow(clippy::cast_precision_loss)] // bytes of disk: far below 2^53
    let disk = median(all.iter().map(|p| p.disk as f64).collect());
    #[allow(clippy::cast_possible_truncation)] // median of i64 values
    let disk = disk.round() as i64;
    Ok(Metrics {
        create_median_s: median(all.iter().flat_map(|p| p.creates.iter().copied()).collect()),
        create_total_s: median(all.iter().map(|p| p.creates.iter().sum()).collect()),
        ready_first_s: median(all.iter().map(|p| p.readies[0]).collect()),
        ready_median_s: median(all.iter().flat_map(|p| p.readies.iter().copied()).collect()),
        disk_added_bytes: disk,
        prune_median_s: median(all.iter().flat_map(|p| p.prunes.iter().copied()).collect()),
    })
}

fn tool_version(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_else(
            || "unknown".to_owned(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_owned(),
        )
}

fn load_average() -> String {
    let out = tool_version("uptime", &[]);
    out.split("load average").nth(1).map_or(out.clone(), |s| {
        s.trim_start_matches(['s', ':', ' ']).to_owned()
    })
}

fn header(args: &RunArgs, work_fs: &str) -> String {
    let os = if cfg!(target_os = "macos") {
        format!("macOS {}", tool_version("sw_vers", &["-productVersion"]))
    } else {
        tool_version("uname", &["-sr"])
    };
    let cpu = if cfg!(target_os = "macos") {
        tool_version("sysctl", &["-n", "machdep.cpu.brand_string"])
    } else {
        std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|c| {
                c.lines()
                    .find(|l| l.starts_with("model name"))
                    .map(|l| l.split(':').nth(1).unwrap_or("").trim().to_owned())
            })
            .unwrap_or_else(|| "unknown".to_owned())
    };
    let cores = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    format!(
        "# Graft benchmark: grafts vs git worktrees vs full clones\n\n\
         | | |\n| --- | --- |\n\
         | machine | {} |\n| OS | {os} ({}) |\n| CPU | {cpu}, {cores} threads |\n\
         | filesystem | {work_fs} (dedicated volume) |\n| graft | {} |\n| git | {} |\n| node / npm | {} / {} |\n\
         | runs | {} (after one discarded warm-up cell per fixture and method{}); cells with fewer than {MIN_SAMPLES} agents repeat until they have {MIN_SAMPLES} timed creations |\n\
         | load average at start | {} |\n| command | `{}` |\n\n\
         Each cell creates N workspaces one after another from the same fixture.\n\
         **create** is the workspace command alone. **ready** adds `npm ci --offline` (worktree and clone \
         need one per workspace; grafts reuse a `node_modules` layer that the first graft builds) and a \
         passing `npm test`. **disk added** is the drop in free space on the volume with all N workspaces \
         present, including Graft's layer cache and index. Local clones hardlink git objects, as `git clone` \
         does by default. Values are medians over runs, ± the spread ((max − min) / median).\n",
        if args.machine.is_empty() {
            "unspecified"
        } else {
            &args.machine
        },
        std::env::consts::ARCH,
        tool_version(&args.graft.to_string_lossy(), &["--version"]),
        tool_version("git", &["--version"]),
        tool_version("node", &["--version"]),
        tool_version("npm", &["--version"]),
        args.runs,
        if args.no_warmup {
            "; warm-up disabled"
        } else {
            ""
        },
        load_average(),
        std::env::args().collect::<Vec<_>>().join(" "),
    )
}

/// Runs one discarded single-agent cell per fixture and method. Page caches,
/// npm's cache and each fixture's git index start cold on first touch;
/// measuring that would only measure the machine's state.
fn warm_up(
    methods: &[String],
    fixtures: &[(&fixture::Spec, PathBuf, u64)],
    graft: &Path,
    work: &Path,
) -> anyhow::Result<()> {
    for (spec, dir, _) in fixtures {
        for method_name in methods {
            eprintln!(
                "graft-bench: warm-up {} {method_name} (discarded)",
                spec.name
            );
            let cell = work.join(format!("warmup-{}-{method_name}", spec.name));
            std::fs::create_dir_all(&cell)?;
            let mut method = make_method(method_name, graft, dir, &cell)?;
            one_pass(method.as_mut(), 1, work)?;
            method.finish()?;
            let _ = std::fs::remove_dir_all(&cell);
        }
    }
    Ok(())
}

fn make_method(
    name: &str,
    graft: &Path,
    fixture: &Path,
    cell: &Path,
) -> anyhow::Result<Box<dyn Method>> {
    Ok(match name {
        "graft" => Box::new(GraftMethod::new(graft, fixture, cell)?),
        "worktree" => Box::new(WorktreeMethod::new(fixture, cell)),
        _ => Box::new(CloneMethod::new(fixture, cell)),
    })
}

fn run(args: &RunArgs) -> anyhow::Result<()> {
    for m in &args.methods {
        anyhow::ensure!(
            ["graft", "worktree", "clone"].contains(&m.as_str()),
            "unknown method {m:?}"
        );
    }
    anyhow::ensure!(args.runs >= 1, "--runs must be at least 1");
    let specs = args
        .fixtures
        .iter()
        .map(|f| fixture::spec(f))
        .collect::<anyhow::Result<Vec<_>>>()?;
    std::fs::create_dir_all(&args.work)?;
    std::fs::create_dir_all(&args.out)?;
    let work = args.work.canonicalize()?;
    let graft = args.graft.canonicalize().context("the --graft binary")?;
    let fs = graft_fs::filesystem_type(&work)?;
    let reflinks = graft_fs::probe_reflink(&work)?;

    let head = header(
        args,
        &format!(
            "{fs}, reflinks {}",
            if reflinks { "supported" } else { "unsupported" }
        ),
    );
    std::fs::write(args.out.join("header.md"), &head)?;
    let csv_path = args.out.join("results.csv");
    let mut csv = std::fs::File::create(&csv_path)?;
    writeln!(csv, "{CSV_HEADER}")?;

    let fixtures_dir = work.join("fixtures");
    let mut fixtures = Vec::new();
    for spec in &specs {
        let dir = fixture::ensure(&fixtures_dir, spec)?;
        let checkout = checkout_bytes(&dir)?;
        fixtures.push((spec, dir, checkout));
    }

    if !args.no_warmup {
        warm_up(&args.methods, &fixtures, &graft, &work)?;
    }

    let mut rows = Vec::new();
    for run_no in 1..=args.runs {
        for (spec, dir, checkout) in &fixtures {
            for method_name in &args.methods {
                for &agents in &args.agents {
                    let cell = work.join(format!("cell-{}-{method_name}-{agents}", spec.name));
                    if cell.exists() {
                        std::fs::remove_dir_all(&cell)?;
                    }
                    std::fs::create_dir_all(&cell)?;
                    let need = estimate(method_name, agents, spec, *checkout, reflinks);
                    let free = graft_fs::available_bytes(&work)?;
                    let margin = args.margin_gib << 30;
                    eprintln!(
                        "graft-bench: run {run_no}/{} {} {method_name} x{agents}",
                        args.runs, spec.name
                    );
                    let outcome = if need.saturating_add(margin) > free {
                        Err(format!(
                            "skipped: needs ~{}, {} free with a {} GiB margin",
                            graft_fs::human_bytes(need),
                            graft_fs::human_bytes(free),
                            args.margin_gib
                        ))
                    } else {
                        let mut method = make_method(method_name, &graft, dir, &cell)?;
                        measure(method.as_mut(), agents, &work).map_err(|e| format!("error: {e:#}"))
                    };
                    if let Err(why) = &outcome {
                        eprintln!("graft-bench:   {why}");
                    }
                    let row = Row {
                        run: run_no,
                        fixture: spec.name.to_owned(),
                        files: fixture::tracked_files(spec),
                        method: method_name.clone(),
                        agents,
                        outcome,
                    };
                    writeln!(csv, "{}", row.to_csv())?;
                    csv.flush()?;
                    rows.push(row);
                    let _ = std::fs::remove_dir_all(&cell);
                }
            }
        }
    }
    std::fs::write(args.out.join("summary.md"), report::summary(&rows, &head))?;
    eprintln!("graft-bench: wrote {} and summary.md", csv_path.display());
    Ok(())
}

fn summarize(dir: &Path) -> anyhow::Result<()> {
    let rows = report::read_csv(&dir.join("results.csv"))?;
    let head = std::fs::read_to_string(dir.join("header.md")).unwrap_or_default();
    std::fs::write(dir.join("summary.md"), report::summary(&rows, &head))?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Cmd::Run(args) => run(&args),
        Cmd::Summarize { dir } => summarize(&dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_scale_with_agents_and_respect_reflinks() {
        let spec = fixture::spec("large").unwrap();
        let checkout = 800 << 20;
        let wt1 = estimate("worktree", 1, &spec, checkout, true);
        let wt16 = estimate("worktree", 16, &spec, checkout, true);
        assert!(wt16 > 15 * (wt1 - (spec.deps * spec.dep_files) as u64 * 8192));
        let graft = estimate("graft", 16, &spec, checkout, true);
        assert!(
            graft < wt16 / 10,
            "reflinked grafts must be estimated far smaller"
        );
        assert_eq!(
            estimate("graft", 16, &spec, checkout, false),
            estimate("clone", 16, &spec, checkout, false)
        );
    }

    /// A method that creates a minimal Node project per workspace, so the
    /// whole measure loop runs without the graft binary.
    struct Fake {
        dir: PathBuf,
        created: usize,
        pruned: usize,
        resets: usize,
    }

    impl Method for Fake {
        fn create(&mut self, i: usize) -> anyhow::Result<PathBuf> {
            let path = self.dir.join(format!("w{i}"));
            std::fs::create_dir(&path)?;
            std::fs::write(
                path.join("package.json"),
                r#"{"name":"w","scripts":{"test":"node -e 0"}}"#,
            )?;
            // Something measurable on disk.
            std::fs::write(path.join("blob"), vec![7u8; 4 << 20])?;
            self.created += 1;
            Ok(path)
        }
        fn needs_install(&self) -> bool {
            false
        }
        fn prune(&mut self, _i: usize, path: &Path) -> anyhow::Result<()> {
            std::fs::remove_dir_all(path)?;
            self.pruned += 1;
            Ok(())
        }
        fn reset(&mut self) -> anyhow::Result<()> {
            self.resets += 1;
            Ok(())
        }
        fn finish(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn measure_repeats_small_cells_and_cleans_up() {
        if Command::new("npm").arg("--version").output().is_err() {
            assert!(
                std::env::var_os("GRAFT_E2E_REQUIRE_NPM").is_none(),
                "npm is required but missing"
            );
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let mut fake = Fake {
            dir: dir.path().to_owned(),
            created: 0,
            pruned: 0,
            resets: 0,
        };
        let m = measure(&mut fake, 2, dir.path()).unwrap();
        // 2 agents need 3 passes to reach MIN_SAMPLES creations.
        assert_eq!((fake.created, fake.pruned, fake.resets), (6, 6, 3));
        assert!(m.create_median_s > 0.0 && m.ready_median_s >= m.create_median_s);
        assert!(m.create_total_s >= m.create_median_s);
        // Disk deltas are not asserted: on a shared temporary directory
        // other processes move free space. Real runs use a dedicated volume.
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "workspaces left behind"
        );
    }

    #[test]
    fn median_of_even_and_odd() {
        assert!((median(vec![3.0, 1.0, 2.0]) - 2.0).abs() < 1e-9);
        assert!((median(vec![4.0, 1.0, 2.0, 3.0]) - 2.5).abs() < 1e-9);
    }
}
