# Using graft (0.1.1, brew) for parallel agent work, compared with git worktrees

Context: turborepo checkout (594 MB on disk, no `node_modules`/`target` in Root), macOS on APFS, 2026-11-18.
I used it to run two Claude threads in parallel (#15059 manual-hasher fix, glob-escaping docs). I also tried
bud, ring, rewind, radar, run and prune on throwaway grafts.

## Numbers

| Operation                          | graft                          | `git worktree add`  |
| ---------------------------------- | ------------------------------ | ------------------- |
| Create workspace (reflink backend) | ~25 s wall (4.8 s sys)         | 2.4 s               |
| Create workspace (nfs backend)     | 0.09 s                         | n/a                 |
| Disk per workspace                 | 1.5 MiB (CoW)                  | 137 MB              |
| Fork a workspace with its changes  | `graft bud`, 0.1 s             | no equivalent       |
| `git status` inside workspace      | ~6 s on nfs, normal on reflink | normal              |

## What I loved

- **Disk cost.** 1.5 MiB per graft instead of a full 137 MB checkout. That makes keeping five or more agents
  running cheap.
- **`prune` won't lose work.** It refused to remove a graft with uncommitted changes and told me exactly
  what would be lost. `git worktree remove` makes you remember `--force` and doesn't explain anything.
- **`graft run` sandboxing.** A write to the Root from inside `graft run` failed with
  `Operation not permitted` (seatbelt). Worktrees can't stop an agent from editing the wrong tree. This is the
  feature that matters most for agents.
- **`radar`.** It listed every pair of grafts touching the same file, and it hinted that behaviour checks
  were off because `graft.toml` has no `[radar] test`. Nothing comparable exists for worktrees.
- **`bud` + `ring`/`rewind` (nfs backend).** Forking a graft with its delta takes constant time. Checkpointing
  and rewinding also work, and the rewind can itself be undone (`graft rewind X undo-2`). That covers "try two
  approaches from here" and "undo the agent's last 10 minutes".
- **`doctor`.** One command reported reflink, NFS, sandbox and same-filesystem status, and warned up front
  about the space in the path.
- **Grafts are still normal git.** Each graft is a real checkout on `graft/<name>`, so `gh`, `cargo` and
  hooks work without changes.

## Pain points

1. **The default home path has a space** (`~/Library/Application Support/graft`). Every script and every
   agent prompt has to quote it. I had to tell each thread explicitly. `doctor` warns about it, but a
   default without a space (e.g. `~/.graft`) would be better.
2. **The default (reflink) backend is about 10x slower to create than a worktree** (25 s vs 2.4 s here),
   with almost all of that time outside user CPU. I expected CoW to be the fast option.
3. **Rings don't work on the default backend:** "reflink backend … cannot take rings yet (ADR 0013)". The
   standout feature is disabled unless you know to pass `--backend nfs`.
4. **The nfs backend is fast to create but slow to use.** `git status` took ~6 s every time, which adds
   up for agents that run it constantly.
5. **`graft ring <name>` without a ring name creates a ring.** The help says "or list its rings", so I
   expected a listing and got `ring-2` instead. Listing should not have a side effect.
6. **A daemon version mismatch warning is printed on every command** (`graftd 0.1.0 … graft 0.1.1`). An
   automatic restart after upgrade, or warning once, would be enough.
7. **Build caches aren't shared.** Every graft starts with a cold `target/` and no `node_modules`.
   Blueprints/layers in `graft.toml` look like the answer, but there's no starter config for a
   Cargo + pnpm repo.
8. **Orchestrator handoff.** The MonoCode `sessions.start` call has no `cwd`, so threads start in Root and
   must `cd` into the graft themselves. That's not graft's fault, but a `graft new --exec <cmd>` or a
   documented `graft mcp` setup for harnesses would close the gap.

## What I'd like to see

- A default home path without spaces, or `graft new --path`.
- Rings on reflink, or nfs made the default once `git status` is fast.
- `graft ring --list` (or no-arg listing) with no side effects.
- A `graft.toml` example for Rust + pnpm monorepos that shares `target/` and the pnpm store across grafts.
- `graft run` as the default way to start agents, so the sandbox isn't optional.
- `radar` integrated into `integrate`, so overlapping grafts get flagged before landing.

## Verdict

For parallel agents I'd pick graft over worktrees, mainly for the `run` sandbox, `radar` and the safe
`prune`. The defaults (reflink, a path with a space, rings off) hide its best features. Once the nfs
backend's `git status` speed is fixed, it would beat worktrees on every operation.

_(The two running threads were asked to note their own graft friction; append their notes here when they report.)_
