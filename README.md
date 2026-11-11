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

> **Status:** early development. Grafts are real working trees whose
> unchanged files are reflinks of Root's; virtual (lazy) grafts come later. See
> [docs/STATUS.md](docs/STATUS.md) for progress and
> [docs/architecture.md](docs/architecture.md) for the design.

## Try it

```sh
cargo install --path crates/graft-cli   # installs `graft`
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

Dependency trees are built once and shared: declare them as layers in
`graft.toml` ([reference](docs/blueprint.md)), approve the build command once
with `graft trust`, and every graft after the first gets `node_modules` (or
`target/`) as a reflink in milliseconds. `graft gc` removes layers no graft
uses.

`graft doctor` reports the platform, the filesystem the repository is on,
whether it supports reflinks (tested by cloning a scratch file), where
`$GRAFT_HOME` is, and whether FUSE and NFS are available.

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

Not yet chosen.
