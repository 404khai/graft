import { Fragment, useEffect, useState } from "react";
import { Flora, type FloraKind } from "../components/Flora";
import { Footer } from "../components/Footer";
import { GraftLogo } from "../components/GraftLogo";
import { Nav } from "../components/Nav";
import "../styles.css";
import "./changelog.css";

const PR = "https://github.com/404khai/graft/pull/";

type Entry = {
  /** the release that shipped it */
  release: string;
  date: string; // ISO
  title: string;
  tag: "Feature" | "Performance" | "Platform" | "Foundation";
  lede: string;
  items: { title: string; body: string; pr: number }[];
  art?: FloraKind;
};

/** Newest first. Dates are when each release was tagged. */
const RELEASES = [
  { version: "0.2.0", date: "2026-11-19" },
  { version: "0.1.1", date: "2026-11-15" },
  { version: "0.1.0", date: "2026-11-14" },
];

// From the merged PRs on main.
const ENTRIES: Entry[] = [
  {
    release: "0.2.0",
    date: "2026-11-19",
    title: "Rings on reflink grafts, and ~/.graft on macOS",
    tag: "Feature",
    lede: "Rings and rewind now work on the default backend, and new macOS installs keep grafts at a path with no space in it.",
    items: [
      { title: "Rings on reflink grafts", body: "graft ring commits the working tree, tracked and untracked, under a per-worktree ref; graft rewind rings the current state as undo-<n> first. Ignored files such as build outputs and layers are never touched.", pr: 64 },
      { title: "~/.graft on macOS", body: "New installs use ~/.graft instead of ~/Library/Application Support/graft, so scripts and prompts no longer quote graft paths. An existing home stays where it is, and graft doctor says which default applies.", pr: 63 },
      { title: "Rereads in FUSE grafts ≈2× faster", body: "Read-only opens of files unchanged from the base keep the kernel's page cache. Reading a 20k-file tree again: 5.9 s → 3.1 s; rg 1.5–2.1 s → 1.2–1.4 s. A written file still drops it, so no reader sees stale data.", pr: 66 },
    ],
    art: "leaf1",
  },
  {
    release: "0.2.0",
    date: "2026-11-18",
    title: "Start agents inside their graft",
    tag: "Feature",
    lede: "From the Turborepo field report: one command to create a graft and start an agent in it, confined, and grafts every git tool can see.",
    items: [
      { title: "graft new <name> -- <command>", body: "Creates the graft, then runs the command in it through graft run, confined and with GRAFT_NAME set. graft run <name> with no command starts a confined shell.", pr: 61 },
      { title: "Grafts as git worktrees", body: "Opt in with [git] worktrees in config.toml and grafts are registered as git worktrees of Root, on their own graft-live/<name>/live branch, so editors and git clients list them. A confined run can write only its own branch.", pr: 39 },
      { title: "Radar warns before landing", body: "graft integrate prints a warning for each overlap Radar has recorded with the grafts it lands. It does not block; --json and graft_integrate return them as radar.", pr: 62 },
      { title: "A starter graft.toml", body: "examples/rust-pnpm/ shares Cargo's target/ across grafts and installs a pnpm workspace offline from the shared store.", pr: 60 },
      { title: "No stale graftd after an upgrade", body: "An old graftd serving no mounts is restarted automatically. graft ring's help now says --list lists rings.", pr: 56 },
    ],
    art: "graft",
  },
  {
    release: "0.2.0",
    date: "2026-11-18",
    title: "A first git status in 1 s, and a steadier graftd",
    tag: "Performance",
    lede: "Fixes for the waits and flakes agents hit right after creating or restarting a graft.",
    items: [
      { title: "First git status in NFS grafts", body: "An NFS graft starts building its .git as soon as it is mounted. On a 10k-file repo the first git status after graft new went from 5.5 s to 1.15 s.", pr: 55 },
      { title: "One graftd at a time", body: "graft daemon stop now returns once graftd has exited, and a new graftd waits for a stopping one. This ends an intermittent \"not a git repository\" in remounted grafts.", pr: 65 },
    ],
    art: "leaf2",
  },
  {
    release: "0.2.0",
    date: "2026-11-17",
    title: "See inside Graft when something goes wrong",
    tag: "Feature",
    lede: "Diagnostics for bug reports, each also available to agents over MCP.",
    items: [
      { title: "graft doctor --report", body: "One JSON file with doctor's findings, config, each Root's schema and integrity, graftd's log tail and recent crash files, with secrets redacted. graft_doctor hands an agent its path.", pr: 48 },
      { title: "graft fsck", body: "Read-only checks that each virtual graft's op log replays to its delta, that blobs exist and match their hashes, and that layers and mounts are in order.", pr: 50 },
      { title: "graft stats", body: "Counts and latencies for every FUSE and NFS request, and content-cache hits, from graftd. Also graft_stats.", pr: 51 },
      { title: "Request ids", body: "Every command and MCP call carries a request id into graftd's log, which now rotates at 8 MiB.", pr: 49 },
      { title: "Opt-in telemetry", body: "Off unless you run graft telemetry on, and sent only to an endpoint you configure: one anonymous count per command. DO_NOT_TRACK turns it off.", pr: 52 },
    ],
    art: "branch",
  },
  {
    release: "0.2.0",
    date: "2026-11-16",
    title: "Fixes from the Astrid field report",
    tag: "Feature",
    lede: "An agent built a large Rust and Swift project in grafts over MCP. These are the changes it asked for.",
    items: [
      { title: "graft_init and graft_path", body: "graft mcp starts in any git repository, and an agent can register the Root itself and ask for a graft's path.", pr: 42 },
      { title: "Integrate to a new branch, with your own gate", body: "--create-branch creates a missing target; --test COMMAND gates just this call, with no commit to graft.toml first.", pr: 43 },
      { title: "Clone Root's build outputs", body: "graft new --clone-ignored target,web/node_modules reflinks ignored directories from Root into a new graft, with no build.", pr: 45 },
      { title: "List and prune in milliseconds", body: "graft ls measures disk on 8 threads (or not at all with --no-disk), and prune frees the name at once and deletes in the background: 0.45 s → 0.01 s with a 1.9 GB target/.", pr: 44 },
      { title: "Refused writes named", body: "A failed graft run on macOS names the paths the sandbox refused; --dev-caches makes toolchain caches writable for the run.", pr: 46 },
      { title: "Modes that match git checkout", body: "Files in virtual grafts follow the umask, so ls -l matches Root under umask 002.", pr: 41 },
    ],
    art: "leaf3",
  },
  {
    release: "0.1.1",
    date: "2026-11-15",
    title: "Every MCP tool answers in JSON",
    tag: "Feature",
    lede: "After the first complete agent session over MCP, every safety check held. These changes make every result machine-readable.",
    items: [
      { title: "Exit codes from graft_run", body: "Each run returns exit_code, signal, stdout, stderr, duration_ms and timed_out, with stdout and stderr kept apart.", pr: 35 },
      { title: "Specific refusal codes", body: "work_would_be_lost, checkout_would_fall_behind and a code for each integrate request, with hints that name MCP parameters.", pr: 35 },
      { title: "Dry runs that predict", body: "A dry run now applies the checked-out-branch rule, so a passing dry run means the real one will land.", pr: 35 },
      { title: "Results that name the Root", body: "graft_list, graft_status and the server instructions say which Root and version they serve.", pr: 35 },
      { title: "Plans that drop nothing", body: "graft_plan expands directories into their files and lists what it could not place yet, such as new files.", pr: 35 },
    ],
    art: "leaf1",
  },
  {
    release: "0.1.1",
    date: "2026-11-15",
    title: "Clearer answers for agents over MCP",
    tag: "Feature",
    lede: "From the first agent session driving Graft over MCP: failures an agent can act on, and every tool answering in JSON.",
    items: [
      { title: "Coded errors", body: "A failed call returns {code, message, hint, output}, with codes like not_found, layer_build_failed and confirm_required, so agents don't parse English.", pr: 34 },
      { title: "Hints in MCP terms", body: "Next steps name MCP tools (graft_list, graft_new), not CLI commands an agent can't run.", pr: 34 },
      { title: "graft_rewind", body: "Rings made with graft_ring can now be restored over MCP, and every rewind can itself be undone.", pr: 34 },
      { title: "JSON everywhere", body: "graft_list and graft_status return JSON, like plan and radar; graft ls --json and graft status --json do the same on the CLI.", pr: 34 },
      { title: "No stray daemon", body: "A virtual graft that fails to create no longer leaves graftd running.", pr: 34 },
    ],
    art: "graft",
  },
  {
    release: "0.1.1",
    date: "2026-11-14",
    title: "Faster landing and NFS builds on macOS",
    tag: "Performance",
    lede: "Fixes from a real benchmark against git worktrees on an Apple M3, with every correctness suite unchanged.",
    items: [
      { title: "graft integrate ≈2.6× faster", body: "One scratch checkout serves the whole queue when no test gate or resolver runs, and layers are cloned only when one does: 4 grafts land in 1.6 s instead of 4.3 s.", pr: 33 },
      { title: "tsc in NFS grafts ≈30% faster", body: "Overwrites are journaled once with no copy-up, and macOS no longer retries AppleDouble files: 18.8 s instead of 27.4 s, measured side by side.", pr: 33 },
      { title: "Quieter graft new", body: "A layer build prints one line and keeps its output in a log; --verbose streams it, and a failed build shows it.", pr: 33 },
      { title: "Clearer integrate and doctor", body: "graft integrate says when no test gate ran, and graft doctor explains how to quote graft paths that contain a space.", pr: 33 },
      { title: "Reflink grafts match worktrees", body: "The reported 5–10× gap inside reflink grafts on macOS did not reproduce in interleaved runs: git, rg, find and tsc are within noise of a worktree.", pr: 33 },
    ],
    art: "leaf2",
  },
  {
    release: "0.1.0",
    date: "2026-11-12",
    title: "Agents drive Graft over MCP",
    tag: "Feature",
    lede: "graft mcp serves the whole graft lifecycle to Claude Code, Codex, Cursor and any other MCP client over stdio.",
    items: [
      { title: "Lifecycle tools", body: "Agents create grafts, run commands in them, watch Radar, integrate and prune, with no human at the terminal.", pr: 27 },
      { title: "Confirmation for destructive calls", body: "Forced prunes and integrating into Root refuse to run without confirm: true.", pr: 27 },
    ],
    art: "graft",
  },
  {
    release: "0.1.0",
    date: "2026-11-11",
    title: "Scheduler: split work so agents rarely collide",
    tag: "Feature",
    lede: "graft plan reads a task, partitions it along the code's dependency graph and creates grafts that hold claims on their files.",
    items: [
      { title: "Plans by dependency cohesion", body: "Tightly coupled files stay in one node; changes to shared hub files go first, in a contract node.", pr: 26 },
      { title: "Claims", body: "A graft that writes into another graft's claims is refused, and the holder is named.", pr: 26 },
    ],
    art: "branch",
  },
  {
    release: "0.1.0",
    date: "2026-11-07",
    title: "Integrate, Radar, Blueprints and sandboxed runs",
    tag: "Feature",
    lede: "Everything a worktree leaves to you, in one week: a private runtime per graft, live overlap detection, confinement and a tested merge queue.",
    items: [
      { title: "Integrate", body: "Land grafts into Root through a queue: rebase, structural merge with Mergiraf, a test gate, then a plain git commit.", pr: 25 },
      { title: "Radar", body: "Path, symbol and behaviour-level overlaps across live grafts, driven by the op log.", pr: 24 },
      { title: "Blueprints", body: "Ports, Postgres databases, Docker volumes and .env per graft, declared in graft.toml.", pr: 23 },
      { title: "graft run", body: "Confine an agent to its graft: Landlock on Linux, Seatbelt on macOS.", pr: 22 },
      { title: "Bud, Ring and Rewind", body: "Fork, checkpoint and rewind virtual grafts as metadata operations.", pr: 21 },
    ],
    art: "leaf1",
  },
  {
    release: "0.1.0",
    date: "2026-11-06",
    title: "Faster reflink grafts",
    tag: "Performance",
    lede: "Two cuts to graft new on large trees, with every correctness suite unchanged.",
    items: [
      { title: "Cache tree in every graft index", body: "Grafts get the cache tree a worktree index has, so git status skips unchanged directories.", pr: 19 },
      { title: "Parallel layer cloning", body: "Layer trees such as node_modules are cloned on several threads.", pr: 16 },
    ],
  },
  {
    release: "0.1.0",
    date: "2026-11-05",
    title: "Virtual grafts on macOS",
    tag: "Platform",
    lede: "An NFS loopback backend brings virtual grafts to macOS with no kernel or system extension.",
    items: [
      { title: "NFS backend", body: "The same FsBackend trait, delta store and op log as FUSE on Linux.", pr: 10 },
      { title: "Create in milliseconds", body: "On the 200k-file fixture, 4 NFS grafts create in 49 ms where 4 worktrees take 24 s.", pr: 10 },
    ],
    art: "leaf2",
  },
  {
    release: "0.1.0",
    date: "2026-11-04",
    title: "Grafts accept writes",
    tag: "Feature",
    lede: "Writes in a virtual graft are copied up into a content-addressed delta and journaled in an append-only op log.",
    items: [
      { title: "Delta and op log", body: "Replaying the op log reproduces the delta exactly, and a kill -9 of graftd loses at most the in-flight write.", pr: 9 },
      { title: "status, diff and log from the delta", body: "No tree scans: graft status matches git status, read straight from the delta.", pr: 9 },
    ],
  },
  {
    release: "0.1.0",
    date: "2026-11-03",
    title: "Lazy virtual grafts on Linux",
    tag: "Platform",
    lede: "graftd mounts grafts over FUSE and serves files on demand from Root's git object store.",
    items: [
      { title: "graftd", body: "A daemon that owns mounts, speaks JSON-RPC over a Unix socket and remounts on restart.", pr: 7 },
      { title: "Constant-time create", body: "Creating a graft writes metadata only, whatever the size of the repo.", pr: 7 },
    ],
    art: "leaf3",
  },
  {
    release: "0.1.0",
    date: "2026-11-01",
    title: "A benchmark against worktrees",
    tag: "Foundation",
    lede: "One command measures create time, disk, time to first passing test and prune time for grafts, worktrees and clones.",
    items: [{ title: "bench/", body: "Three fixtures from 1k to 200k files and 1 to 16 agents, with CSV and Markdown results.", pr: 6 }],
  },
  {
    release: "0.1.0",
    date: "2026-10-31",
    title: "The first grafts",
    tag: "Foundation",
    lede: "Graft goes from an empty workspace to reflink grafts with shared dependency layers in a day.",
    items: [
      { title: "Layer cache", body: "node_modules and friends are built once per lockfile and reflinked into every graft.", pr: 5 },
      { title: "Reflink grafts", body: "graft new, ls, path, status, diff and prune, with unchanged files sharing disk blocks with Root.", pr: 3 },
      { title: "Root registry", body: "graft init registers a repository; graft records live in a per-Root SQLite store.", pr: 2 },
      { title: "Workspace and CLI", body: "Crates, lint policy, CI on Linux and macOS, and graft doctor.", pr: 1 },
    ],
    art: "tree",
  },
];

const fmt = (iso: string, opts: Intl.DateTimeFormatOptions) =>
  new Date(`${iso}T12:00:00Z`).toLocaleDateString("en-US", { ...opts, timeZone: "UTC" });
const monthKey = (iso: string) => iso.slice(0, 7);
const releaseId = (v: string) => `v${v.replaceAll(".", "-")}`;
const MONTHS = [...new Set(ENTRIES.map((e) => monthKey(e.date)))];

export function Changelog() {
  const [month, setMonth] = useState(MONTHS[0]);
  useEffect(() => {
    const onScroll = () => {
      let current = MONTHS[0];
      for (const el of document.querySelectorAll<HTMLElement>("[data-month]")) {
        if (el.getBoundingClientRect().top < window.innerHeight * 0.3) current = el.dataset.month!;
      }
      setMonth(current);
    };
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  return (
    <div className="rails">
      <Nav current="Changelog" />
      <header className="cl-head">
        <div className="cl-head-art" aria-hidden="true">
          <GraftLogo className="cl-head-logo" title="" />
          <Flora kind="leaf1" className="cl-h-leaf1" style={{ "--r": "-30deg" }} />
          <Flora kind="leaf2" className="cl-h-leaf2" style={{ "--r": "160deg", "--d": "-3s" }} />
        </div>
        <div className="cl-head-copy">
          <h1>What's new?</h1>
          <p>Everything that has landed in Graft, newest first.</p>
        </div>
        <Flora kind="tree" className="cl-h-tree" />
      </header>

      <div className="cl-body">
        <nav className="cl-months" aria-label="Months">
          <b>2026</b>
          {MONTHS.map((m) => (
            <a key={m} href={`#${m}`} aria-current={m === month ? "true" : undefined}>
              <i />
              {fmt(`${m}-01`, { month: "long" })}
            </a>
          ))}
          <b className="cl-releases-head">Releases</b>
          {RELEASES.map((r) => (
            <a key={r.version} href={`#${releaseId(r.version)}`}>
              <i />
              {r.version}
            </a>
          ))}
        </nav>

        <main className="cl-entries">
          {ENTRIES.map((e, i) => {
            const firstOfMonth = ENTRIES.findIndex((x) => monthKey(x.date) === monthKey(e.date)) === i;
            const release = ENTRIES.findIndex((x) => x.release === e.release) === i
              ? RELEASES.find((r) => r.version === e.release)
              : undefined;
            // Entries from the same day share one date and no divider.
            const sameDay = i > 0 && ENTRIES[i - 1].date === e.date;
            const continues = ENTRIES[i + 1]?.date === e.date;
            const cls = ["cl-entry", sameDay && "same-day", continues && "continues"]
              .filter(Boolean)
              .join(" ");
            return (
              <Fragment key={e.title}>
              {release && (
                <div className="cl-release" id={releaseId(release.version)}>
                  <h2>Graft {release.version}</h2>
                  <span>Released {fmt(release.date, { month: "long", day: "numeric", year: "numeric" })}</span>
                </div>
              )}
              <article
                className={cls}
                id={firstOfMonth ? monthKey(e.date) : undefined}
                data-month={firstOfMonth ? monthKey(e.date) : undefined}
              >
                {e.art && (
                  <Flora
                    kind={e.art === "graft" ? "leaf3" : e.art}
                    className={`cl-peek ${i % 2 ? "left" : "right"}`}
                    style={{ "--r": i % 2 ? "24deg" : "-24deg", "--d": `-${i}s` }}
                  />
                )}
                {!sameDay && (
                  <time dateTime={e.date}>{fmt(e.date, { month: "long", day: "numeric", year: "numeric" })}</time>
                )}
                <h2>{e.title}</h2>
                <span className={`cl-tag tag-${e.tag.toLowerCase()}`}>{e.tag}</span>
                <span className="cl-tag tag-release">{e.release}</span>
                <p className="cl-lede">{e.lede}</p>
                <div className="cl-items">
                  {e.items.map((it) => (
                    <div key={it.title} className="cl-item">
                      <h3>{it.title}</h3>
                      <p>{it.body}</p>
                      <a href={`${PR}${it.pr}`}>Shipped in <span>#{it.pr}</span></a>
                    </div>
                  ))}
                </div>
              </article>
              </Fragment>
            );
          })}
        </main>
      </div>
      <Footer />
    </div>
  );
}

