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
