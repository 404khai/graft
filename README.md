# Graft

Copy-on-write workspaces for parallel coding agents: a permanent replacement
for `git worktree`.

Every agent gets a **graft**: a copy-on-write view of one shared **Root** plus
a private delta that holds only what that agent changed. Creating, forking and
pruning a graft is meant to take about a second regardless of repo size. On
top of that, Graft gives each graft its own runtime (ports, databases, `.env`),
detects overlapping edits between agents while they work, and lands finished
work into Root through a tested merge queue. The output is always ordinary git
history.

> **Status:** 0.1.0, the first release. Every phase of the plan is built and
> tested; the performance work (beating worktrees on every workload) is
> deferred to after it, and the measured results below show where grafts
> still lose. See [docs/STATUS.md](docs/STATUS.md) for what is done and
> [docs/architecture.md](docs/architecture.md) for the design.

## Install

- **Prebuilt** (Linux x86_64 and aarch64, macOS arm64): download an archive
  from the [releases](https://github.com/404khai/graft/releases), check it
  with `gh attestation verify <archive> --repo 404khai/graft`, and put `graft`
  and `graftd` on your `PATH`.
- **Homebrew**: each release carries a `graft.rb` formula for a tap.
- **From source** (Rust 1.99): `cargo install --locked --git
  https://github.com/404khai/graft graft-cli graft-daemon`, or from a
  checkout, `cargo install --locked --path crates/graft-cli` and
  `--path crates/graft-daemon` (graftd serves virtual grafts).

`graft doctor` reports what this machine supports.

## Try it

```sh
graft doctor              # can this machine reflink?
graft init                # register this repository as a Root
graft new auth            # a graft checked out at HEAD
cd "$(graft path auth)"   # work in it like any git checkout
graft ls                  # name, base, state, changed files, unshared disk
graft status auth         # what changed since the base
graft diff auth           # as a unified diff
graft prune auth          # refuses if work would be lost; --force to discard
```

Virtual grafts (`graft new --backend fuse` on Linux, `--backend nfs` on
macOS) can also be forked and checkpointed, as metadata operations:

```sh
graft bud auth auth-alt               # fork: same base, a copy of the delta
graft ring auth before-refactor       # checkpoint; `graft ring auth --list`
graft rewind auth before-refactor     # go back; prints how to undo it
```

Run an agent or any command so it can only write inside its graft (and a
private `TMPDIR`): writes to Root, other grafts or `$HOME` fail. Landlock on
Linux, Seatbelt on macOS; tool caches can be allowed in
[`graft.toml`](docs/blueprint.md#sandbox).

```sh
graft run auth -- claude                  # or codex, npm test, cargo build
```

A Blueprint in `graft.toml` gives every graft its own ports, Postgres
database, Docker volumes and `.env` files, so two agents can run the same
app and its tests side by side: `graft new a --blueprint web`, then
`graft env a` ([reference](docs/blueprint.md#blueprintname)).

See overlapping work while agents work, not at merge time: `graft radar`
lists grafts that changed the same file or the same function, type or
export (tree-sitter: Rust, TypeScript, Python, Go), and with a `[radar]
test` command it merges overlapping grafts in scratch checkouts and reports
tests that fail only when combined. `graft radar --watch` keeps it current.

Land finished grafts as ordinary git history: `graft integrate a b` merges
each graft onto the target's head in a scratch checkout (line merge, then
[Mergiraf](https://mergiraf.org) when it is installed), runs the
`[integrate] test` gate, and commits only if it passes. `graft queue` shows
what landed and why anything stopped; `graft rebase b` brings a live graft
up to date.

Split a task before agents start: `graft plan task.md` reads the files and
symbols the task names, partitions them along the code's dependency graph
so tightly coupled files stay together, puts changes to shared hub files in
a first "contract" node, and orders the rest. `graft plan task.md --apply`
creates the grafts that are ready, each holding claims on its files; a
graft that touches another's claims is refused, by name.

### Use it from an agent (MCP)

`graft mcp` serves Graft to any MCP client over stdio: agents create
grafts, run commands in them, watch Radar and integrate, with destructive
calls needing `confirm: true`. The tools and resources are documented in
[docs/mcp.md](docs/mcp.md). Register the Root once with `graft init`, then
point your client at it:

- **Claude Code**: `claude mcp add graft -- graft mcp --root /path/to/repo`,
  or in the repository's `.mcp.json`:

  ```json
  { "mcpServers": { "graft": { "command": "graft", "args": ["mcp", "--root", "/path/to/repo"] } } }
  ```

- **Codex**: in `~/.codex/config.toml`:

  ```toml
  [mcp_servers.graft]
  command = "graft"
  args = ["mcp", "--root", "/path/to/repo"]
  ```

- **Cursor**: in `.cursor/mcp.json` (the same shape as `.mcp.json` above).

Dependency trees are built once and shared: declare them as layers in
`graft.toml` ([reference](docs/blueprint.md)), approve the build command once
with `graft trust`, and every graft after the first gets `node_modules` (or
`target/`) as a reflink in milliseconds. `graft gc` removes layers no graft
uses.

`graft doctor` reports the platform, the filesystem the repository is on,
whether it supports reflinks (tested by cloning a scratch file), where
`$GRAFT_HOME` is, and whether FUSE and NFS are available.

## Measured results

From the benchmark harness (`bench/`), on a GitHub Actions `ubuntu-latest`
runner (AMD EPYC 9V74, 4 threads) on XFS with reflinks, three runs, medians
± spread, 16 agents each creating a workspace one after another, then
`npm ci --offline` and a passing `npm test` (the Phase 8 baseline, run
42113580319; full tables in [bench/results/](bench/results/)):

| fixture | method | create (median) | ready, later agents | disk added (16) | prune |
| --- | --- | ---: | ---: | ---: | ---: |
| small (1k files) | worktree | 75 ms | 425 ms | 124.4 MiB | 32 ms |
| | graft (reflink) | 123 ms | 250 ms | 11.8 MiB | 46 ms |
| | graft (FUSE) | 19 ms | 141 ms | 1.2 MiB | 6 ms |
| medium (20k files) | worktree | 1.29 s | 2.57 s | 3.0 GiB | 1.12 s |
| | graft (reflink) | 1.78 s | 1.91 s | 326.1 MiB | 1.15 s |
| | graft (FUSE) | 19 ms | 153 ms | 45.0 MiB | 6 ms |
| large (200k files) | worktree | 15.12 s | 15.56 s | 22.7 GiB | 7.63 s |
| | graft (reflink) | 10.55 s | 10.65 s | 1.9 GiB | 6.97 s |
| | graft (FUSE) | 18 ms | 135 ms | 1.1 MiB | 6 ms |

What this shows, and what it does not:

- Disk: grafts add 10× (reflink) to 20,000× (FUSE) less than worktrees.
- Virtual grafts create and prune in milliseconds whatever the repo size,
  and later agents are ready in about 140 ms.
- Reflink grafts lose to worktrees on create for small and medium repos,
  and the first graft of a fixture is slower than a worktree because it
  builds the shared `node_modules` layer.
- Inside a workspace, reflink grafts match worktrees on builds and tests but
  are 1–8% slower on some git and search commands; FUSE and NFS grafts are
  up to about 40× slower on file-heavy work (`rg`, writing 10,000 files).
  The deferred performance phase targets these.
- macOS (APFS, on a runner VM that does not reproduce timings): on the large
  fixture an NFS graft creates in 49 ms against 24 s for a worktree and adds
  7.8 MiB against 5.5 GiB for four agents.

## Development

The toolchain is pinned in `rust-toolchain.toml`.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo deny check
```

Tests never touch your real `$GRAFT_HOME`. The privileged filesystem tests
(loop-mounted ext4, btrfs and XFS) run in CI; see
`crates/graft-fs/tests/privileged.rs` to run them locally on Linux.

Contributors, human or agent, start with [AGENTS.md](AGENTS.md).

## License

MIT: see [LICENSE](LICENSE). You may use, change and redistribute Graft,
including in commercial work, as long as copies keep the copyright notice and
the licence text. Contributions are accepted under the same licence.
