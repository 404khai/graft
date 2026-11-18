# 0038: Rings and rewind on reflink grafts
Status: accepted (amends 0013 for ring and rewind)

## Context

ADR 0013 made bud, ring and rewind features of virtual grafts, and
reflink grafts refuse them (option R2, chosen with the owner). Reflink is
the default backend, so the Turborepo field report
(`reports/graft_report_turborepo.md`) found "the standout feature is
disabled unless you know to pass `--backend nfs`". The owner asked for
rings on the default backend.

ADR 0013 rejected R1 (copy changed files into `blobs/` as delta rows)
because bud would cost a full clone, rewind rewrites a live working tree,
and there is no op log for a ring's sequence number.

## Options considered

- A. Keep R2.
- B. R1 as ADR 0013 described it.
- C. Rings as git snapshots: commit the working tree as git sees it
  (`git add -A` into a scratch index, so tracked and untracked files, not
  ignored ones) with `HEAD` as parent, under the per-worktree ref
  `refs/worktree/graft/rings/<label>`. Rewind restores files and `HEAD`
  from that commit. Bud stays virtual-only.

## Decision

C.

- **Ring** writes the snapshot commit without touching the real index,
  then creates the ref (refusing one that exists). Labels follow ring
  rules; the default is `ring-<n>`; `undo-*` is reserved. `seq` is null
  (there is no op log); `entries` counts paths that differ from `HEAD`.
- **Rewind** first rings the current state as `undo-<n>`, then removes
  files the current state has and the ring does not (never through a
  symlink), writes the ring's files (`checkout-index` from a scratch
  index), and runs `git reset --mixed` to the ring's parent, so `HEAD`, its
  branch and the index return to where they were. Commits made since stay
  reachable from the undo ring.
- **Ignored files are never part of a ring and never touched by a
  rewind**: build outputs and layers (which are ignored in the graft) stay
  as they are.
- Per-worktree refs live in the graft's own `.git`, or in its worktree
  registration (ADR 0036), so they go with the graft and never appear in
  Root's refs.
- **Bud** keeps refusing reflink grafts (ADR 0013's reason holds: it needs
  a clone of the working tree).

## Consequences

- `graft ring`, `graft ring --list` and `graft rewind` work on every graft;
  over MCP too.
- A rewind rewrites files a process may have open, as any `git checkout`
  does; the undo ring keeps what was there.
- Unlike virtual rings, these do not cover ignored files and have no op
  sequence number.
- Tests: `reflink_rings_snapshot_and_rewind_the_working_tree`
  (`tests/e2e/grafts.rs`), `rings_of_a_registered_graft_stay_in_its_worktree`
  (`tests/e2e/worktrees.rs`), and the MCP cycle.
