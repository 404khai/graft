# 0001: Detect reflink support by probing, not by filesystem name
Status: accepted

## Context

`graft doctor` must report whether a Root's filesystem supports reflinks, and
the reflink backend will need the same answer to decide between cloning and
copying. Support is not a property of the filesystem type alone:

- XFS supports reflinks only when created with `reflink=1` (default since
  xfsprogs 5.1, but older filesystems exist).
- ZFS supports `FICLONE` only with OpenZFS 2.2+ block cloning enabled.
- Reflinks never cross filesystems, so `$GRAFT_HOME` and the Root must share
  one.
- Containers often put the repo on overlayfs, whose behaviour depends on the
  lower filesystem and kernel.

## Options considered

1. **Lookup table by filesystem type** (`statfs` magic on Linux,
   `f_fstypename` on macOS). Cheap and side-effect free, but wrong for XFS
   without `reflink=1`, ZFS and overlayfs.
2. **Feature query APIs.** macOS has `VOL_CAP_INT_CLONE` via `getattrlist`;
   Linux has no equivalent, so this only helps on one platform.
3. **Probe**: create a scratch file next to the target, try `FICLONE` /
   `clonefile(2)`, verify the clone, delete it. One code path per platform,
   and it tests exactly the operation the backend will use.

## Decision

Probe (option 3). Report the filesystem type as well, for diagnosis, and keep
a small table of types with a fixed answer (`FsType::reflink_expected`) so
tests can check the probe against known filesystems.

`ENOTSUP`/`EOPNOTSUPP`, `EINVAL`, `ENOTTY`, `EXDEV` and `ENOSYS` mean "not
supported"; any other error is reported as an error, not as "unsupported".

Doctor probes inside `.git` when it is a directory, so the scratch files are
never visible to `git status`, and removes them afterwards.

## Consequences

- Doctor needs write access to the repository; on a read-only checkout it
  reports the probe error instead of guessing.
- The answer is correct for unusual configurations (XFS without reflink,
  ZFS block cloning) without special cases.
- A brief scratch directory (`.graft-reflink-probe-*`) appears in the probe
  directory while doctor runs.
