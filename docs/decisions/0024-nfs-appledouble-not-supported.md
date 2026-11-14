# 0024: Refuse AppleDouble files over NFS as "not supported"
Status: accepted

## Context

ADR 0009: grafts have no extended attributes, and the NFS server refuses
the AppleDouble `._name` files macOS uses to emulate them on NFSv3, so
they never become changes. macOS sets `com.apple.provenance` on every file
a process creates, so every new file in an NFS graft makes the client try
`._name`. graftd refused with `NFS3ERR_ACCES`, and the client retried:
LOOKUP + CREATE of `._name` twice per file (profiling: 3 CREATEs per file
`tsc` emitted). `mount_nfs` has no option to stop AppleDouble files.

## Options considered

- A. Keep `NFS3ERR_ACCES`.
- B. **Refuse with `NFS3ERR_NOTSUPP`**, which is also what the refusal
  means. Measured: the client then tries once per file.
- C. Accept `._name` files into a private, unjournaled side store so
  xattrs appear to work. Hidden state that is neither in the delta nor in
  git, lost on remount; contradicts ADR 0009's "no xattrs".

## Decision

B.

## Invariants and tests

- No `._name` file is ever created or journaled, and xattr writes still
  fail: `no_appledouble_files_appear` (nfs_backend) passes unchanged.
- Measured with a probe of graftd's LOOKUP/CREATE calls: for one new file,
  two `._name` LOOKUP + CREATE pairs before, one after.

## Consequences

- `setxattr` on a graft file now fails with `EPERM` (from the client's
  mapping of `NFS3ERR_NOTSUPP`) instead of `EACCES`. Both are errors;
  tools that copy xattrs best-effort (cp, git) were already ignoring them.
- One LOOKUP + CREATE per new file remains.
