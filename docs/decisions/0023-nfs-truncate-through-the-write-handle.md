# 0023: NFS truncates share the write handle; no copy-up on truncate to 0
Status: accepted

## Context

Phase 8 profiling of `tsc` in an NFS graft
(`bench/results/profiles/nfs-tsc-macos-arm64-apfs.md`). The macOS NFS
client overwrites a file as SETATTR(size = 0) followed by WRITEs. NFSv3
has no open or close, so the NFS adapter (ADR 0009) opens a write handle
on the first WRITE and journals the file once it has been idle for 500
ms. SETATTR did not use that handle: `GraftView::setattr` copied the file
up, truncated it, journaled a `truncate` op and retired the staging file,
and the WRITE then copied up and journaled again. Each overwrite cost two
copy-ups, two blob installs and six SQLite write transactions; SETATTR
plus WRITE took 15.5 s of a 35 s rebuild.

## Options considered

- A. Keep SETATTR journaling at once. Simple, and the truncate is durable
  as its own op; costs as above.
- B. **Open (or reuse) the NFS write handle for a SETATTR that changes
  the size**, so the view truncates the open staging file and leaves
  journaling to the idle release, as for writes. When the new size is 0,
  open the handle empty (`OpenMode { truncate: true }`), so the old
  contents are never copied.
- C. Detect "SETATTR(0) followed by WRITE" heuristically and drop the
  first op. Fragile, and still copies up.

Independently, `GraftView::truncate` to size 0 copied the old contents up
before discarding them, on every backend.

## Decision

B, plus: truncating to 0 opens the staging file empty on every backend.

## Invariants and tests

- Readers see a truncate and the writes after it immediately, through
  the staging file, as before (reads, GETATTR and READDIR consult staged
  files). `an_overwrite_is_journaled_once` (nfs_backend) reads the new
  contents before journaling, then checks there is exactly one op for the
  path, a write whose blob is the final contents. It fails without the
  change (two ops).
- Crash consistency is unchanged in kind: the staging file is recorded in
  the database before the truncate is applied, so a `kill -9` of graftd
  before the idle journal recovers the file's latest contents on restart
  (ADR 0008 recovery). `a_crash_before_an_overwrite_is_journaled_keeps_it`
  (e2e, NFS and FUSE) checks this; the existing crash and conformance
  suites pass unchanged.
- The op log differs only in shape: an overwrite over NFS is now one
  `write` op of kind `truncate` carrying the final blob, the same as an
  `open(O_TRUNC)` + write + close on FUSE. Replaying the op log still
  reproduces the delta (`delta_props`, unchanged).
- A SETATTR that changes only the mode or mtime is unchanged (mtime still
  journals pending writes first).

## Consequences

- A truncate that is not followed by writes is journaled at idle, not at
  once: up to ≈0.5–0.75 s later, like any NFS write today.
- Measured: SETATTR 11.0 s → 2.5 s and WRITE 4.5 s → 0.16 s for 6,004
  emitted files; see the profile note.
