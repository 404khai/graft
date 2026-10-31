# Architecture

This is a summary of [AGENTS.md](../AGENTS.md) §1–§5. If the two disagree,
AGENTS.md wins; fix this file.

## What Graft is

Graft replaces `git worktree` for parallel agentic work. A worktree gives
every agent a full copy of the working tree and all its derived state, so its
cost grows with repo size times agent count. Graft gives every agent a
**copy-on-write view** of one shared **Root** plus a private **delta** holding
only what that agent changed. Creating, forking and pruning a graft should take
about a second regardless of repo size.

On top of cheap workspaces Graft adds:

- **Blueprints**: a private runtime per graft (ports, databases, volumes,
  `.env`).
- **Radar**: live detection of overlapping edits between live grafts.
- **Integrate**: a queue that rebases, merges structurally, tests and lands
  work into Root.
- **Scheduler**: splits a task along the dependency graph so agents rarely
  collide.
- **Enforcement**: an agent process can only write inside its own graft.

Graft's output is always ordinary git history; CI, code review and GitHub
work unchanged.

**Not goals (for now):** replacing git, hosting or a GUI, Windows support
(the filesystem backend stays behind a trait so ProjFS can be added later),
or a new merge algorithm.

## Vocabulary

| Term | Meaning | Code name |
| --- | --- | --- |
| Root | The main repository: git object store plus Graft's layer cache | `Root` |
| Graft | One agent workspace: base snapshot + private delta + Blueprint + sandbox policy, mounted as a directory | `Graft` |
| Live graft | Mounted, with an open delta | `GraftState::Live` |
| Delta | Files a graft changed relative to its base | `Delta` |
| Op log | Append-only journal of every write, rename and delete in a graft | `OpLog` |
| Blueprint | Declarative environment recipe in `graft.toml` | `Blueprint` |
| Layer | Shared, read-only, content-addressed directory tree | `Layer` |
| Layer cache | Layers keyed by the hash of their inputs | `LayerCache` |
| Prune | Remove a graft and release its references | `graft prune` |
| Integrate | Merge a graft's delta into Root through the queue | `graft integrate` |
| Bud | Fork a graft in constant time | `graft bud` |
| Ring | Named checkpoint of a graft's delta | `graft ring` |
| Radar | Overlap and conflict view across live grafts | `graft radar` |
| Union | The result of a successful Integrate | `Union` |

User-facing text never calls a graft a "worktree", "sandbox" or "clone".

## Components

```
graft (CLI) ──┐                    ┌── graft-mcp (MCP server, for agents)
              └── JSON-RPC over ───┤
                  a Unix socket    └── graftd (daemon: owns mounts)
                                            │
        ┌───────────────┬───────────────┬───┴───────────┬──────────────┐
    graft-fs       graft-store      graft-root     graft-radar   graft-integrate
  (reflink, FUSE,  (SQLite, blobs,  (gix object    (overlaps)    (queue, merge,
   NFS backends)    op log)          store, layers)                test gate)
```

### Crates

Crates depend downward only: a crate may depend on crates listed above it,
never below.

| Crate | Role |
| --- | --- |
| `graft-core` | IDs (`GraftId`, `RootId`, `BlobHash`), `GraftError`, `$GRAFT_HOME`, config |
| `graft-root` | Git object store access via `gix`; the layer cache |
| `graft-store` | Per-Root SQLite metadata, delta blob store, op log |
| `graft-fs` | `FsBackend` trait; reflink, FUSE and NFS backends; host probing |
| `graft-sandbox` | Process confinement (Landlock + namespaces, Seatbelt) |
| `graft-blueprint` | `graft.toml` parsing and environment provisioning |
| `graft-radar` | Overlap index, symbol mapping, speculative merges |
| `graft-integrate` | Integrate queue, rebase, structural merge, test gate |
| `graft-sched` | Dependency graph, partitioning, claims |
| `graft-daemon` | `graftd`: owns mounts, serves IPC |
| `graft-mcp` | MCP server exposing Graft to agents |
| `graft-cli` | The `graft` binary |

`unsafe` is allowed only in `graft-fs` and `graft-sandbox`, where it is denied
at the crate root and allowed per module with a `SAFETY` comment on every
block. Every other crate has `#![forbid(unsafe_code)]`; a test enforces this.

## Tech stack

Rust (edition 2024, toolchain pinned in `rust-toolchain.toml`); `tokio` for
the daemon, IPC and MCP; `clap` for the CLI; `thiserror` in libraries and
`anyhow` in binaries; `tracing` for logs; `gix` for git objects; SQLite via
`rusqlite` (bundled, WAL) for metadata and the op log; `blake3` for content
hashing; `fuser` on Linux; an NFS loopback server on macOS; `FICLONE` /
`clonefile(2)` for reflinks with a copy fallback; Landlock and Seatbelt for
sandboxing; `tree-sitter` for code structure; Mergiraf as an external merge
driver; `serde` + `toml` for config; `rmcp` for MCP.

| Platform | Status |
| --- | --- |
| Linux x86_64 / aarch64 (btrfs, XFS, ext4) | First class |
| macOS 14+ (APFS) | First class from the reflink phase; virtual FS from the macOS backend phase |
| Windows | Out of scope |

## On-disk layout

- `$GRAFT_HOME`: `$GRAFT_HOME` if set (must be absolute), else
  `$XDG_DATA_HOME/graft` or `~/.local/share/graft` on Linux, and
  `~/Library/Application Support/graft` on macOS.
- `$GRAFT_HOME/config.toml`: optional user config (see `graft-core`).
- `$GRAFT_HOME/roots/<root-id>/` per Root:
  - `graft.db`: SQLite (grafts, op log, claims, queue)
  - `blobs/`: delta blobs by `blake3` hex, sharded by the first two characters
  - `layers/`: the layer cache
  - `mounts/<graft-name>/`: mount points
- A Root ID is derived from the repo's canonical path plus its first commit.

Every persisted structure is documented and versioned in
`docs/on-disk-format.md` (from the Root registry phase on).

## Repository layout

```
crates/         one directory per crate above
tests/e2e/      end-to-end tests driving the `graft` binary (built as the
                `e2e` test target of graft-cli so Cargo provides the binary)
tests/fixtures/ fixture repos, generated by scripts
bench/          end-to-end benchmark harness and results
docs/           STATUS.md, this file, ADRs in decisions/
scripts/        CI and developer helpers
```
