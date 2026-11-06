# 0011: Parallel layer cloning
Status: accepted

## Context

The Phase 8 profile (`bench/results/profiles/`, run 42180527426) shows the
first graft of a Root losing to a worktree on XFS because of one step:
reflinking the freshly built `node_modules` (10,000 files) into the graft
takes 1.13 s, against 0.43 s for the same clone from the cache later.
FICLONE writes back a file's dirty data before sharing its blocks, and
`clone_tree` did that one file at a time on one thread. The working-tree
checkout already clones on up to 8 threads (`materialize` in graft-root).

## Options considered

- A. Avoid the clone for the first graft: keep the built tree in the graft
  and fill the cache later. The cache would then copy a tree the agent may
  already be editing, so the layer could hold the agent's private writes.
  Rejected: it breaks layer isolation.
- B. Flush the build's output (`syncfs`) before cloning. The same writeback
  happens, just in one place; no evidence it is cheaper, and it syncs
  unrelated data.
- C. Clone files on several threads, as the checkout does, so writebacks
  and reflinks overlap. No new dependency (`std::thread::scope`).

## Decision

C. `clone_tree` walks the source once, creating directories and symlinks
and listing files; clones the files on up to 8 threads (one per 64 files,
at least one); then applies directory permissions deepest first.

**Invariant preserved:** the cloned tree is identical to the source:
same paths, contents, file modes, symlinks and directory modes, including
read-only directories inside read-only directories. Every file is still
created with `create_new`/`O_NOFOLLOW` or a reflink to a path that did not
exist, and the first error still fails the whole clone.

**Tests:** `clone::tests::clones_many_files_in_parallel_exactly` (2,000
files, symlinks, an executable, nested read-only directories, compared
entry by entry), `stops_at_the_first_error_while_cloning_in_parallel`,
and the existing clone, layer e2e and conformance tests.

## Consequences

- Layer clones use up to 8 threads during `graft new`. There is no
  fallback to make: one worker is the old behaviour, and is what small
  layers get.
- Measured with `scripts/profile-graft-new.sh` before and after (PR).
