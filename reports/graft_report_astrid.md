# Graft field report: two features in parallel on Astrid

Date: 2026-11-15 · graft 0.1.1 (Homebrew) · macOS 27 (Darwin), APFS, Apple Silicon
Agent: Claude Code (Opus 5.5) · Repo: `astrid` (Rust + Swift + TypeScript, 13 GB `target/`)
Result: [PR #53](https://github.com/404khai/astrid/pull/53), personal memory (ADR 0040) and the wake word (ADR 0034 amendment)

## How I used it

- **Two grafts**, `personal-memory` and `wake-word`, both from `main` at `ee61878f`, reflink backend.
- **Driven over MCP.** This session was already running, so I couldn't attach a new MCP server to it. Instead I drove `graft mcp` over stdio JSON-RPC with a small client: `initialize`, then `tools/call`, timing every call. Every graft operation below went through MCP except `graft init` (MCP has no init) and `graft doctor`.
- **"Parallel" means interleaved.** I'm one agent, so I alternated between the two tasks while their tests ran in the background. The isolation was real: each graft had its own tree, its own build and its own tests. The work wasn't done by two separate agents.

## Metrics

All times are wall clock, measured by the MCP client. The MCP handshake (`initialize`) took 5–10 ms every time.

| Operation | Graft / args | Time | Result |
| --- | --- | ---: | --- |
| `graft init` (CLI) | register Root | **10 ms** | ok |
| `graft_new` | personal-memory | **63 ms** | ok, 14 MB tree (tracked files only) |
| `graft_new` | wake-word | **63 ms** | ok |
| seed `target/` + `web/node_modules` (`cp -c`, outside graft) | each graft | **30.1 s / 31.6 s** | ok, APFS clones |
| `graft_run` `cargo test memory` | personal-memory | **32.8 s** | ok, 4/4 (incremental build on the cloned target) |
| `graft_run` `swift test` | wake-word | **9.5 s** | **failed**: sandbox blocked clang's module cache in `/var/folders` |
| `graft_run` `swift test` + module cache moved into the graft | wake-word | **44.0 s** | **failed**: sandbox blocked `xcrun_db` and SwiftPM's user caches |
| `swift test` run directly in the graft path (unconfined) | wake-word | 14 s / 4 s full suite | ok, 45/45 |
| `graft_status` | all | **36 ms** | ok |
| `graft_radar` | all | **24 ms** | no overlaps (correct: the files didn't overlap) |
| `graft_integrate` dry run, branch missing | both | **8 ms** | error `failed`: "no branch feat/memory-and-wake" |
| `graft_integrate` dry run | both | **285 ms** | `checked`, "no test gate" |
| `graft_run` fmt + clippy + full `cargo test` | personal-memory | 35.7 s, then **19.5 s** | 154/156: 2 operator tests failed **only under the sandbox** (both pass unconfined) |
| `cargo test` full, unconfined in the graft | personal-memory | 65 s | 437 passed, 1 ignored |
| `graft_integrate` (confirm) | personal-memory | **312 ms** | landed `70e851dd` |
| `graft_integrate` (confirm) | wake-word | **298 ms** | landed `1a2b56cd` |
| `graft_list` | all | **10.4 s** | ok; it reports `disk_bytes` 3.0 GB and 203 MB |
| `graft_prune` | personal-memory | **12.8 s** | pruned |
| `graft_prune` | wake-word | **11.6 s** | pruned |

**End to end:** about 20 minutes from the first `graft_new` (19:38:44) to both landed (19:57:49), including all the coding. Graft's own fast operations (new, status, radar, integrate) added under 1 s in total. Most of the overhead came from seeding dependencies (~62 s), which graft didn't do for me, and from list/prune (~35 s).

## What I loved

- **`graft_new` at 63 ms with JSON back.** Getting `{name, path, base, backend}` meant no output parsing and no guessing paths. I created both workspaces before thinking about either task.
- **`graft_integrate` is the standout.** It merged, committed with my message, landed on a named branch, reported `"no test gate ([integrate] test is not set)"` honestly, and refused to touch Root's dirty checkout. That last part mattered: Root had uncommitted user files, and with worktrees I would have had to cherry-pick or merge by hand. The dry run predicted the real run exactly.
- **Radar and status are cheap enough to run all the time** (24 ms and 36 ms). An empty overlap list was real reassurance before landing two branches together.
- **The errors were structured and actionable.** Missing Root, missing branch and similar failures each came back as one clear sentence with a code.
- **`graft prune` checks before it deletes.** It knew both grafts were `integrated`, so it didn't need `force`.
- **The integrate commit contains only tracked changes.** The 13 GB of cloned `target/` never leaked into the commit.

## Pain points (honest)

1. **`graft_run`'s sandbox can't build Swift on macOS.** Clang's module cache, `xcrun_db` and SwiftPM's user caches all live outside the graft. Moving the module cache into the graft wasn't enough. The `[sandbox] write` allowlist is read from `graft.toml` at the base commit, and the MCP `graft_run` has no per-call `--write` (the CLI does). So for an Xcode toolchain project, the confined path is effectively unusable without first committing a `graft.toml`.
2. **The sandbox causes test failures that look like real ones.** Two operator tests that spawn helper processes failed under `graft_run` and passed unconfined. Graft gave no hint that the sandbox caused it. An agent less careful than it should be could "fix" code that isn't broken.
3. **`graft_run` only reports exit codes.** That's correct, but my `sh -c '... | tail'` wrappers hid a failing `swift test` behind exit 0. That was my mistake, but a `run` mode with structured test summaries, or `pipefail` by default for `sh -c`, would have caught it.
4. **Dependencies aren't there on day one.** A new graft has no `target/` and no `node_modules`. Layers solve this, but only through `graft.toml` read from a commit, so I'd have had to commit config to `main` before starting. I worked around it with `cp -c` (~30 s per graft for 13 GB). A `graft_new` option like "clone these ignored dirs from Root" (reflink, no build) would give that for free.
5. **`graft_list` takes 10 s, and `graft_prune` takes 12 s per graft.** Both are dominated by walking or deleting the cloned 13 GB `target/` (`disk_bytes`). List should be instant, with `disk_bytes` optional or computed in the background. Prune could rename first and delete asynchronously.
6. **There's no `graft_init` over MCP.** The first MCP call fails on an unregistered repo, so the agent needs the CLI anyway.
7. **`integrate onto` needs the branch to exist already.** A `create: true` option would save a step.
8. **Paths contain spaces** (`~/Library/Application Support/graft/...`). `graft doctor` warns about it. Every shell call needed careful quoting, and a few tools behave worse. A default without spaces (`~/.graft`) would remove that class of bug.
9. **There's no MCP `cwd` or "open" helper.** Each shell step needed the long mount path. An env var or a `graft_path` tool would help a little (the CLI has `graft path`).

## What I'd like to see

- `graft_run` with a per-call `write` list over MCP, and a built-in macOS developer-tools sandbox profile (Xcode, SwiftPM, clang module caches, `xcrun`).
- When a confined run fails, a hint such as "N sandbox denials during this run". `sandbox-exec` denials can be counted.
- `graft_new` with `clone_ignored: ["target", "web/node_modules"]` (reflink from Root), and layers configurable without a commit, from the operator's `config.toml`.
- A fast `graft_list` (disk size opt-in) and asynchronous prune.
- `graft_init` over MCP, and `integrate` that can create the target branch.
- A test gate set per call (`graft_integrate {test: [...]}`), so I can gate without committing `graft.toml`.

## Compared with git worktrees (my estimate; I didn't run a side-by-side)

| | git worktree | graft (as used) |
| --- | --- | --- |
| Create two workspaces | ~0.2–1 s each (checkout of 14 MB) | 63 ms each |
| Dependencies | Same problem; same `cp -c` workaround (~30 s) | Same, unless layers are committed |
| Building and testing | Unconfined, so Swift just works | Confined path broke Swift and two tests; I fell back to unconfined runs |
| Overlap check before landing | Manual (`git diff --name-only` × 2) | `graft_radar`, 24 ms, JSON |
| Landing two branches on a new branch with a dirty Root | Manual: commit in each worktree, create the branch, merge or cherry-pick twice, careful with Root's dirty files | Two `graft_integrate` calls, ~300 ms each, Root untouched |
| Cleanup | `git worktree remove` (fast, but `target/` deletion is just as slow) | `graft_prune`, ~12 s each, with a check that work isn't lost |
| Agent ergonomics | Text output, parsing | JSON over MCP, error codes |

**Net effect:** for this two-task job, graft probably saved **3–5 minutes** of git bookkeeping (branching, committing, landing, cleanup) and, more importantly, removed several ways to make mistakes with a dirty Root. The sandbox cost about **2 minutes** of dead ends (two failed Swift runs, two sandbox-only test failures). Seeding dependencies was a wash with worktrees. Graft's biggest real wins were integrate, radar and the JSON MCP surface. Its biggest real cost was `graft_run` on a macOS Swift toolchain.

**Would I use it again for parallel agent work?** Yes, especially with several agents, where radar and the integrate queue matter most. But for a mixed Rust and Swift repo on macOS I'd skip `graft_run` until the sandbox has a developer-tools profile, and I'd commit a `graft.toml` with a `target/` layer up front.
