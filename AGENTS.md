# AGENTS.md — Graft

> Read this whole file before writing any code. It is the source of truth for what Graft is, how the repo is organised, how you work, and the order in which things get built. If something here conflicts with a request in a task or issue, stop and ask.

---

## 1. Mission

Graft is a permanent replacement for `git worktree` in parallel agentic work.

A worktree gives every agent a full copy of the working tree plus all its derived state (dependencies, build outputs, databases). Its cost grows with the size of the repo and the number of agents. Graft gives every agent a **copy-on-write view** of one shared **Root**, plus a private **delta** that holds only what that agent changed. Creating, forking and removing a workspace should take about a second regardless of repo size.

On top of cheap workspaces, Graft adds what worktrees leave to the user:

- **Blueprints**: a private runtime per workspace (ports, databases, volumes, `.env`).
- **Radar**: live detection of overlapping edits between agents, while they work.
- **Integrate**: a queue that rebases, merges structurally, tests and lands work into Root.
- **Scheduler**: splits a task along the code's dependency graph so agents rarely collide.
- **Enforcement**: an agent's process can only write inside its own graft.

The output of Graft is always ordinary git history. CI, code review and GitHub must keep working unchanged.

### Non-goals (for now)

- Replacing git as a version control system. Root *is* a git repo.
- Hosting, a cloud service, or a GUI. Graft is a CLI + daemon + MCP server.
- Windows support. Design interfaces so a ProjFS backend can be added later, but do not build it.
- Writing a new merge algorithm from scratch. Reuse existing structural merge tooling.

---

## 2. Vocabulary

Use these terms consistently in code, CLI output, docs, commit messages and PRs.

| Term | Meaning | Code name |
| --- | --- | --- |
| **Root** | The main repository: its git object store plus Graft's layer cache | `Root` |
| **Graft** (noun) | One agent workspace: a base snapshot + a private delta + a Blueprint + a sandbox policy, mounted as a directory | `Graft` |
| **Graft** (verb) | Create a workspace (`graft new`) | — |
| **Grafts** | The list of workspaces (`graft ls`) | — |
| **Live graft** | A graft that is mounted and has an open delta | `GraftState::Live` |
| **Delta** | The files a graft has changed relative to its base | `Delta` |
| **Op log** | Append-only journal of every write, rename and delete in a graft | `OpLog` |
| **Blueprint** | Declarative recipe for a graft's environment, in `graft.toml` | `Blueprint` |
| **Layer** | A shared, read-only, content-addressed directory tree (e.g. `node_modules`) | `Layer` |
| **Layer cache** | Store of layers keyed by the hash of their inputs | `LayerCache` |
| **Prune** | Remove a graft and release its references | `graft prune` |
| **Integrate** | Merge a graft's delta into Root through the queue | `graft integrate` |
| **Bud** | Fork a graft in constant time | `graft bud` |
| **Ring** | A named checkpoint of a graft's delta | `graft ring` |
| **Radar** | The overlap and conflict view across live grafts | `graft radar` |
| **Union** | The result of a successful Integrate | `Union` |

Never call a graft a "worktree", "sandbox" or "clone" in user-facing text.

---

## 3. How you work (agent operating rules)

1. **Work one phase at a time, in order.** Do not start a phase until the previous phase's PR is merged, unless a phase explicitly says it can run in parallel.
2. **Plan before coding.** At the start of a phase, write a short plan in the PR description draft: files you will touch, new types, open questions. If a design decision is not covered here, write an ADR (see §8) before implementing.
3. **Small, reviewable commits.** Each commit compiles and passes tests. Squash noise before opening the PR.
4. **Never push to `main`.** Always work on a branch named per §7 and open a PR.
5. **Tests are not optional.** Every new behaviour ships with tests. Every bug fix ships with a regression test.
6. **Keep `docs/STATUS.md` current.** Tick off acceptance criteria as you meet them.
7. **Do not add dependencies casually.** Each new crate must be justified in the PR description (why it, why not std, maintenance status, license).
8. **Ask, don't guess, when:**
   - an acceptance criterion cannot be met as written;
   - a change would alter the on-disk format after it has shipped;
   - a change touches anything outside the repo (user's home directory beyond `$GRAFT_HOME`, system config, network);
   - you would need `sudo`, a kernel module, or a system extension.
9. **Never destroy user data.** No command may delete files outside `$GRAFT_HOME` or a graft's own delta. Prune must refuse to remove a graft with un-integrated changes unless `--force` is passed.
10. **Report honestly.** If a benchmark misses its target or a test is flaky, say so in the PR. Do not tune tests to pass.

---

## 4. Tech stack

Use the latest stable version of each crate at the time you add it, and record the reason in the PR. Verify a crate is maintained before adopting it.

| Concern | Choice | Notes |
| --- | --- | --- |
| Language | Rust, edition 2024, stable toolchain | Pin the toolchain in `rust-toolchain.toml` |
| Async runtime | `tokio` | Daemon, IPC, MCP server |
| CLI | `clap` (derive) | |
| Errors | `thiserror` in libraries, `anyhow` in binaries | No `unwrap`/`expect` in library code outside tests |
| Logging | `tracing`, `tracing-subscriber` | Structured fields, no `println!` in libraries |
| Git object store | `gix` (gitoxide) | Read objects lazily; never shell out to `git` in hot paths |
| Metadata + op log | SQLite via `rusqlite` (bundled) | One DB per Root; WAL mode |
| Content hashing | `blake3` | For layers and delta blobs |
| FUSE (Linux) | `fuser` | |
| NFS loopback (macOS) | `nfsserve` or equivalent | Evaluate in its phase; write an ADR |
| Reflinks | `FICLONE` ioctl on Linux, `clonefile(2)` on macOS | Fall back to a plain copy with a warning |
| Sandboxing | `landlock` + namespaces on Linux; Seatbelt (`sandbox-exec` profile) on macOS | |
| Code parsing | `tree-sitter` + language grammars | Start with Rust, TypeScript, Python, Go |
| Structural merge | Mergiraf as an external merge driver | Do not vendor it; call it as a process |
| Config | `serde` + `toml` | |
| IPC | JSON-RPC over a Unix domain socket | CLI ↔ daemon |
| MCP | Official Rust MCP SDK (`rmcp`) | |
| Testing | `cargo test`, `insta` (snapshots), `proptest`, `assert_cmd`, `tempfile` | |
| Benchmarks | `criterion` for micro; custom harness for end-to-end | |

### Platform support

| Platform | Status |
| --- | --- |
| Linux x86_64 / aarch64 (btrfs, XFS, ext4) | First class |
| macOS 14+ (APFS) | First class from the Reflink phase; virtual FS from the macOS backend phase |
| Windows | Out of scope; keep the filesystem backend behind a trait |

---

## 5. Repository layout

A Cargo workspace. Crates depend downward only (a crate may depend on crates listed above it, never below).

```
graft/
├── AGENTS.md
├── README.md
├── Cargo.toml                # workspace
├── rust-toolchain.toml
├── deny.toml                 # cargo-deny config
├── graft.toml                # Graft's own Blueprint (dogfooding)
├── crates/
│   ├── graft-core/           # IDs, config, errors, vocabulary types, paths
│   ├── graft-root/           # Root: git object store access, layer cache
│   ├── graft-store/          # SQLite metadata, delta blob store, op log
│   ├── graft-fs/             # FsBackend trait + reflink, FUSE, NFS backends
│   ├── graft-sandbox/        # Process isolation per platform
│   ├── graft-blueprint/      # graft.toml parsing, env provisioning
│   ├── graft-radar/          # Overlap index, symbol mapping, speculative merges
│   ├── graft-integrate/      # Integrate queue, rebase, merge, test gate
│   ├── graft-sched/          # Dependency graph, partitioning, claims
│   ├── graft-daemon/         # graftd: owns mounts, IPC server
│   ├── graft-mcp/            # MCP server exposing Graft to agents
│   └── graft-cli/            # `graft` binary
├── tests/
│   ├── e2e/                  # end-to-end CLI tests against fixture repos
│   └── fixtures/             # small fixture repos (generated by scripts)
├── bench/                    # end-to-end benchmark harness and results
├── docs/
│   ├── STATUS.md             # phase checklist, kept current
│   ├── architecture.md
│   ├── on-disk-format.md
│   ├── blueprint.md
│   └── decisions/            # ADRs: NNNN-short-title.md
└── scripts/
```

### On-disk layout

- `$GRAFT_HOME` defaults to the platform data dir (`~/.local/share/graft` on Linux, `~/Library/Application Support/graft` on macOS), overridable by env var.
- Per Root: `$GRAFT_HOME/roots/<root-id>/`
  - `graft.db` — SQLite: grafts, op log, claims, queue
  - `blobs/` — content-addressed delta blobs (`blake3` hex, sharded by first two chars)
  - `layers/` — layer cache
  - `mounts/<graft-name>/` — mount points
- Root ID is derived from the canonical path of the repo plus its first commit hash.
- Document every on-disk structure in `docs/on-disk-format.md`, versioned. Bump the format version on any change and write a migration.

---

## 6. Engineering standards

### Code

- `cargo fmt` clean, `cargo clippy --all-targets --all-features -- -D warnings` clean. Enable `clippy::pedantic` at workspace level; allow individual lints only with a comment explaining why.
- `unsafe` only in `graft-fs` and `graft-sandbox`, each block with a `// SAFETY:` comment. Add `#![forbid(unsafe_code)]` to every other crate.
- Public items have doc comments. Each crate has a crate-level doc explaining its role.
- No panics across FFI or FUSE callback boundaries. Convert errors to the correct `errno`.
- Every path that touches user files must be canonicalised and checked to be inside the expected directory before use.
- Prefer explicit types for anything persisted (`GraftId`, `RootId`, `BlobHash`), not bare strings.

### Testing

- Unit tests next to code. Integration tests per crate in `crates/<name>/tests/`. End-to-end tests in `tests/e2e/` drive the `graft` binary with `assert_cmd`.
- Filesystem backends are tested through one shared conformance suite that runs against every backend (create, read, write, rename, unlink, symlink, permissions, large files, many files, concurrent writers).
- Tests that need FUSE, NFS or root privileges are gated behind a cargo feature and an env var, and run in CI on a dedicated job.
- Property tests for the delta store and op log (replaying the op log must reproduce the delta exactly).
- No test may touch the real `$GRAFT_HOME`; always use `tempfile`.

### CI (GitHub Actions)

- Matrix: `ubuntu-latest`, `macos-latest`.
- Jobs: fmt, clippy, test, `cargo deny check`, doc build, privileged FS tests (Linux).
- Benchmarks run on demand and on PRs labelled `bench`, never as a required check.

---

## 7. Git workflow

### Branch names

Format: `<type>/<short-kebab-description>`

- `type` is a conventional commit type (§7.2).
- **Never put a phase number, phase name prefix, or date in a branch name.**
- Keep it under 40 characters, lowercase, hyphen-separated.

Good: `feat/reflink-grafts`, `fix/prune-dangling-mount`, `perf/lazy-tree-reads`
Bad: `phase-2-reflink`, `p2/feat-reflink`, `feat/phase2-reflink-grafts`

### Commit messages: Conventional Commits

```
<type>(<scope>): <imperative summary, lowercase, no period, ≤ 72 chars>

<body: what and why, wrapped at 72 chars>

<footer: BREAKING CHANGE: ..., Refs: #issue>
```

- **Types:** `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `build`, `ci`, `chore`.
- **Scope:** the crate name without the `graft-` prefix (`core`, `root`, `store`, `fs`, `sandbox`, `blueprint`, `radar`, `integrate`, `sched`, `daemon`, `mcp`, `cli`), or `repo` for workspace-wide changes.
- Breaking changes use `!` after the scope and a `BREAKING CHANGE:` footer.
- The PR title is the conventional commit title of the squash-merge commit.

### Pull requests

Every PR uses this structure:

```markdown
## Summary
One or two sentences: what this PR delivers.

## Why
The problem it solves, in Graft's vocabulary.

## What changed
- Bullet list of notable changes, grouped by crate.

## How to test
Exact commands a reviewer can run.

## Acceptance criteria
- [ ] Copied from AGENTS.md for this phase, ticked honestly.

## Benchmarks
Numbers, machine, and method, if the phase has performance targets. Otherwise "n/a".

## New dependencies
Crate, version, reason, license. Otherwise "none".

## Not in this PR
Deliberately deferred work, with where it will land.
```

Large phases may be split into several stacked PRs. Each follows the same naming rules and template; the phase is complete when all are merged.

---

## 8. Architecture decision records

Write an ADR in `docs/decisions/NNNN-short-title.md` whenever you choose between real alternatives (a crate, an on-disk format, a protocol, a platform mechanism). Template:

```markdown
# NNNN: Title
Status: proposed | accepted | superseded by NNNN
## Context
## Options considered
## Decision
## Consequences
```

---

## 9. Phases

Each phase lists its goal, scope, acceptance criteria, branch name, commit/PR title and PR description. Performance numbers marked *target* are goals to measure against; report actuals honestly.

---

### Phase 0 — Foundation

**Goal:** a workspace that builds, tests and lints cleanly on Linux and macOS, with a `graft` binary that does nothing useful yet.

**Scope**
- Cargo workspace with all crates from §5 as empty libraries (crate docs only), plus `graft-cli` binary and `graft-daemon` binary.
- `rust-toolchain.toml`, workspace lints, `deny.toml`, `.editorconfig`, `.gitignore`.
- `graft-core`: `GraftId`, `RootId`, `BlobHash` newtypes; `GraftError`; `$GRAFT_HOME` resolution; config loading from `$GRAFT_HOME/config.toml`.
- `graft --version`, `graft --help`, `graft doctor` (prints platform, filesystem type of the repo, reflink support, FUSE/NFS availability).
- CI workflow per §6.
- `docs/STATUS.md` with every phase's acceptance criteria as checklists. `docs/architecture.md` summarising §1–§5.

**Acceptance criteria**
- [ ] `cargo build`, `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check`, `cargo deny check` pass on Linux and macOS in CI.
- [ ] `graft doctor` correctly reports reflink support on APFS and btrfs, and its absence on ext4.
- [ ] Every non-FS crate has `#![forbid(unsafe_code)]`.

**Branch:** `chore/workspace-scaffold`

**PR title:** `chore(repo): scaffold workspace, cli skeleton and ci`

**PR description**
```markdown
## Summary
Sets up the Graft Cargo workspace, a `graft` CLI with `--version`, `--help` and `doctor`, and CI for Linux and macOS.

## Why
Every later phase needs a consistent crate layout, lint policy and CI before real code lands.

## What changed
- Workspace with all planned crates as documented stubs.
- graft-core: ID newtypes, error type, GRAFT_HOME and config resolution.
- graft-cli: clap skeleton and `graft doctor` platform checks.
- CI: fmt, clippy, test, cargo-deny, docs on ubuntu and macos.
- docs/STATUS.md and docs/architecture.md.

## How to test
cargo test --workspace
cargo run -p graft-cli -- doctor

## Acceptance criteria
- [ ] Build, test, clippy, fmt and deny pass in CI on Linux and macOS
- [ ] `graft doctor` reports reflink support correctly
- [ ] forbid(unsafe_code) on all non-FS crates

## Benchmarks
n/a

## New dependencies
(list)

## Not in this PR
Any graft creation; arrives with the Root registry and reflink work.
```

---

### Phase 1 — Root registry and metadata store

**Goal:** Graft can register a git repo as a Root and persist graft records, with no file materialisation yet.

**Scope**
- `graft-root`: open a repo with `gix`, compute `RootId`, resolve refs and commits.
- `graft-store`: SQLite schema v1 (roots, grafts, op log table created but unused), migrations, WAL mode.
- `graft init` registers the current repo as a Root. `graft root` prints Root info.
- Graft records: name, id, base commit, state (`Created`, `Live`, `Integrated`, `Pruned`), timestamps.
- `docs/on-disk-format.md` v1.

**Acceptance criteria**
- [ ] `graft init` is idempotent and refuses non-git directories with a clear error.
- [ ] Schema migrations are tested from an empty DB.
- [ ] Concurrent CLI invocations do not corrupt the DB (test with parallel processes).
- [ ] Graft names are validated (lowercase, digits, hyphens, ≤ 40 chars) and unique per Root.

**Branch:** `feat/root-registry`

**PR title:** `feat(store): add root registry and graft metadata store`

**PR description**
```markdown
## Summary
Registers git repos as Roots and persists graft records in a per-Root SQLite store.

## Why
Every graft operation needs a durable, concurrent-safe record of Roots and grafts before any files are involved.

## What changed
- graft-root: gix-backed repo access and stable RootId.
- graft-store: schema v1, migrations, WAL, typed queries.
- graft-cli: `graft init`, `graft root`.
- docs/on-disk-format.md v1.

## How to test
cargo test -p graft-store -p graft-root
cd tests/fixtures/small && graft init && graft root

## Acceptance criteria
- [ ] init is idempotent; non-git dirs rejected
- [ ] migrations tested from empty
- [ ] parallel CLI calls do not corrupt the DB
- [ ] name validation and uniqueness

## Benchmarks
n/a

## New dependencies
(list)

## Not in this PR
Creating working directories for grafts; next phase.
```

---

### Phase 2 — Reflink grafts

**Goal:** the first useful version. `graft new` creates a workspace by cloning the Root's working tree with filesystem reflinks, so unchanged files share disk blocks.

**Scope**
- `graft-fs`: `FsBackend` trait (`create`, `path`, `diff`, `prune`) and a `ReflinkBackend` using `FICLONE` (Linux) and `clonefile` (macOS), with a copy fallback that warns.
- Each graft gets a git checkout of its base commit at `mounts/<name>/`, using a lightweight git setup that shares Root's object store (alternates), not a full clone.
- Commands: `graft new <name> [--from <ref>]`, `graft ls`, `graft path <name>`, `graft status [<name>]`, `graft diff <name>`, `graft prune <name> [--force]`.
- `graft ls` shows name, base, state, changed-file count and actual disk usage (blocks not shared with Root).
- Prune refuses if the graft has uncommitted or un-integrated changes unless `--force`.

**Acceptance criteria**
- [ ] On APFS and btrfs, creating a graft of a 1 GB working tree uses < 5 MB of new disk (*target*), measured by `graft ls`.
- [ ] Two grafts from the same Root can be created, edited and pruned independently.
- [ ] A graft never shares a git branch lock with Root or another graft.
- [ ] Prune leaves no files, mounts or DB rows behind (verified by test).
- [ ] Copy fallback works on ext4 and prints a single clear warning.

**Branch:** `feat/reflink-grafts`

**PR title:** `feat(fs): create grafts with reflink-backed working trees`

**PR description**
```markdown
## Summary
Adds `graft new/ls/path/status/diff/prune`, creating grafts as reflinked working trees that share disk blocks with Root.

## Why
This is the cheapest path to beating worktrees on disk: unchanged files cost nothing, and grafts are anonymous so there is no branch lock.

## What changed
- graft-fs: FsBackend trait, ReflinkBackend (FICLONE / clonefile), copy fallback.
- graft-root: object store sharing for grafts (no full clone).
- graft-cli: new, ls, path, status, diff, prune.
- Disk-usage accounting that counts only unshared blocks.

## How to test
graft init
graft new auth && cd "$(graft path auth)" && echo x >> README.md
graft ls && graft diff auth && graft prune auth --force

## Acceptance criteria
- [ ] < 5 MB new disk for a 1 GB tree on APFS/btrfs (target; actual: )
- [ ] independent create/edit/prune of two grafts
- [ ] no branch lock conflicts
- [ ] prune leaves nothing behind
- [ ] ext4 copy fallback with one warning

## Benchmarks
(machine, repo, numbers)

## New dependencies
(list)

## Not in this PR
Shared dependency layers; next phase.
```

---

### Phase 3 — Layer cache

**Goal:** dependency trees and build outputs exist once per input hash and are shared by every graft.

**Scope**
- `graft-root`: `LayerCache` keyed by `blake3` of declared inputs (lockfile contents, toolchain version, optional extra files).
- A minimal `graft.toml` with `[[layer]]` entries: `path` (e.g. `node_modules`), `inputs` (files to hash), `build` (command to produce it). The full Blueprint comes later; this phase only parses layers.
- On `graft new`, each layer is built once if missing, then reflinked into the graft. Writes to a layer inside a graft stay private to that graft.
- Reference counting; `graft gc` removes unreferenced layers.

**Acceptance criteria**
- [ ] With a Node fixture, the second and later `graft new` calls do not run the install command.
- [ ] Changing the lockfile produces a new layer; the old one is kept while referenced.
- [ ] `graft gc` never removes a layer referenced by a live graft (tested).
- [ ] Time from `graft new` to first passing `npm test` on the fixture is < 3 s after the first graft (*target*).

**Branch:** `feat/layer-cache`

**PR title:** `feat(root): share dependency and build layers across grafts`

**PR description**
```markdown
## Summary
Adds a content-addressed layer cache so dependency trees like node_modules are built once per lockfile and shared across grafts.

## Why
Per-workspace installs are the biggest disk and cold-start cost of worktrees.

## What changed
- graft-root: LayerCache keyed by blake3 of declared inputs, refcounting.
- graft-blueprint: minimal graft.toml parsing for [[layer]].
- graft-cli: layers applied on `graft new`; `graft gc`.

## How to test
cd tests/fixtures/node-app && graft init
graft new a && graft new b   # second one must not run install
graft gc

## Acceptance criteria
- [ ] install runs once per lockfile
- [ ] lockfile change → new layer, old kept while referenced
- [ ] gc never removes referenced layers
- [ ] < 3 s to first passing test after first graft (target; actual: )

## Benchmarks
(numbers)

## New dependencies
(list)

## Not in this PR
Ports, databases, env; arrive with Blueprints.
```

---

### Phase 4 — Benchmark harness

**Goal:** a repeatable comparison of Graft against `git worktree` and full clones, so every later phase can prove it helps.

**Scope**
- `bench/` harness that, for a given repo and N agents (1, 4, 8, 16), measures: create time, disk added, time to first passing test, prune time.
- Three fixture repos: small (≈1k files), medium Node or Rust project (≈20k files), large (≈200k files, generated).
- Results written as CSV + a Markdown summary in `bench/results/`.

**Acceptance criteria**
- [ ] One command runs the whole matrix and produces the summary.
- [ ] Results are reproducible within ±10% across three runs on the same machine.
- [ ] Summary committed for at least one Linux and one macOS machine.

**Branch:** `test/worktree-benchmarks`

**PR title:** `test(repo): add benchmark harness comparing grafts to worktrees`

**PR description**
```markdown
## Summary
Adds an end-to-end benchmark comparing grafts, git worktrees and full clones on create time, disk, time-to-first-test and prune time.

## Why
Graft's core claim is that workspace cost scales with the edit, not the repo. This makes that claim measurable for every later phase.

## What changed
- bench/ harness and fixture generators.
- CSV and Markdown output.
- First committed results.

## How to test
./bench/run.sh --agents 1,4,8,16

## Acceptance criteria
- [ ] single command runs the full matrix
- [ ] ±10% reproducibility over 3 runs
- [ ] results for Linux and macOS committed

## Benchmarks
(summary table)

## New dependencies
(list)

## Not in this PR
Virtual filesystem; next phase.
```

---

### Phase 5 — Virtual filesystem: read path (Linux)

**Goal:** grafts become virtual. Creating one writes metadata only; files are served lazily from Root's object store.

**Scope**
- `graft-daemon` (`graftd`): long-running process owning mounts, with a JSON-RPC Unix-socket API. The CLI starts it on demand.
- `graft-fs`: `FuseBackend` using `fuser`. Directory listings and file contents come from the git tree of the base commit via `gix`, plus layers from the layer cache.
- Inode table, attribute cache, and a read cache for blob contents.
- Read-only in this phase: writes return `EROFS`.
- The shared backend conformance suite (§6) runs against the read-only operations.

**Acceptance criteria**
- [ ] `graft new` on the large fixture completes in < 1 s regardless of repo size (*target*).
- [ ] `ls -R`, `cat`, `grep -r`, `rg` and `cargo check` / `tsc --noEmit` produce identical results in a graft and in Root.
- [ ] Daemon restart remounts existing grafts.
- [ ] Unmount on prune works even if a process holds files open (lazy unmount with a warning).

**Branch:** `feat/virtual-fs-reads`

**PR title:** `feat(fs): serve grafts lazily from the root object store via fuse`

**PR description**
```markdown
## Summary
Introduces graftd and a FUSE backend that mounts grafts as virtual directories, serving files on demand from Root's git object store.

## Why
Lazy reads make graft creation O(1) in repo size, which reflinks alone cannot do on large repos or non-CoW filesystems.

## What changed
- graft-daemon: graftd process, JSON-RPC over a Unix socket, mount lifecycle.
- graft-fs: FuseBackend (read path), inode and attribute caches.
- Backend conformance suite (read operations).

## How to test
GRAFT_FS=fuse graft new big && cd "$(graft path big)" && rg TODO
cargo test -p graft-fs --features fuse-tests

## Acceptance criteria
- [ ] < 1 s create on large fixture (target; actual: )
- [ ] identical results for ls/cat/grep/rg/type-check vs Root
- [ ] remount on daemon restart
- [ ] safe unmount on prune

## Benchmarks
(numbers vs reflink backend)

## New dependencies
(list)

## Not in this PR
Writes; next phase.
```

---

### Phase 6 — Delta store and op log

**Goal:** grafts accept writes. Every change goes into the graft's private delta and is journaled.

**Scope**
- `graft-store`: delta = map of path → (blob hash | tombstone | symlink | mode change). Blobs stored content-addressed in `blobs/`.
- Copy-up on first write (whole file in this phase; chunking is a later optimisation).
- Op log rows: sequence number, timestamp, op kind, path(s), resulting blob hash, optional actor/tool-call ID supplied by the caller.
- FUSE write path: create, write, truncate, rename, unlink, mkdir, rmdir, symlink, chmod, fsync.
- `graft diff` and `graft status` read from the delta, not by scanning the tree.
- `graft log <name>` shows the op log.

**Acceptance criteria**
- [ ] Full backend conformance suite passes, including concurrent writers.
- [ ] Property test: replaying the op log from empty reproduces the delta exactly.
- [ ] `git status` inside a graft matches `graft status`.
- [ ] Building and running the test suite of the medium fixture inside a graft succeeds.
- [ ] A crash (kill -9 of graftd) mid-write loses at most the in-flight operation; the delta is consistent on restart.

**Branch:** `feat/delta-oplog`

**PR title:** `feat(store): journal graft writes into a private delta and op log`

**PR description**
```markdown
## Summary
Adds the write path: changes in a graft are copied up into a content-addressed delta and recorded in an append-only op log.

## Why
The delta is what makes a graft cost only what an agent changes, and the op log makes every graft's progress visible without entering its directory.

## What changed
- graft-store: delta model, blob store, op log, crash recovery.
- graft-fs: FUSE write operations with copy-up.
- graft-cli: status/diff from the delta, `graft log`.

## How to test
cargo test -p graft-store -p graft-fs --features fuse-tests
graft new w && cd "$(graft path w)" && cargo test && graft log w

## Acceptance criteria
- [ ] full conformance suite incl. concurrent writers
- [ ] op log replay == delta (property test)
- [ ] git status matches graft status
- [ ] medium fixture builds and tests inside a graft
- [ ] crash consistency

## Benchmarks
(write throughput and build time vs Root)

## New dependencies
(list)

## Not in this PR
macOS virtual backend; Bud and Ring.
```

---

### Phase 7 — macOS virtual backend

**Goal:** virtual grafts on macOS without a kernel extension.

**Scope**
- ADR comparing an NFSv3 loopback server, macFUSE and FSKit (if available). Default recommendation: NFS loopback, as EdenFS does.
- `NfsBackend` implementing the same `FsBackend` trait, reusing the delta store and op log.
- Run the backend conformance suite on macOS CI.

**Acceptance criteria**
- [ ] No kernel extension or system extension approval required.
- [ ] Conformance suite passes on macOS.
- [ ] Medium fixture builds and tests inside a graft on macOS.
- [ ] Benchmarks recorded against the reflink backend on the same machine.

**Branch:** `feat/macos-nfs-backend`

**PR title:** `feat(fs): add nfs loopback backend for virtual grafts on macos`

**PR description**
```markdown
## Summary
Brings virtual grafts to macOS through a local NFS server, with no kernel or system extension.

## Why
FUSE on macOS needs a third-party kernel extension; NFS loopback is built in and is what EdenFS uses.

## What changed
- ADR: macOS virtual filesystem mechanism.
- graft-fs: NfsBackend on the shared FsBackend trait.
- macOS conformance job in CI.

## How to test
GRAFT_FS=nfs graft new m && cd "$(graft path m)" && npm test

## Acceptance criteria
- [ ] no kext/system extension
- [ ] conformance suite passes on macOS
- [ ] medium fixture builds and tests
- [ ] benchmarks vs reflink recorded

## Benchmarks
(numbers)

## New dependencies
(list)

## Not in this PR
Sandboxing.
```

---

### Phase 8 — Bud and Ring

**Goal:** constant-time fork, checkpoint and rewind.

**Scope**
- `graft bud <src> <dst>`: new graft sharing the source's base and a copy of its delta index (blobs shared, not copied).
- `graft ring <name> [<label>]`: named checkpoint = op log sequence number + delta index snapshot.
- `graft rewind <name> <ring>`: restore a graft to a ring.
- Blob refcounting across grafts and rings.

**Acceptance criteria**
- [ ] Bud of a graft with 1,000 changed files completes in < 200 ms (*target*).
- [ ] Rewinding then re-applying produces an identical delta (test).
- [ ] Pruning a graft does not remove blobs used by its buds or rings.

**Branch:** `feat/bud-and-ring`

**PR title:** `feat(store): fork, checkpoint and rewind grafts in constant time`

**PR description**
```markdown
## Summary
Adds `graft bud`, `graft ring` and `graft rewind`: forking, checkpointing and rewinding grafts as metadata operations.

## Why
Cheap forks let the scheduler try two approaches to a risky task; rings let an agent back out of a bad path without losing earlier work.

## What changed
- graft-store: delta index snapshots, shared blob refcounts.
- graft-cli: bud, ring, rewind.

## How to test
graft bud auth auth-alt && graft ring auth before-refactor && graft rewind auth before-refactor

## Acceptance criteria
- [ ] bud < 200 ms for 1,000 changed files (target; actual: )
- [ ] rewind/reapply identity
- [ ] prune keeps blobs used by buds and rings

## Benchmarks
(numbers)

## New dependencies
(list)

## Not in this PR
—
```

---

### Phase 9 — Sandboxed run

**Goal:** an agent started through Graft cannot write outside its graft.

**Scope**
- `graft-sandbox`: Linux (Landlock rules + mount and PID namespaces where available, degrade gracefully with a warning) and macOS (generated Seatbelt profile).
- Policy: graft directory read-write; Root and layers read-only via the mount; temp dir private; configurable extra read-write paths (e.g. tool caches) declared in `graft.toml`.
- `graft run <name> -- <command...>` runs any command (e.g. `claude`, `codex`) inside the graft with the policy applied and `GRAFT_NAME`, `GRAFT_ID` set.
- Optional actor tagging: the op log records the `graft run` session ID for every write.

**Acceptance criteria**
- [ ] A test process inside `graft run` fails to write to Root's working tree, another graft, and `$HOME` (outside allowed paths).
- [ ] Common agents and toolchains run normally inside the sandbox (test with a fixture script that runs git, cargo/npm and a network call if allowed).
- [ ] Clear error messages when the kernel lacks Landlock support.

**Branch:** `feat/sandboxed-run`

**PR title:** `feat(sandbox): confine agent processes to their graft`

**PR description**
```markdown
## Summary
Adds `graft run`, which starts any agent or command confined so it can only write inside its own graft.

## Why
With worktrees, isolation is a convention in a prompt. Graft makes it a guarantee.

## What changed
- graft-sandbox: Landlock/namespaces on Linux, Seatbelt profile on macOS.
- graft-blueprint: `[sandbox]` extra paths.
- graft-cli: `graft run`.
- Op log actor tagging per run session.

## How to test
graft run auth -- sh -c 'touch /tmp/ok && touch "$ROOT/escape"'   # second must fail
cargo test -p graft-sandbox

## Acceptance criteria
- [ ] writes outside the graft are denied
- [ ] toolchains run normally
- [ ] clear degraded-mode errors

## Benchmarks
n/a

## New dependencies
(list)

## Not in this PR
Ports, databases, services; next phase.
```

---

### Phase 10 — Blueprints

**Goal:** each graft gets its own runnable environment.

**Scope**
- Full `graft.toml` schema, documented in `docs/blueprint.md`:

  ```toml
  [blueprint.web]
  layers = ["node_modules"]
  setup = ["npm run db:migrate"]
  env_files = [".env.example"]

  [blueprint.web.env]
  DATABASE_URL = "postgres://localhost:{{port.db}}/{{graft.name}}"

  [[blueprint.web.port]]
  name = "app"
  container_port = 3000

  [[blueprint.web.service]]
  name = "db"
  kind = "postgres-template"
  template = "app_dev"
  ```

- Ports: allocate a free host port per declared port and expose it as `{{port.<name>}}`; on Linux optionally a network namespace so the app can bind its usual port.
- Databases: `postgres-template` clones a template database per graft and drops it on prune.
- Docker: namespaced volume and network names per graft.
- `.env` files: copied from Root (ignored files) or rendered from templates.
- `graft new --blueprint <name>`; `graft env <name>` prints the resolved environment.

**Acceptance criteria**
- [ ] Two grafts of the web fixture run their dev servers and test suites at the same time with no port or database collision.
- [ ] Prune removes the graft's database, volumes and port reservations.
- [ ] Template rendering errors are reported with file and line.

**Branch:** `feat/blueprints`

**PR title:** `feat(blueprint): provision a private runtime for every graft`

**PR description**
```markdown
## Summary
Adds Blueprints: a graft.toml recipe that gives every graft its own ports, database, volumes and environment.

## Why
Port, database and .env collisions are the main reason parallel agents cannot run apps and tests side by side today.

## What changed
- graft-blueprint: full schema, templating, port allocation, postgres-template and docker providers.
- graft-cli: `--blueprint`, `graft env`.
- docs/blueprint.md.

## How to test
cd tests/fixtures/web-app
graft new a --blueprint web && graft new b --blueprint web
graft run a -- npm run dev & graft run b -- npm test

## Acceptance criteria
- [ ] two grafts run concurrently without collisions
- [ ] prune cleans DB, volumes, ports
- [ ] template errors include file and line

## Benchmarks
n/a

## New dependencies
(list)

## Not in this PR
Radar.
```

---

### Phase 11 — Radar

**Goal:** overlapping work between live grafts is detected while agents work, not at merge time.

**Scope**
- `graft-radar`: path-level overlap index updated from op log events in the daemon.
- Symbol level: tree-sitter parse of changed files (Rust, TypeScript, Python, Go first) mapping changes to symbols (functions, types, exports); index symbol → grafts.
- Behaviour level: background speculative merge of pairs of overlapping live grafts into a temporary virtual graft, then run a configured fast test command; record pass/fail.
- `graft radar` (table of overlaps by severity) and `graft radar --watch`. Daemon emits events over IPC for the MCP server later.

**Acceptance criteria**
- [ ] Two grafts editing the same function are reported at symbol level within 5 s of the second write (*target*).
- [ ] Edits to different functions in the same file are reported at path level only.
- [ ] A seeded behavioural conflict (both pass alone, fail together) is detected by speculative merge.
- [ ] Radar adds < 5% overhead to write latency (*target*).

**Branch:** `feat/radar`

**PR title:** `feat(radar): detect overlapping edits across live grafts`

**PR description**
```markdown
## Summary
Adds Radar: path, symbol and behaviour-level overlap detection across live grafts, driven by the op log.

## Why
Worktrees hide conflicts until merge time. Because every graft's delta is journaled centrally, Graft can surface them while they are still cheap.

## What changed
- graft-radar: overlap index, tree-sitter symbol mapping, speculative merges.
- graft-daemon: event stream for overlaps.
- graft-cli: `graft radar`, `--watch`.

## How to test
cargo test -p graft-radar
./tests/e2e/radar_seeded_conflict.sh

## Acceptance criteria
- [ ] same-function overlap reported < 5 s (target; actual: )
- [ ] different functions → path level only
- [ ] seeded behavioural conflict detected
- [ ] < 5% write overhead (target; actual: )

## Benchmarks
(numbers)

## New dependencies
(list)

## Not in this PR
Integrate queue.
```

---

### Phase 12 — Integrate

**Goal:** finished grafts land in Root through a safe, ordered queue that produces normal git history.

**Scope**
- `graft-integrate`: persistent queue in SQLite.
- Steps per graft: rebase delta onto Root's current head (conflicts recorded as state, not fatal) → structural merge via Mergiraf as merge driver → optional resolver hook (an external command, e.g. an agent, given both deltas, op logs and a task description) → test gate (configured command; later, affected tests only) → create a commit (conventional message supplied or generated) on Root, or push a branch and open a PR if configured → prune or mark `Integrated`.
- `graft integrate <name...>`, `graft queue`, `graft integrate --dry-run`.
- Live grafts see Root's new head on their next `graft rebase <name>`.

**Acceptance criteria**
- [ ] Non-overlapping grafts integrate with zero manual steps.
- [ ] A reordering conflict that line-based git merge fails is resolved by the structural merge step (fixture).
- [ ] A failing test gate returns the graft to `Live` with the failure output attached.
- [ ] Root never receives a commit that failed the gate (test).
- [ ] Output is plain git history: `git log` and `git fsck` are clean.

**Branch:** `feat/integrate-queue`

**PR title:** `feat(integrate): land grafts into root through a tested merge queue`

**PR description**
```markdown
## Summary
Adds Integrate: a queue that rebases each graft onto Root, merges structurally, runs a test gate and lands it as ordinary git commits.

## Why
Parallel work is only valuable if it can be combined safely. This replaces manual merge-and-pray with an ordered, verified path into Root.

## What changed
- graft-integrate: queue, rebase, Mergiraf driver, resolver hook, test gate, commit/PR output.
- graft-cli: integrate, queue, rebase.

## How to test
./tests/e2e/integrate_two_grafts.sh
graft integrate auth api --dry-run

## Acceptance criteria
- [ ] non-overlapping grafts integrate unattended
- [ ] structural merge resolves fixture conflict
- [ ] failed gate returns graft to Live with output
- [ ] Root never gets a failing commit
- [ ] git log and git fsck clean

## Benchmarks
n/a

## New dependencies
(list)

## Not in this PR
Scheduler.
```

---

### Phase 13 — Scheduler

**Goal:** split a multi-part task into grafts that rarely collide, and order them so interfaces land first.

**Scope**
- `graft-sched`: build a file- and symbol-level dependency graph from tree-sitter (imports, references).
- Hub detection (files most of the graph depends on).
- Partitioning by community detection (e.g. Leiden or Infomap); write an ADR on the algorithm.
- Plan output: ordered task DAG, each node = a set of files/symbols to claim, a suggested graft name, and dependencies. A "contract" node for hub changes runs first.
- Claims: each graft holds soft claims on symbols. A write outside a claim triggers admission against live claims (allowed, queued, or rejected with the conflicting graft named).
- `graft plan <task.md>` prints the DAG; `graft plan --apply` creates the grafts. Cap concurrent grafts at the DAG's width.

**Acceptance criteria**
- [ ] On a fixture task touching three modules, the plan places tightly coupled files in the same node.
- [ ] Hub-file changes are always in the first node.
- [ ] Admission rejects a write to a symbol claimed by another live graft and names that graft.
- [ ] Plans are deterministic for the same input.

**Branch:** `feat/scheduler`

**PR title:** `feat(sched): partition work by dependency cohesion and enforce claims`

**PR description**
```markdown
## Summary
Adds a scheduler that turns a task into an ordered set of grafts partitioned along the code's dependency graph, with symbol claims enforced at write time.

## Why
Research shows agents coordinate poorly by messaging each other. Structure the system enforces works better than negotiation.

## What changed
- graft-sched: dependency graph, hub detection, community partitioning, plan DAG, claims and admission.
- graft-fs: admission check on write.
- graft-cli: `graft plan`, `--apply`.
- ADR: partitioning algorithm.

## How to test
graft plan tests/fixtures/tasks/three-modules.md
graft plan tests/fixtures/tasks/three-modules.md --apply && graft ls

## Acceptance criteria
- [ ] coupled files grouped
- [ ] hubs in first node
- [ ] admission rejects claimed writes and names the holder
- [ ] deterministic plans

## Benchmarks
n/a

## New dependencies
(list)

## Not in this PR
MCP server.
```

---

### Phase 14 — MCP server

**Goal:** any MCP-capable agent can create, use and integrate grafts without a human running commands.

**Scope**
- `graft-mcp` using the official Rust MCP SDK, talking to `graftd` over IPC.
- Tools: `graft_new`, `graft_list`, `graft_status`, `graft_diff`, `graft_run`, `graft_bud`, `graft_ring`, `graft_radar`, `graft_integrate`, `graft_plan`, `graft_prune`.
- Destructive tools (`graft_prune` with force, `graft_integrate` to Root) require an explicit confirmation argument.
- Radar events exposed so an agent can be told when it overlaps another.
- Setup docs for Claude Code, Codex and Cursor in `README.md`.

**Acceptance criteria**
- [ ] An MCP client test harness can run the full cycle: plan → new → run → radar → integrate → prune.
- [ ] Tool schemas are documented and covered by snapshot tests.
- [ ] Destructive tools refuse without confirmation.

**Branch:** `feat/mcp-server`

**PR title:** `feat(mcp): expose graft lifecycle to agents over mcp`

**PR description**
```markdown
## Summary
Adds an MCP server so Claude Code, Codex, Cursor and other agents can drive Graft directly.

## Why
Graft's users are agents. An MCP interface makes Graft agent-agnostic with no per-tool integration.

## What changed
- graft-mcp: tool definitions and handlers over graftd IPC.
- Confirmation gating for destructive tools.
- README setup guides.

## How to test
cargo test -p graft-mcp
graft mcp   # then connect an MCP client

## Acceptance criteria
- [ ] full lifecycle via MCP test harness
- [ ] schema snapshot tests
- [ ] destructive tools require confirmation

## Benchmarks
n/a

## New dependencies
(list)

## Not in this PR
—
```

---

### Phase 15 — Materialize, hardening and first release

**Goal:** a release people can rely on.

**Scope**
- `graft materialize <name> [--branch <b>] [--dir <path>]`: write a graft out as a plain directory or git branch, for tools that cannot run on a virtual FS.
- Chunk-level copy-up for large files (optional if benchmarks justify it).
- Fuzzing of the FUSE/NFS request handlers and the op log parser.
- Final benchmark run vs worktrees, published in `bench/results/` and the README.
- Install via `cargo install`, prebuilt binaries for Linux and macOS, Homebrew formula.
- `docs/` complete: architecture, on-disk format, Blueprint reference, MCP reference.

**Acceptance criteria**
- [ ] Materialize round-trips: materialize → edit → re-import produces the expected delta.
- [ ] 24 h of fuzzing with no crashes.
- [ ] README states measured results, not targets.
- [ ] Release workflow produces signed binaries for both platforms.

**Branch:** `chore/first-release`

**PR title:** `chore(repo): add materialize, hardening and release tooling for 0.1.0`

**PR description**
```markdown
## Summary
Prepares Graft 0.1.0: materialize escape hatch, fuzzing, final benchmarks, docs and release tooling.

## Why
Users need a way out to plain directories for unsupported tools, and evidence before trusting Graft with their repos.

## What changed
- graft-cli: `graft materialize`.
- Fuzz targets for FS handlers and op log.
- Final benchmarks and README results.
- Release workflow and Homebrew formula.

## How to test
graft materialize auth --branch auth-export
cargo fuzz run fs_requests -- -max_total_time=600

## Acceptance criteria
- [ ] materialize round-trip
- [ ] 24 h fuzzing clean
- [ ] README uses measured results
- [ ] signed binaries for Linux and macOS

## Benchmarks
(final table)

## New dependencies
(list)

## Not in this PR
Windows (ProjFS) backend.
```

---

## 10. Definition of done (every PR)

- [ ] Branch name follows §7.1 (no phase numbers).
- [ ] PR title is a valid conventional commit title with the right scope.
- [ ] PR description follows the §7.3 template, with acceptance criteria ticked honestly.
- [ ] fmt, clippy, tests and cargo-deny pass in CI.
- [ ] New behaviour has tests; bug fixes have regression tests.
- [ ] Docs updated (`docs/STATUS.md` always; others as relevant).
- [ ] No new `unsafe` outside `graft-fs` / `graft-sandbox`; every `unsafe` block has a `SAFETY` comment.
- [ ] On-disk format changes are versioned, documented and migrated.
- [ ] Any architectural choice between alternatives has an ADR.

---

## 11. Security and safety rules

- Treat repository contents, `graft.toml`, task files and agent tool-call metadata as untrusted input. Never execute commands from them except the explicitly declared `build`, `setup` and test commands, and only inside `graft run`.
- Never log secrets. Redact values of env vars whose names match `*KEY*`, `*TOKEN*`, `*SECRET*`, `*PASSWORD*`.
- The IPC socket is created with `0600` permissions in a directory only the user can access.
- The MCP server never exposes a tool that runs arbitrary commands outside a graft.
- Prune, gc and integrate never touch paths outside `$GRAFT_HOME`, a graft's own mount, or Root's git refs.

---

## 12. When you are stuck

Write the blocker at the top of the PR description under `## Blocked`, with what you tried, what you observed and the decision you need. Do not work around a blocker by weakening an acceptance criterion or disabling a test.
