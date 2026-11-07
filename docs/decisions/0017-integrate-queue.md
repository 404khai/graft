# 0017: Integrate queue
Status: accepted

## Context

AGENTS.md Phase 13: finished grafts land in Root through a safe, ordered
queue that produces ordinary git history. Per graft: rebase its changes
onto Root's current head (conflicts are state, not fatal), merge
structurally with Mergiraf, optionally run a resolver hook, run a test
gate, create a commit on Root (or a branch), then mark the graft
`Integrated` or prune it. `graft integrate <name...>`, `graft queue`,
`graft integrate --dry-run`, and `graft rebase <name>` for live grafts.
Acceptance: non-overlapping grafts integrate with no manual step; a
reordering conflict that a line merge fails is resolved structurally; a
failing gate returns the graft to `Live` with the output attached; Root
never receives a commit that failed the gate; `git log` and `git fsck`
stay clean.

AGENTS.md §11 allows Integrate to touch only `$GRAFT_HOME`, a graft's
own mount and Root's git refs: never Root's working tree.

## Options considered

**Where the merge happens**

- A. In Root's working tree (`git merge`). Fast and familiar, but it
  writes Root's files, which §11 forbids, and a failed gate would leave
  Root mid-merge.
- B. In a scratch reflink checkout of the target head under
  `$GRAFT_HOME/roots/<root-id>/integrate/` (the same machinery as Radar,
  ADR 0016). Root sees nothing until the gate has passed and one ref is
  updated with compare-and-swap.

**Which changes**

- C. The graft's commits, cherry-picked. Misses uncommitted work and does
  not exist for virtual grafts with no `.git` yet.
- D. The graft's working state against its base, as `graft diff` shows it
  (committed and uncommitted, both backends), landed as one commit.

**File-level merge**

- E. Mergiraf as a git merge driver inside `git merge`. Needs a real merge
  in a repository with `.gitattributes` set up.
- F. Per changed file: if only one side changed it, take that side;
  otherwise `git merge-file`; if that conflicts, `mergiraf merge base ours
  theirs` (when Mergiraf is installed and knows the language); if that
  still conflicts, the resolver hook; otherwise the conflict is recorded.

**The branch checked out in Root**

- G. Update its ref anyway. Root's working tree and index then show the
  integration as a reverse change: confusing, and the next commit there
  could undo it.
- H. Follow git's own rule (`git fetch` refuses to update a checked-out
  branch): refuse to move the branch Root has checked out, unless the
  operator passes `--update-checkout`. With that flag, and only if Root's
  working tree is clean, Graft fast-forwards it with `git merge
  --ff-only`, the one place Integrate writes Root's files, and only at
  the operator's explicit request on the command line.

## Decision

B + D + F + H.

- **Queue.** Schema v9 table `integrations`: one row per request, with the
  target branch, message, state (`queued`, `running`, `conflict`,
  `failed`, `landed`, `dry-run`), the resulting commit and the output of
  the step that stopped it. `graft integrate a b` enqueues `a` and `b` in
  order and works through the queue holding `integrate.lock`, so
  concurrent calls serialise and land in order. A failure or conflict
  stops only its own entry.
- **Steps.** Check out the target's head in scratch; apply the graft's
  changes with the file merge (F); run `[integrate] resolver` for
  remaining conflicts; run `[integrate] test` (the gate) confined, as
  Radar runs its tests; commit with `git commit` in the scratch checkout
  (Root's identity, through the included config); copy the objects into
  Root with `git fetch`; move the target ref with `git update-ref <ref>
  <new> <old>`, which fails if Root moved meanwhile (the entry is then
  retried from the new head, once).
- **Commit message.** `--message`, else the graft's own commits' first
  subject since its base, else `chore: integrate graft <name>`; a
  `Graft: <name> <id>` trailer is always added.
- **After landing** the graft becomes `Integrated` (or is pruned with
  `--prune`). `[integrate] mode = "branch"` instead creates or moves
  `refs/heads/graft/<name>` in Root and leaves the target alone.
- **Failure.** A failing gate or an unresolved conflict leaves the graft
  `Live`; `graft queue` shows the output. Root's refs are untouched.
- **Trust.** The gate and the resolver are commands from the repository,
  so `graft trust` must approve them; an unapproved gate stops the entry
  rather than skipping the tests.
- **`graft rebase <name>`** moves a reflink graft to the target's head:
  the same file merge into its working tree, then its base, pin ref and
  `.git` (branch and index, with `git reset --mixed`) move to the new
  head; files only Root changed are checked out with git (modes,
  symlinks, filters kept); conflicts are left as conflict markers and
  listed.
- **Rebasing a virtual graft** keeps the op log exact with a new op,
  `rebase` (the new base in `target`): replay starts again from empty
  there. The graft is unmounted (refused while files are held open), each
  changed file is merged against the new head, and in one transaction
  the `rebase` op, the ops that recreate the merged files over the new
  base's tree (`graft_fs::rebase_ops`: parent directories, type changes,
  blobs) and the new base are journaled; then the pin moves, the graft is
  remounted and its `.git` reset to the new head. Rings taken before the
  last rebase cannot be rewound to (their rows belong to the old base).
- **Symlinks** are never merged: a path that is a symlink on either side
  is taken from the side that changed it, written as a symlink; both
  changing it is a conflict.
- **Push mode** (`mode = "push"`, `remote`, `pr`) is the only network
  access in Integrate (AGENTS.md §3.8). It is never the default: choosing
  it in graft.toml, which the operator reviews, is the consent. It lands
  on `graft/<name>` as `branch` mode does, pushes it with
  `--force-with-lease` and, with `pr = true`, runs `gh pr create`; without
  `gh` the branch is still pushed and the queue says so.

## Consequences

- Root's working tree is never written, except by `--update-checkout`.
- Each integration is one commit with the graft's net change.
- Mergiraf is optional at run time: without it, line-level conflicts stay
  conflicts and say that Mergiraf would be tried.
- On-disk format 11: `integrate/` scratch directories, schema v9, and the
  `rebase` op log kind.
