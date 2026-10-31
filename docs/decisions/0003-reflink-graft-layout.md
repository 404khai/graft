# 0003: Reflink graft layout
Status: accepted

## Context

The reflink backend gives each graft a real working tree at
`$GRAFT_HOME/roots/<root-id>/mounts/<name>/`, checked out at a base commit.
AGENTS.md asks that:

- unchanged files share disk blocks with Root (< 5 MB new disk for a 1 GB
  tree on APFS/btrfs);
- the graft's git setup shares Root's object store instead of cloning it;
- a graft never shares a branch lock with Root or another graft;
- `graft ls` reports actual disk use, counting only blocks not shared.

Four questions follow: what git layout to use, where file contents come
from, how to keep the base commit alive, and how to measure unshared blocks.

## Options considered

**Git layout**

- A. `git worktree add`. Shares Root's refs, so two worktrees cannot check
  out one branch: this is exactly the branch lock to avoid. Also writes
  administrative files into Root's `.git/worktrees/`.
- B. `git clone --shared`. Uses alternates, but copies refs and runs a
  checkout that writes every file from the object store, sharing no blocks.
- C. Build a minimal repository directly: `.git/HEAD`, `.git/config`,
  `.git/objects/info/alternates` pointing at Root's object directory, one
  branch, and an index written by Graft. Graft has full control over how
  files are created.

**File contents**

- D. Write every file from the object store. Correct, but shares nothing.
- E. Reflink Root's working tree wholesale. Shares everything, but copies
  Root's uncommitted edits and untracked or ignored files (`target/`,
  `node_modules/`), and is wrong whenever the base is not Root's HEAD.
- F. Per file: reflink from Root's working tree when that file is known to
  hold exactly the base commit's blob; otherwise write it from the object
  store through git's filter pipeline (CRLF, smudge filters).

**Keeping the base alive**

- G. Nothing. If Root's branch is deleted and `git gc` prunes, the graft's
  base objects vanish under it.
- H. A ref in Root per graft, `refs/graft/<graft-id>`, created by `graft new`
  and deleted by `graft prune`. AGENTS.md §11 allows Graft to touch Root's
  git refs.

**Unshared blocks**

- I. `st_blocks`. Counts shared blocks as if they were private.
- J. Per-file, platform-native: `FIEMAP` on Linux (extents without
  `FIEMAP_EXTENT_SHARED`), `ATTR_CMNEXT_PRIVATESIZE` via `getattrlist` on
  macOS. Fall back to `st_blocks` where neither works (ext4 has nothing to
  share, so this is exact there).

## Decision

C + F + H + J.

- **Layout.** `.git/config` includes Root's config (`[include] path`), so
  filters, identity and remotes behave as they do in Root, then sets the
  keys a graft needs. It also adds a `root` remote pointing at Root. HEAD is
  on a branch `graft/<name>` that exists only inside the graft's repository,
  so it can never contend with Root's branches. A shallow Root's `shallow`
  file is copied. Roots that set `core.worktree` are refused, because the
  include would point the graft at Root's working tree.
- **Contents.** Graft runs a status of Root's index against its working tree
  (`gix` status: stat comparison with content hashing for racy entries). A
  base-tree entry is reflinked from Root only if Root's index has the same
  blob and mode at that path and the file is clean. Immediately before and
  after the clone, the source's stat must still match Root's index entry;
  otherwise the file is written from the object store instead. Everything
  else is written from the object store through the graft's filter pipeline.
  Tree paths are validated with `gix-validate` (no `..`, no `.git`, NTFS and
  HFS tricks) before anything is created. Symlinks and submodule
  directories are created last, so no file write can traverse a symlink
  that came from the tree.
- **Fallback.** If the filesystem cannot reflink, or `$GRAFT_HOME` is on a
  different filesystem from Root, files are copied and `graft new` prints
  one warning.
- **Atomicity.** The tree is built in `mounts/.tmp-<graft-id>/` and renamed
  to `mounts/<name>/` when complete. The graft's record reserves the name
  first (unique index), so concurrent `graft new` calls with one name
  cannot race.
- **Measurement.** `graft ls` walks the graft and sums unshared bytes (J),
  `.git` included.

## Consequences

- `git status`, `git commit` and `git log` work normally inside a graft.
  Commits land in the graft's own object directory, and Integrate must fetch
  them into Root.
- `refs/graft/<graft-id>` refs appear in Root (`git for-each-ref`) for the
  lifetime of each graft. `git push --mirror` would push them.
- Creating a graft costs a stat of every tracked file in Root plus one clone
  per file. It is proportional to the number of files, not their size. The
  virtual filesystem phase makes creation independent of both.
- Grafts created while Root's working tree is being edited fall back to the
  object store for any file whose stat changes, and stay correct.
