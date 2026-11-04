# 0008: Delta store and op log
Status: accepted

## Context

Virtual grafts (ADR 0006) are read-only. This phase makes them writable:
every change goes into the graft's private delta and is journaled in the op
log. AGENTS.md requires:

- the delta maps paths to a blob, a tombstone, a symlink or a mode change,
  with blobs content-addressed in `blobs/`;
- whole-file copy-up on first write;
- op log rows with sequence, time, kind, paths, resulting blob and an
  optional actor;
- replaying the op log from empty reproduces the delta exactly;
- `git status` inside a graft matches `graft status`;
- a `kill -9` of `graftd` mid-write loses at most the in-flight operation;
- `graft status` and `graft diff` read the delta instead of scanning the
  tree.

Decisions needed: the delta model, when writes are journaled, the blob
store, how a virtual graft gets a `.git`, how status finds changes, and
what is not supported.

## Options considered

**Delta model**

- A. Only changed files, with renames of directories applied lazily
  ("redirect" entries). Cheap renames, but every lookup must follow
  redirects and replay is harder to reason about.
- B. A map from path to entry, where an entry is a file, symlink, directory
  or tombstone. A file's content is either a delta blob or *the content of
  a lower path* (the base tree or a layer, both immutable for the graft's
  lifetime). Directories carry an `opaque` flag: an opaque directory hides
  whatever the lower layers have at that path. Renaming a lower directory
  copies its entries (not its contents) under the new path.

**When to journal writes**

- C. One op log row per `write(2)` call. Exact, but hashes and commits a
  transaction per 128 KiB chunk.
- D. Copy-up into a *staging file* on first open for writing. Writes go
  straight to that file. The content is hashed into a blob and journaled as
  one `write` op when the file is flushed, fsynced or released. Staging
  files are recorded in the database when created, so a restarted daemon
  finalises them. `kill -9` of `graftd` leaves the page cache intact, so
  everything written before the kill survives.

**Blob store**

- E. Blobs in SQLite. Simple, but large files bloat the database and every
  read copies through SQLite.
- F. Files in `blobs/<aa>/<hash>`, written to `blobs/tmp/` and renamed into
  place, so a blob is either absent or complete. `std::fs::copy` clones on
  APFS (`clonefile`) and on btrfs/XFS (`copy_file_range`), so copying a
  staging file into the store shares blocks where the filesystem can.

**`.git` in a virtual graft**

- G. Synthesize git state on every request. Complex and slow.
- H. A real directory, `grafts/<graft-id>/git/`, created on the first lookup
  of `.git` and served read-write as a pass-through. It is the same minimal
  repository as a reflink graft's (ADR 0003, alternates to Root's object
  store), plus an index built from the base tree whose stat data matches
  what the mount reports: committer time as mtime, blob size as size.
  `core.checkStat = minimal` and `core.trustctime = false`, because inode
  numbers are allocated lazily. Building it costs O(files), but only on
  first use of git, so `graft new` stays O(1).

**Status from the delta**

- I. Run a git status over the mount. Scans the tree.
- J. The delta's paths are the candidates. Each is compared with the base
  (working-tree form, mode). Added paths that the graft's ignore rules
  exclude are hidden, like git's own status.

## Decision

B + D + F + H + J.

- **Journal.** Every mutation runs under the graft's write lock. It
  computes the new delta entries, writes the op log row and the changed
  delta rows in one SQLite transaction, then updates the in-memory delta.
  A crash before commit loses that operation; after commit it is applied.
  Op kinds: `create`, `write`, `truncate`, `mkdir`, `symlink`, `unlink`,
  `rmdir`, `rename`, `chmod`, `touch`. The entry's mtime is the op's
  explicit mtime (`touch`), otherwise its time.
- **Replay.** `Delta::apply(op, lower)` is the only function that changes a
  delta. The live path and replay both use it, and a property test checks
  that replaying the stored op log equals the stored delta. A second
  property test checks the overlay against a plain in-memory filesystem
  model.
- **Durability.** `fsync` of a file fsyncs its blob and commits. Without
  `fsync`, data survives a crash of `graftd` but may not survive power
  loss, as on any local filesystem.
- **Staging** files live in `grafts/<graft-id>/work/`. Unlinking an open
  file tombstones the path; its staged content is dropped on release.
- **Cleanup.** `graft prune` removes `grafts/<graft-id>/` and the graft's
  delta rows. A blob sweep deletes blobs no delta row or staging row
  references; it runs in `graftd` under the write locks, or in the CLI while
  holding `graftd.lock` when no daemon runs. Shared refcounts arrive with
  Bud and Ring.
- **Kernel caching.** Every change arrives through the mount, so the
  kernel's caches stay coherent. Long entry and attribute TTLs are kept;
  `keep_cache` is only set for files that still have their lower content.
- **Not supported**, with errors tools already handle: hard links (`EPERM`;
  cargo and npm fall back to copying), device nodes, FIFOs and sockets via
  `mknod` (`EPERM`), changing ownership to another user (`EPERM`), extended
  attributes (`ENOTSUP`), `RENAME_EXCHANGE` (`EINVAL`). `RENAME_NOREPLACE`
  is honoured. Directory mtimes stay at the base commit time.
- **Prune safety.** A virtual graft has work at risk if its delta has
  changes against the base, or its `.git` has commits Root lacks.
- **Actor.** `graftd` accepts a per-graft actor over IPC (`set_actor`) and
  records it on each op; `graft run` sets it from the sandboxing phase on.

## Consequences

- Schema v4: new tables `delta` and `staged`, and columns `mode`, `mtime`
  and `target` on `op_log`. Format version 5.
- A rename of a large lower directory costs one delta row per descendant,
  but no file contents are copied.
- `graft status` reflects completed operations; a file still open for
  writing appears once flushed or closed.
- The first `git` command in a virtual graft builds its index (O(files)).
  The performance phase may replace this with an fsmonitor-backed index.
- Layers become lower layers too: writes into `node_modules` are copied up
  into the graft's delta and never reach the layer cache.
