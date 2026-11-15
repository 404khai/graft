# 0028: Background prune and opt-in disk use
Status: accepted

## Context

The Astrid field report (`reports/graft_report_astrid.md`) used reflink
grafts with a cloned 13 GB `target/`. `graft_list` took 10.4 s and each
`graft_prune` about 12 s. Both were spent walking that tree:

- `ls` measures each graft's unshared bytes (`unshared_bytes`): one
  `FIEMAP` / `getattrlist` call per file, serially.
- `prune` deletes the tree before returning.

Agents call `graft_list` often and want it instant; the disk figure is
rarely what they need. Phase 2 makes `graft ls` show disk use, and
`scripts/measure-graft-disk.sh` and the bench read it.

## Options considered

- A. Cache disk use per graft and refresh it in the background. Needs
  invalidation on every write to a reflink tree, which Graft does not see.
- B. Make disk use opt-in for agents (`graft_list {disk: true}`,
  `graft ls --no-disk`), keep it on by default in `graft ls`, and walk on
  several threads.
- C. For prune: delete on a thread before exiting. The process still
  waits.
- D. For prune: rename the tree to a hidden `.trash-<graft-id>` in the
  mounts directory and delete it from a detached process.

## Decision

B and D.

- `unshared_bytes` walks directories on up to 8 threads (a shared work
  queue; std only). It returns the same total as the serial walk
  (`parallel_and_serial_walks_agree`).
- `graft_list` passes `--no-disk` unless `disk: true`; then `disk_bytes`
  is null with `note: "disk not measured"`.
- `graft prune` runs every safety check first, unchanged. It then renames
  the tree to `mounts/.trash-<id>` (constant time on one filesystem), so
  the name can be reused at once, and starts `graft __empty-trash`
  detached, with no stdio so `graft mcp` does not wait for it.
  `trash.lock` keeps sweepers from racing; `--wait` deletes before
  returning; if the sweeper cannot start, prune deletes inline.
  `graft gc` empties anything an interrupted sweeper left.
- Trash entries are removed with the same check as prune: direct children
  of the canonical mounts directory, never through a symlink.
- The bench times `prune --wait`, so the comparison with `git worktree
  remove` still counts the whole delete.

## Consequences

- `graft_list` and `graft_prune` no longer scale with build-tree size.
- For a moment after prune, the tree's blocks are still allocated under
  `$GRAFT_HOME`; Phase 2's "prune leaves no files behind" holds once the
  sweeper finishes (`prune_leaves_nothing_behind` waits for it, up to 20 s)
  or after `graft gc`.
- Tests: `prune_wait_deletes_before_returning_and_frees_the_name`,
  `gc_finishes_an_interrupted_prune`, `ls_no_disk_skips_measuring`, and the
  MCP cycle's `graft_list` checks.
