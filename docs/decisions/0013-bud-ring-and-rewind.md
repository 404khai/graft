# 0013: Bud, Ring and rewind
Status: accepted

## Context

AGENTS.md Phase 9 asks for:

- `graft bud <src> <dst>`: a new graft sharing the source's base and a copy
  of its delta index, blobs shared rather than copied, in < 200 ms for a
  graft with 1,000 changed files (*target*);
- `graft ring <name> [<label>]`: a named checkpoint, "op log sequence number
  + delta index snapshot";
- `graft rewind <name> <ring>`: restore a graft to a ring, where rewinding
  and then re-applying produces an identical delta;
- blob refcounting across grafts and rings: pruning a graft never removes
  blobs its buds or rings use.

Virtual grafts (FUSE, NFS) have exactly what this needs: a delta of rows in
`graft.db`, blobs in `blobs/`, and an op log (ADR 0008). Reflink grafts, the
default backend, have none of it: their changes live in a real working tree
(ADR 0003) and they keep no op log. So the design has two halves.

## Options considered

**Bud of a virtual graft**

- A. Copy the delta rows only. Constant in repo size, but the bud's op log
  would be empty, so replaying it would not reproduce its delta (the Phase 6
  property).
- B. Copy the delta rows, the op log rows and the layer references in one
  SQLite transaction, under a new graft record with the same base. Replay of
  the bud's op log reproduces its delta, because it is the source's log over
  the same lower tree. Cost: one `INSERT … SELECT` per table, proportional
  to the source's changes and ops, independent of the repo.

**Ring storage**

- C. Only an op log sequence number; rewind replays the log up to it.
  Rewind costs O(ops) and the blobs of the checkpointed state are protected
  only if the sweep also scans the op log.
- D. Sequence number plus a copy of the delta rows (`ring_delta`). Rewind
  costs O(changed paths); the sweep marks blobs from `delta` and
  `ring_delta`, so a ring keeps its blobs alive by construction.

**Rewind and the op log**

- E. Truncate the op log to the ring's sequence number. Simple, but the log
  stops being append-only and the work after the ring is gone.
- F. Append a `rewind` op naming the ring's sequence number, and set the
  delta to the ring's snapshot in the same transaction. Replay treats
  `rewind to n` as "the state after op n", so replaying the whole log still
  reproduces the delta. Before rewinding, Graft rings the current state
  automatically (`undo-<seq>`), so a rewind never loses work and can itself
  be undone ("re-applied") with `graft rewind <name> undo-<seq>`.

**Refcounting**

- G. Per-blob counters updated on every write, bud, ring and prune. Fast to
  query, but every path that changes a delta must keep them exact forever.
- H. Mark and sweep, as today (ADR 0008): the sweep keeps every blob that
  any `delta`, `ring_delta` or staging row references. Bud and ring add
  rows, so they add references; prune deletes rows, so it drops them. No
  counter can drift.

**Reflink grafts** (needs the owner, see below)

- R1. Bud clones the source's working tree (and `.git`) with reflinks; ring
  copies the files that differ from the base into `blobs/` (cloned, so
  blocks are shared) and records them as delta rows; rewind writes the
  working tree back to base + ring. Works, but bud costs a clone of every
  file (≈1.5 s on the medium fixture today, more on large), so the 200 ms
  target cannot be met, and there is no op log for the "sequence number"
  half of a ring.
- R2. Bud, ring and rewind refuse reflink grafts with a clear error naming
  the virtual backends, until the owner decides.
- R3. Journal reflink grafts (watch the working tree and record an op log).
  A large change to the reflink backend, outside this phase.

## Decision

For virtual grafts: B + D + F + H.

- **Bud** journals the source's pending writes (graftd `sync`), then in one
  transaction creates the bud's record (same base, same backend, `created`),
  copies the source's `delta`, `op_log` and `layer_refs` rows, and records
  `budded_from`. It pins the base in Root (`refs/graft/<id>`) like `graft
  new` and asks graftd to mount it. Blobs are shared, never copied. The bud
  gets a fresh `.git` at its base on first use: commits in the source's
  `.git` are not carried over, but its working tree is identical (changes
  the source committed show as uncommitted changes in the bud). Files the
  source still has open for writing and has not journaled are not part of
  the bud.
- **Ring** journals pending writes, then in one transaction records
  `(graft, label, seq = last op's seq, time)` and copies the delta into
  `ring_delta`. Labels follow graft name rules and are unique per graft; the
  default label is `ring-<seq>`.
- **Rewind** journals pending writes, unmounts the graft (graftd), refuses
  if any file is still staged, rings the current state as `undo-<seq>` (if
  that ring does not exist), then in one transaction appends a `rewind` op
  (`to_seq` = the ring's seq) and replaces the delta with the ring's
  snapshot, and remounts. Unmounting drops the kernel's caches and graftd's
  in-memory view, so no reader sees the old delta after the rewind.
- **Replay** handles `rewind to n` by recomputing the state after op `n`
  from the same log. The property test gains rings and rewinds, and checks
  both that replay equals the stored delta and that rewinding to the undo
  ring reproduces the delta from before the rewind.
- **Prune** deletes the graft's rings with its record (cascade). Its blobs
  stay while a bud's delta, any ring, or another graft references them.
  Rings belong to one graft and die with it; a bud does not inherit its
  source's rings.

For reflink grafts: R2, chosen with the owner (2026-11-07). R1 would make
bud as slow as `graft new` and rewind would rewrite a live working tree,
for a feature whose point is being cheap. R3 cannot give the op log's
guarantees: a working tree can only be watched (FSEvents, inotify), which
drops and coalesces events and misses writes before a crash, so replay
would no longer reproduce the delta; virtual grafts already provide that
journal. Bud, ring and rewind are features of virtual grafts. This is
revisited when the deferred performance work chooses each platform's
default backend: if the default stays reflink, with measurements then.

## Consequences

- Schema v6 (format version 7): `grafts.budded_from`, `op_log.to_seq`, new
  tables `rings` and `ring_delta`; migration from v5 adds them, existing
  data is unchanged.
- Bud and ring cost O(changed paths + ops) of SQLite work plus a mount;
  nothing scales with the repo.
- Every ring holds its blobs until it, or its graft, is removed. There is
  no `graft ring --delete` yet; `graft prune` and later `graft gc` policy
  are the ways to release them.
- `graft bud`, `graft ring` and `graft rewind` fail on reflink grafts with an
  error naming the virtual backend.
