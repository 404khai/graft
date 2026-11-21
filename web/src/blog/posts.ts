import type { FigureId } from "../generated/figures";

/** A post is a list of blocks, so it renders as HTML and copies as Markdown from one source. */
export type Block =
  | { p: string }
  | { h2: string }
  | { code: string; lang?: string }
  | { figure: FigureId; caption: string }
  | { list: string[] }
  | { stats: { label: string; value: string; note?: string }[] }
  | { table: { head: string[]; rows: string[][]; caption?: string } }
  | { chart: Chart };

/** A bar chart: one bar per method. `value` is in `unit`; `log` for values spanning orders of magnitude. */
export type Chart = {
  title: string;
  unit: "ms" | "MiB";
  log?: boolean;
  bars: { label: string; value: number; tone: "worktree" | "reflink" | "virtual" }[];
  note?: string;
};

export type Post = {
  slug: string;
  title: string;
  /** short display title for the cover */
  cover: string;
  category: "Engineering" | "Design";
  date: string; // ISO
  summary: string;
  figure: FigureId;
  sources: { label: string; href: string }[];
  body: Block[];
};

export const AUTHOR = {
  name: "404khai",
  handle: "@404khai",
  avatar: "/404khai.png",
  href: "https://404khai.vercel.app",
};

const GH = "https://github.com/404khai/graft/blob/main";

// Inline markup: `code`, **bold**, [text](href). Kept tiny on purpose.
export const POSTS: Post[] = [
  {
    slug: "why-worktrees-break-down",
    title: "Why worktrees break down, and how Graft works",
    cover: "Root & Delta",
    category: "Engineering",
    date: "2026-11-21",
    summary:
      "A git worktree gives every agent a full copy of the repo and everything built from it. That cost grows with the repo and the number of agents. Graft gives each agent a copy-on-write view of one shared Root plus a private delta. Here is how it works, with the numbers.",
    figure: "lazy",
    sources: [
      { label: "README: measured results (bench run 42113580319)", href: "https://github.com/404khai/graft#measured-results" },
      { label: "docs/architecture.md", href: `${GH}/docs/architecture.md` },
      { label: "ADR 0003: reflink graft layout", href: `${GH}/docs/decisions/0003-reflink-graft-layout.md` },
      { label: "ADR 0004: layer cache and build trust", href: `${GH}/docs/decisions/0004-layer-cache-and-build-trust.md` },
      { label: "ADR 0006: virtual graft read path", href: `${GH}/docs/decisions/0006-virtual-graft-read-path.md` },
      { label: "ADR 0008: delta store and op log", href: `${GH}/docs/decisions/0008-delta-store-and-op-log.md` },
    ],
    body: [
      { p: "Running several coding agents on one repository needs one rule above all: agents must not write over each other. The standard answer is `git worktree add`, one per agent. It works, and it is free to try. It also scales with the wrong thing." },
      { h2: "What a worktree actually costs" },
      { p: "A worktree is a full checkout of the repo: every tracked file written to disk. Then the agent needs to run something, so it also needs every file built from the repo: `node_modules`, `target/`, a virtualenv, a database. A worktree shares git's object store with the main checkout, and nothing else." },
      { table: {
        head: ["", "Per worktree", "Grows with"],
        rows: [
          ["Working tree", "every tracked file, written out", "repo size"],
          ["Dependencies", "a full install (`npm ci`, `cargo build`)", "dependency tree size"],
          ["Runtime", "the same ports, database and `.env` as every other worktree", "number of agents (collisions)"],
          ["Branch", "one branch, locked: two worktrees cannot check out the same one", "—"],
          ["Overlap", "invisible until someone merges", "how long agents work"],
        ],
      } },
      { p: "Each of those is fine for one or two agents. With sixteen agents on a 200,000-file repo, the first two rows dominate. In our benchmark, sixteen worktrees of the large fixture, each with its `node_modules` installed, add **22.7 GiB** of disk, and each one takes about 15 seconds to create." },
      { chart: {
        title: "Disk added by 16 workspaces, large fixture (200k files)",
        unit: "MiB",
        log: true,
        bars: [
          { label: "git worktree", value: 23245, tone: "worktree" },
          { label: "graft (reflink)", value: 1946, tone: "reflink" },
          { label: "graft (FUSE)", value: 1.1, tone: "virtual" },
        ],
        note: "Log scale. XFS with reflinks, GitHub Actions ubuntu-latest, medians of three runs.",
      } },
      { p: "The other rows cost differently: in collisions. Two worktrees running the same app fight over port 3000 and the same development database. Two agents editing the same function only find out when one of them tries to merge. The worktree has no idea what the other worktrees are doing." },
      { h2: "Root and delta" },
      { p: "Graft starts from a different question: what does an agent actually own? Not the repository. It owns **what it changed**." },
      { p: "So a graft is two things. **Root** is the repository itself: git's object store, shared and read-only, plus a cache of built layers like `node_modules`. The **delta** is private to one graft: the files that agent has created, changed or deleted, and nothing else. A graft is a directory that shows Root with the delta laid over it. Reading a file that was never changed reads Root; writing one copies it into the delta first." },
      { figure: "lazy", caption: "Every graft reads through to the same git objects. Each one owns only its delta." },
      { p: "A graft costs what the agent changes, not what the repo weighs. Creating one, forking it (`graft bud`) or checkpointing it (`graft ring`) touches metadata, not files." },
      { h2: "Two ways to lay a delta over Root" },
      { p: "Graft has two kinds of backend, because filesystems differ in what they can share." },
      { table: {
        head: ["Backend", "How the graft exists", "Create cost", "Where"],
        rows: [
          ["reflink", "a real checkout whose unchanged files share disk blocks with Root (`FICLONE`, `clonefile`)", "one clone per file: grows with the repo", "APFS, btrfs, XFS; plain copy with one warning on ext4"],
          ["FUSE", "a mount served by `graftd` from git's objects, read lazily; writes go to the delta", "metadata only: constant", "Linux"],
          ["NFS", "the same, over an NFSv3 server on 127.0.0.1 that macOS mounts itself", "metadata only: constant", "macOS, no kernel extension"],
        ],
      } },
      { p: "A virtual graft (FUSE or NFS) builds its directory tree lazily: a directory's entries are read from its git tree object the first time something looks inside it, and a file's contents on its first read. Memory grows with the paths an agent visits, not the size of the repo." },
      { chart: {
        title: "Create one workspace, large fixture (200k files)",
        unit: "ms",
        log: true,
        bars: [
          { label: "git worktree", value: 15120, tone: "worktree" },
          { label: "graft (reflink)", value: 10550, tone: "reflink" },
          { label: "graft (FUSE)", value: 18, tone: "virtual" },
        ],
        note: "Log scale. Median per workspace, 16 created one after another.",
      } },
      { h2: "Every write, journaled" },
      { p: "Every change in a virtual graft goes through `graftd`, which records it in an append-only **op log** and updates the delta in one SQLite transaction. A crash loses at most the write in flight. Replaying the op log from empty rebuilds the delta exactly; a property test checks that on every run." },
      { p: "Because every graft's changes are journaled in one place, Graft can see what all agents are doing without entering their directories. That is what the rest of the system is built on." },
      { h2: "What a worktree leaves to you" },
      { table: {
        head: ["Problem", "With worktrees", "With Graft"],
        rows: [
          ["Dependencies", "install per workspace", "**layers**: built once per lockfile hash, shared by every graft"],
          ["Ports, databases, `.env`", "shared, so they collide", "**Blueprints** in `graft.toml`: a free port, a cloned database and rendered env per graft"],
          ["Overlapping edits", "found at merge time", "**Radar**: same file, same function, or tests that fail only when combined, while agents work"],
          ["Landing work", "merge by hand and hope", "**Integrate**: rebase, structural merge (Mergiraf), test gate, then a plain git commit"],
          ["Agent writes elsewhere", "a rule in the prompt", "`graft run`: Landlock on Linux, Seatbelt on macOS"],
          ["Branch locks", "one branch per worktree", "grafts are anonymous: Root and every graft can have `main` checked out"],
        ],
      } },
      { h2: "The first graft, and the ones after" },
      { p: "Layers change the shape of start-up cost. The first graft of a lockfile builds `node_modules` once; every graft after it gets the layer as a reflink or a pass-through in milliseconds. A worktree pays the install every time." },
      { chart: {
        title: "Ready to test, later agents, medium fixture (20k files)",
        unit: "ms",
        log: true,
        bars: [
          { label: "git worktree", value: 2570, tone: "worktree" },
          { label: "graft (reflink)", value: 1910, tone: "reflink" },
          { label: "graft (FUSE)", value: 153, tone: "virtual" },
        ],
        note: "Create, install if needed, and a passing npm test. Log scale.",
      } },
      { table: {
        head: ["Fixture", "Method", "Create", "Ready, later agents", "Disk added (16)", "Prune"],
        rows: [
          ["small, 1k files", "worktree", "75 ms", "425 ms", "124.4 MiB", "32 ms"],
          ["", "graft (reflink)", "123 ms", "250 ms", "11.8 MiB", "46 ms"],
          ["", "graft (FUSE)", "19 ms", "141 ms", "1.2 MiB", "6 ms"],
          ["medium, 20k files", "worktree", "1.29 s", "2.57 s", "3.0 GiB", "1.12 s"],
          ["", "graft (reflink)", "1.78 s", "1.91 s", "326.1 MiB", "1.15 s"],
          ["", "graft (FUSE)", "19 ms", "153 ms", "45.0 MiB", "6 ms"],
          ["large, 200k files", "worktree", "15.12 s", "15.56 s", "22.7 GiB", "7.63 s"],
          ["", "graft (reflink)", "10.55 s", "10.65 s", "1.9 GiB", "6.97 s"],
          ["", "graft (FUSE)", "18 ms", "135 ms", "1.1 MiB", "6 ms"],
        ],
        caption: "Bench run 42113580319: GitHub Actions ubuntu-latest (AMD EPYC 9V74, 4 threads), XFS with reflinks, three runs, medians.",
      } },
      { h2: "Where grafts still lose" },
      { list: [
        "Reflink grafts create slower than worktrees on small and medium repos: 123 ms against 75 ms, and 1.78 s against 1.29 s. Cloning file by file costs more than writing a fresh checkout until the repo is large.",
        "The first graft of a lockfile is slower than a worktree, because it builds the shared layer.",
        "Inside a workspace, reflink grafts match worktrees on builds and tests but are 1–8% slower on some git and search commands.",
        "Virtual grafts are up to about 40× slower on file-heavy work such as `rg` or writing 10,000 files. That is the next performance target.",
      ] },
      { p: "Graft's output is always ordinary git history. CI, code review and GitHub keep working unchanged; the difference is how much of the repository each agent has to carry to get there." },
    ],
  },
  {
    slug: "graftd-handoff",
    title: "Two graftds, one lock",
    cover: "The Handoff",
    category: "Engineering",
    date: "2026-11-19",
    summary:
      "An intermittent \"not a git repository\" inside a graft came from a command that ran while graftd was still shutting down. The fix was to wait for the lock, not the socket.",
    figure: "handoff",
    sources: [
      { label: "Issue #58: flaky \"not a git repository\" on Linux CI", href: "https://github.com/404khai/graft/issues/58" },
      { label: "PR #65: wait for a stopping graftd to exit", href: "https://github.com/404khai/graft/pull/65" },
    ],
    body: [
      { p: "Virtual grafts are mounts, and `graftd` owns them. If `graftd` stops, the next `graft` command starts a new one, which remounts every live graft. Under load, that went wrong about 2 or 3 times in 60: a `git status` in a graft that should have been remounted ran in an empty directory and failed with **not a git repository** (#58)." },
      { h2: "Shutdown is three steps" },
      { p: "`graftd` stops in order: remove its socket, unmount every graft, then release its lock. The lock is what guarantees one `graftd` per `$GRAFT_HOME`. The socket is how clients find it." },
      { p: "`graft daemon stop` returned as soon as it had asked. A command run straight after it found no socket and started a new `graftd`. That one saw the lock still held and exited, and the command was answered by the stopping one, which then unmounted the graft it had just been asked about." },
      { figure: "handoff", caption: "The socket goes first and the lock goes last; between them, a new graftd cannot start." },
      { h2: "Wait for the lock, not the socket" },
      { p: "A missing socket does not mean no `graftd` is running. A held lock does. So the client now waits on the lock:" },
      { list: [
        "`Client::wait_until_stopped` polls the lock file every 20 ms until no `graftd` holds it.",
        "Starting a `graftd` first waits for a stopping one to exit.",
        "`graft daemon stop` returns once `graftd` has exited, or fails after 30 s.",
        "Restarting an idle `graftd` after an upgrade uses the same wait.",
      ] },
      { stats: [
        { label: "Reproduction under load", value: "0 / 60", note: "was 2/60 and 3/60" },
        { label: "#58 regression test", value: "40 / 40", note: "was 38/40" },
      ] },
      { p: "It shipped in 0.2.0, with an e2e test that stops `graftd` five times in a row and checks that the next command finds the graft remounted." },
    ],
  },
  {
    slug: "lazy-grafts",
    title: "Serving a whole repo without copying it",
    cover: "Lazy Grafts",
    category: "Engineering",
    date: "2026-11-04",
    summary:
      "How a virtual graft mounts in constant time: a lazy inode table over git's object store, FUSE on Linux, and an NFSv3 loopback server on macOS so nobody has to approve a kernel extension.",
    figure: "lazy",
    sources: [
      { label: "ADR 0006: virtual graft read path", href: `${GH}/docs/decisions/0006-virtual-graft-read-path.md` },
      { label: "ADR 0009: macOS virtual grafts over NFS", href: `${GH}/docs/decisions/0009-macos-virtual-grafts-over-nfs.md` },
    ],
    body: [
      { p: "A `git worktree` is a full checkout. Creating one writes every tracked file, so its cost grows with the repo, and sixteen agents means sixteen copies." },
      { p: "Reflink grafts already share disk blocks with Root, but they still cost one stat and one clone per file. On the 200k-file fixture that is seconds, not milliseconds. For `graft new` to stay constant in repo size, a graft cannot write files at all." },
      { h2: "Read from the object store, on demand" },
      { p: "A virtual graft is a mount. Its root directory is the base commit's tree, and nothing else exists until something asks for it. When a directory is first looked up or listed, its entries are read from the git tree object and given inode numbers. File sizes come from object headers; contents are read on the first `read` and kept in a byte-bounded cache." },
      { figure: "lazy", caption: "Every graft reads through to the same git objects. Each one owns only its delta." },
      { p: "Memory grows with the paths an agent actually visits, not with the repo. Inodes are never reused while the graft is mounted, and because a graft's base never changes, the kernel can cache entries and attributes for a long time." },
      { h2: "Filters are the hard part" },
      { p: "Serving raw blobs is wrong for any repo that uses `.gitattributes`: line endings and smudge filters would make `cat` differ from Root. Running every file through the filter pipeline is correct but means `ls -l` reads every blob to learn its converted size." },
      { p: "So Graft runs the pipeline only where attributes can apply: if Root's config sets `core.autocrlf`, `core.eol` or an attributes file, or if the file or one of its ancestor directories contains a `.gitattributes`. Everywhere else it serves the raw blob with its header size." },
      { h2: "macOS without a kernel extension" },
      { p: "On Linux the mount is FUSE, through `fuser`. On macOS, macFUSE needs a kernel or system extension and FSKit needs a signed app the user enables in System Settings. Both are approval steps we did not want." },
      { p: "Instead `graftd` serves each graft as an **NFSv3 export on 127.0.0.1**, and the `mount_nfs` built into macOS mounts it. It is what EdenFS does. An unprivileged user can mount and unmount it on a directory they own: no `sudo`, no extension. The same `GraftView` sits behind both adapters, so reads, the delta, the op log and `.git` behave the same on both." },
      { stats: [
        { label: "Create 4 grafts, macOS NFS, 200k files", value: "49 ms", note: "4 worktrees: 24 s" },
        { label: "Prune a graft, macOS NFS", value: "55 ms", note: "worktree: 12 s" },
        { label: "Disk added, Linux FUSE, 16 agents, large", value: "1.1 MiB", note: "worktrees: 22.7 GiB" },
      ] },
      { h2: "What it costs" },
      { list: [
        "NFSv3 is stateless, so there is no open or close to journal on. A file's first write opens a handle, and it is journaled once writes go quiet.",
        "`nfsserve` answers every write with FILE_SYNC and has no COMMIT, so an `fsync` never reaches `graftd`. Data survives a `graftd` crash but may not survive power loss before it is journaled.",
        "File-heavy work is slower inside a virtual graft than in a worktree today: up to about 40× on `rg` or writing 10,000 files. That is the next performance target.",
      ] },
    ],
  },
  {
    slug: "op-log",
    title: "An op log you can replay",
    cover: "The Op Log",
    category: "Engineering",
    date: "2026-11-03",
    summary:
      "Every write in a graft is a row in an append-only journal, committed with the delta in one SQLite transaction. Why that makes kill -9 boring, and how a property test keeps it honest.",
    figure: "oplog",
    sources: [{ label: "ADR 0008: delta store and op log", href: `${GH}/docs/decisions/0008-delta-store-and-op-log.md` }],
    body: [
      { p: "A graft costs only what an agent changes. That set of changes is the **delta**: a map from path to a file, a symlink, a directory or a tombstone. Every change that produced it is also recorded, in order, in the **op log**." },
      { p: "Two structures holding the same truth is how bugs get in. So we made one of them derivable from the other." },
      { h2: "One function changes a delta" },
      { p: "`Delta::apply(op, lower)` is the only code that mutates a delta. The live write path calls it, and so does replay. A property test generates random operation sequences, replays the stored op log from empty, and checks the result equals the stored delta. A second property test checks the whole overlay against a plain in-memory filesystem model." },
      { figure: "oplog", caption: "Ops are journaled in order; replaying them from empty rebuilds the delta exactly." },
      { code: "create  write  truncate  mkdir  symlink\nunlink  rmdir  rename    chmod  touch", lang: "op kinds" },
      { h2: "One transaction per change" },
      { p: "Every mutation runs under the graft's write lock: compute the new delta entries, write the op log row and the changed delta rows in **one SQLite transaction**, then update the in-memory delta. A crash before the commit loses that one operation. After it, the operation is applied. There is no state in between." },
      { p: "Journaling every `write(2)` would hash and commit a transaction per 128 KiB chunk. Instead the first write copies the file up into a staging file, writes go straight to it, and the file is hashed into a content-addressed blob and journaled as one `write` when it is flushed, fsynced or closed. Staging files are recorded when created, so a restarted daemon finalises them." },
      { stats: [
        { label: "Lost on kill -9 of graftd", value: "≤ 1 op", note: "the one in flight" },
        { label: "Blob store", value: "blake3", note: "blobs/<aa>/<hash>, written then renamed" },
      ] },
      { h2: "Why it matters for agents" },
      { list: [
        "`graft log` shows what an agent did without entering its directory.",
        "`git status` inside a graft matches `graft status`, which is read from the delta, not a tree scan.",
        "Bud and Ring are cheap because a fork or checkpoint is a copy of the delta index, with blobs shared.",
      ] },
    ],
  },
  {
    slug: "git-status-cache-tree",
    title: "The 12 ms git status gap",
    cover: "Cache Tree",
    category: "Engineering",
    date: "2026-11-06",
    summary:
      "git status was slower in a graft than in a worktree on the same machine. Tracing it found one missing index extension, and building it ourselves closed the gap without shelling out to git.",
    figure: "cachetree",
    sources: [{ label: "ADR 0012: cache tree in graft index", href: `${GH}/docs/decisions/0012-cache-tree-in-graft-index.md` }],
    body: [
      { p: "The Phase 8 baseline compared everything an agent does inside a workspace. One result stood out: `git status` on the medium fixture took **30 ms in a graft against 24 ms in a worktree** on XFS, and 42 ms against 36 ms on ext4." },
      { p: "Stat'ing files and walking directories cost the same in both. The difference was in what git did before that." },
      { h2: "Tracing it" },
      { p: "With `GIT_TRACE2_PERF` on a 20,000-file repo, a graft spent about 12 ms in `unpack_trees`, comparing the whole index with HEAD's tree. A worktree spent about 0.5 ms." },
      { figure: "cachetree", caption: "A cache tree lets git skip every directory whose tree id it already knows." },
      { p: "The worktree's index has a **cache tree**, git's `TREE` extension, which records each directory's tree id. The graft's index, written by gix-index, had none, so git checked every entry. gix-index can write a cache tree it already has, but never builds one." },
      { h2: "Building it ourselves" },
      { p: "Running `git read-tree HEAD` after creating a graft would fix it, but shelling out to git on the hot path is something Graft does not do. Waiting for the first `git commit` to build one is too late: agents run `git status` long before they commit." },
      { p: "So Graft builds the cache tree from the base tree's objects, appends the encoded `TREE` extension to the serialised index, and decodes it again with gix. It is verified against the object store before use, and its root entry count must match the index." },
      { code: "git write-tree   # returns the base tree, trusting our cache tree\ngit status       # clean\ngit fsck         # passes", lang: "the test checks" },
      { stats: [
        { label: "unpack_trees, no cache tree", value: "≈ 12 ms" },
        { label: "unpack_trees, with cache tree", value: "≈ 0.5 ms" },
      ] },
      { p: "A commit made in a graft now has exactly the same tree as the same change committed in a worktree, including names whose git order differs from byte order, a submodule, a symlink and an executable." },
    ],
  },
];

export const postBySlug = (slug: string) => POSTS.find((p) => p.slug === slug);

/** Plain-text word count, for reading time. */
export function readingMinutes(post: Post) {
  const words = post.body
    .map((b) => ("p" in b ? b.p : "h2" in b ? b.h2 : "list" in b ? b.list.join(" ") : "code" in b ? b.code : "table" in b ? b.table.rows.flat().join(" ") : ""))
    .join(" ")
    .split(/\s+/).length;
  return Math.max(1, Math.round(words / 220));
}

/** The post as Markdown, for "Copy as Markdown". */
export function toMarkdown(post: Post, url: string) {
  const out = [`# ${post.title}`, "", `_${post.summary}_`, "", `By ${AUTHOR.name} · ${post.date} · ${url}`, ""];
  for (const b of post.body) {
    if ("p" in b) out.push(b.p, "");
    else if ("h2" in b) out.push(`## ${b.h2}`, "");
    else if ("code" in b) out.push("```", b.code, "```", "");
    else if ("list" in b) out.push(...b.list.map((l) => `- ${l}`), "");
    else if ("figure" in b) out.push(`> Figure: ${b.caption}`, "");
    else if ("stats" in b) out.push(...b.stats.map((s) => `- **${s.value}** ${s.label}${s.note ? ` (${s.note})` : ""}`), "");
    else if ("table" in b)
      out.push(`| ${b.table.head.join(" | ")} |`, `| ${b.table.head.map(() => "---").join(" | ")} |`, ...b.table.rows.map((r) => `| ${r.join(" | ")} |`), ...(b.table.caption ? ["", `_${b.table.caption}_`] : []), "");
    else if ("chart" in b) out.push(`**${b.chart.title}**`, "", ...b.chart.bars.map((x) => `- ${x.label}: ${fmtValue(x.value, b.chart.unit)}`), ...(b.chart.note ? ["", `_${b.chart.note}_`] : []), "");
  }
  out.push("Sources:", ...post.sources.map((s) => `- [${s.label}](${s.href})`));
  return out.join("\n");
}

/** 15120 ms -> "15.12 s", 22732 MiB -> "22.2 GiB". */
export function fmtValue(v: number, unit: Chart["unit"]) {
  if (unit === "ms") return v >= 1000 ? `${(v / 1000).toFixed(2)} s` : `${v} ms`;
  return v >= 1024 ? `${(v / 1024).toFixed(1)} GiB` : `${v} MiB`;
}
