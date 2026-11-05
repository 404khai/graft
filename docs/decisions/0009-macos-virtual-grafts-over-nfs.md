# 0009: macOS virtual grafts over NFS loopback
Status: accepted

## Context

Virtual grafts (ADRs 0006 and 0008) use FUSE on Linux. AGENTS.md asks for
virtual grafts on macOS with **no kernel extension and no system extension
approval**, reusing the delta store and op log, and passing the same
conformance suite.

## Options considered

- A. **macFUSE.** A third-party kernel extension (or, in its newer mode, an
  FSKit-based backend that still ships as a system component). Installing it
  requires admin rights and, on Apple silicon, lowering security policy or
  approving an extension. Fails the requirement.
- B. **FSKit** (macOS 15.4+). User-space file systems, but each one is an app
  extension inside a signed app bundle that the user must enable in System
  Settings. That is an approval step, and it needs code signing for every
  build. Fails the requirement today.
- C. **NFSv3 loopback.** `graftd` serves each graft as an NFSv3 export on
  `127.0.0.1`, and `mount_nfs` (built into macOS) mounts it. This is what
  EdenFS does on macOS. A probe on macOS 27 confirmed that an unprivileged
  user can mount and unmount such an export on a directory they own: no
  `sudo`, no extension.

**NFS server:** `nfsserve` 0.11 (BSD-3-Clause, maintained by Hugging Face,
used by hf-mount) implements NFSv3, MOUNT and portmap over TCP behind an
async trait. Writing an NFS server from scratch is not justified.

## Decision

C, with `nfsserve`.

- **One export per graft.** `graftd` binds a listener on `127.0.0.1:0` for
  each mounted graft and runs `mount_nfs -o vers=3,tcp,port=P,mountport=P,
  nolocks,soft,timeo=…` on `mounts/<name>/`. The NFS adapter
  (`graft-fs::nfs`) translates every NFS procedure into a `GraftView` call,
  so reads, the delta, the op log, `.git` and layers behave exactly as with
  FUSE. NFS file IDs are the view's inode numbers.
- **`soft` mounts.** If `graftd` dies, calls fail with an error instead of
  hanging the process forever; a restarted daemon serves a new port, so
  the old mount is detached (`umount -f`) and the graft remounted.
- **Journaling without open/close.** NFSv3 is stateless: there is no open,
  flush or release. The first WRITE (or truncating SETATTR) of a file opens
  a view handle for it, writes go to its staging file as with FUSE, and the
  handle is released, which journals the file, once no write has arrived
  for a short idle period. A staging file is recorded in the database when
  created, so a crash of `graftd` still loses at most the operation in
  flight (ADR 0008 recovery).
- **Kind on disk.** `grafts.backend` gains `nfs` (schema v5). The CLI's
  `--backend` accepts `nfs`; each platform's `graftd` mounts only its own
  virtual kind (`fuse` on Linux, `nfs` on macOS) and refuses the other with
  a clear error. The default backend stays `reflink` until the performance
  phase decides (AGENTS.md Phase 8).
- **Prune.** `umount`, then `umount -f` if the graft is busy, with a
  warning. Unlike Linux's lazy unmount, processes holding files open get
  errors afterwards instead of continuing to read them.

## Consequences

- **Durability gap.** `nfsserve` answers every WRITE with `FILE_SYNC` and
  does not implement COMMIT, so a client's `fsync` never reaches `graftd`.
  Data written to an NFS graft survives a crash of `graftd` (it is in the
  staging file and the page cache) but may not survive power loss before
  it is journaled. Unstable writes with COMMIT, which would make `fsync`
  durable, are scoped in the performance phase.
- **Local access control.** The export is reachable by any process on the
  machine that can connect to `127.0.0.1:<port>`, and `nfsserve` does not
  expose the caller's credentials to the file system (AUTH_UNIX credentials
  are also forgeable locally). On a single-user Mac this matches the trust
  boundary of the user's own files. On a shared Mac another local user who
  finds the port could read or write the graft. Closing this needs a
  transport only the owner can reach, or caller checks inside the server;
  both are left for hardening (not verified feasible yet).
- **Lost attributes.** `nfsserve` drops the attributes of MKDIR, so new
  directories get `0755`. File modes from CREATE and SETATTR are kept.
- **No extended attributes**, as with FUSE grafts. NFSv3 has no xattrs, so
  the macOS client would store them in AppleDouble `._name` files; the
  server refuses to create those, so they never appear as changes.
  `._name` files already in the base tree are still served.
- **No hard links** (`nfsserve` does not implement LINK), as with FUSE
  grafts.
- The NFS client caches attributes (`actimeo`). All changes go through the
  mount, so the cache stays coherent; tuning it is left to the performance
  phase.
- Tests that mount NFS are gated behind the `nfs-tests` feature and
  `GRAFT_TEST_NFS=1`, and run on macOS CI.
