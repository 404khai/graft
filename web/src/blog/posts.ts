import type { FigureId } from "../generated/figures";

/** A post is a list of blocks, so it renders as HTML and copies as Markdown from one source. */
export type Block =
  | { p: string }
  | { h2: string }
  | { code: string; lang?: string }
  | { figure: FigureId; caption: string }
  | { list: string[] }
  | { stats: { label: string; value: string; note?: string }[] };

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
    .map((b) => ("p" in b ? b.p : "h2" in b ? b.h2 : "list" in b ? b.list.join(" ") : "code" in b ? b.code : ""))
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
  }
  out.push("Sources:", ...post.sources.map((s) => `- [${s.label}](${s.href})`));
  return out.join("\n");
}
