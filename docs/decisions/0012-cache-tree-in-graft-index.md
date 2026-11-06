# 0012: A cache tree in every graft's index
Status: accepted

## Context

Inside a reflink graft, `git status` was slower than in a worktree on the
same machine (Phase 8 baseline, medium fixture: 30 ms against 24 ms on XFS,
42 ms against 36 ms on ext4), and `git diff`/`git commit` slightly so.
Tracing `git status` (`GIT_TRACE2_PERF`) on a 20,000-file repo showed one
consistent difference: the graft's index, written by gix-index, has no
cache tree (git's `TREE` extension), so every `git status` compared the
whole index with HEAD's tree (`unpack_trees`, about 12 ms there), where a
worktree's index lets git skip every unchanged directory (about 0.5 ms).
Stat'ing files and walking directories cost the same in both.

gix-index can write a cache tree it already has, but never builds one,
and an index state without one has no way to set it.

## Options considered

- A. Run `git read-tree HEAD` after creating the graft. Shells out to git
  on the hot path (AGENTS.md §4 forbids it) and rewrites the index.
- B. Leave it to git: the first `git commit` builds one. Agents run
  `git status` long before they commit.
- C. Build the cache tree from the base tree's objects and attach it to the
  index: serialise the index without extensions, append the encoded `TREE`
  extension and the checksum, and decode it again with gix-index.

## Decision

C, for both reflink grafts (`create_worktree`) and virtual grafts (their
index in graftd's git directory).

**Invariant preserved:** the index's entries are unchanged, and its cache
tree describes exactly the base tree: every directory's tree id and the
number of index entries under it. It is valid because both indexes are
built from that tree and only their stat data changes before they are
written. gix writes a cache tree as-is and never invalidates it, so no
code may change an index's entries after this without dropping the
extension; today nothing does. Before use, the cache tree is verified
against the object store (`Tree::verify` with objects) and its root entry
count against the index; the decoded index must still have its entries.

**Tests:** `writes_a_cache_tree_that_git_agrees_with` (graft-root): the
extension is present, `git write-tree` (which trusts it) returns the base
tree, `git status` is clean, `git fsck` passes, and a commit made in the
graft has the same tree as the same change committed in a worktree, with
names whose git order differs from byte order, a submodule, a symlink
and an executable. Every existing graft, e2e and conformance test passes.

## Consequences

- `graft new` reads every tree object of the base once more (the index
  build already read them, so they are cached).
- git keeps the cache tree current from then on, as in any repository.
