import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { Flora, type FloraKind } from "../components/Flora";
import { Footer } from "../components/Footer";
import { GraftLogo } from "../components/GraftLogo";
import { Nav } from "../components/Nav";
import "../styles.css";
import "./changelog.css";

const PR = "https://github.com/404khai/graft/pull/";

type Entry = {
  date: string; // ISO
  title: string;
  tag: "Feature" | "Performance" | "Platform" | "Foundation";
  lede: string;
  items: { title: string; body: string; pr: number }[];
  art?: FloraKind;
};

// From the merged PRs on main.
const ENTRIES: Entry[] = [
  {
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
    date: "2026-11-01",
    title: "A benchmark against worktrees",
    tag: "Foundation",
    lede: "One command measures create time, disk, time to first passing test and prune time for grafts, worktrees and clones.",
    items: [{ title: "bench/", body: "Three fixtures from 1k to 200k files and 1 to 16 agents, with CSV and Markdown results.", pr: 6 }],
  },
  {
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
const MONTHS = [...new Set(ENTRIES.map((e) => monthKey(e.date)))];

function Changelog() {
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
        </nav>

        <main className="cl-entries">
          {ENTRIES.map((e, i) => {
            const firstOfMonth = ENTRIES.findIndex((x) => monthKey(x.date) === monthKey(e.date)) === i;
            return (
              <article
                key={e.title}
                className="cl-entry"
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
                <time dateTime={e.date}>{fmt(e.date, { month: "long", day: "numeric", year: "numeric" })}</time>
                <h2>{e.title}</h2>
                <span className={`cl-tag tag-${e.tag.toLowerCase()}`}>{e.tag}</span>
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
            );
          })}
        </main>
      </div>
      <Footer />
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Changelog />
  </StrictMode>,
);
