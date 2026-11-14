# Profile: `tsc` in an NFS graft (macOS)

Question: the owner's benchmark report (`graft-vs-worktree-report.md`)
measured `npm run build` (`tsc -p .`) in an NFS graft at 53 s against 7 s
in a reflink graft, and an incremental build at 139 s against 22 s. Where
does the time go?

## Machine and method

- Apple M3 (8 cores), 16 GiB, macOS 27.0, APFS, 47 GiB free. Load average
  6–40 from other projects (once over 100); every comparison below is
  interleaved or back to back, never across sessions.
- Project: the report's `bench-app` (3,009 tracked files; `tsc` emits
  ≈6,000 files into `dist/`), Node v24.16.0. Release builds of graft and
  graftd.
- `scripts/profile-nfs-ops.sh`: restarts graftd with
  `GRAFT_LOG=graft::nfs=debug`, which logs the time graftd spends in each
  NFS procedure (new `graft::nfs` timing target), runs one command in the
  graft, and prints calls, total, mean and p99 per procedure, plus the
  client's RPC counts from `nfsstat -c`.
- `sample graftd` during a build, and small Python loops for per-call
  latency (create + write + close of a 2 KB file; stat of missing names).

## Findings

1. **graftd's own work, not the transport, is the cost.** For a rebuild
   (outputs exist, so each is truncated and rewritten) at load ≈10:
   wall 35.2 s, of which graftd spent 14.0 s journaling idle files,
   11.0 s in SETATTR, 4.5 s in WRITE and < 1.5 s in everything else
   (62k GETATTRs, 13k LOOKUPs, 3.5k READs, 154 READDIRPLUS). `sample`
   showed graftd's threads otherwise idle; `TCP_NODELAY` is already set.
2. **An overwrite was copied up and journaled twice.** The macOS client
   overwrites a file as SETATTR(size 0) then WRITE. With no write handle
   open, SETATTR copied the whole old file into a staging file, truncated
   it, journaled a `truncate` op and retired the staging file; the WRITE
   then copied the (now empty) file up again and was journaled at idle.
   That is two copy-ups, two blob installs and six SQLite write
   transactions per emitted file.
3. **Every new file cost two AppleDouble round trips.** macOS stores the
   `com.apple.provenance` xattr it puts on every new file in a `._name`
   file on NFS servers without xattrs. graftd refused it with
   `NFS3ERR_ACCES` and the client retried once: LOOKUP + CREATE of
   `._name` twice per file (`nfsstat`: 3 CREATEs per new file).
   `mount_nfs` has no option to turn this off (`noappledouble` is
   unknown).
4. **Per-call latency through the client** is 0.5–1.3 ms for a LOOKUP of a
   missing name at load 15–35 (33 µs on APFS), and 3–23 ms for create +
   write + close of a 2 KB file (0.5 ms on APFS), most of it the
   server-side work above.

## Changes (ADR 0023, ADR 0024)

- SETATTR with a size goes through the file's NFS write handle (opened
  empty when truncating to 0), so a truncate and the writes after it are
  one staging file and one journaled op, as `open(O_TRUNC)` + write +
  close are on FUSE.
- Truncating to 0 never copies the old contents up (both backends).
- `._name` CREATEs are refused with `NFS3ERR_NOTSUPP`; the client then
  tries once per file instead of twice. User `setxattr` still fails
  (`EPERM`), as before (ADR 0009: no xattrs).

## Results

Same rebuild, `scripts/profile-nfs-ops.sh`, graftd time per procedure:

| | before (load 10) | after (load 26) | after, open empty on truncate (load 37) |
| --- | ---: | ---: | ---: |
| wall | 35.2 s | 16.5 s | 15.2 s |
| journal (6,004 files) | 14.0 s | 6.3 s | 6.7 s |
| SETATTR (6,004) | 11.0 s | 3.6 s | 2.5 s |
| WRITE (6,004) | 4.5 s | 0.10 s | 0.16 s |

Interleaved, 5 rounds, `npx tsc -p .` after a one-line edit (load 6–24),
ms, median (min–max):

| worktree | reflink graft | NFS graft, `main` | NFS graft, this branch |
| ---: | ---: | ---: | ---: |
| 4860 (3260–32535) | 3196 (2814–9564) | 27393 (26416–40416) | 18792 (13444–94618) |

## Still slow

An NFS graft is still ≈6× a reflink graft for `tsc`. What remains, per
emitted file: one SQLite transaction to record the staging file (SETATTR,
≈0.4 ms under load), journaling at idle (blob clone, hash and rename plus
one transaction, ≈1.1 ms, on the flusher thread but holding the graft's
writer lock), one AppleDouble LOOKUP + CREATE, and ≈10 RPCs in all.
Next candidates, not done here: group commit of idle files in one
transaction (needs a multi-op `Store::record` and the crash tests to
cover a batch), keeping the staging row insert off the SETATTR path, and
answering AppleDouble names without a round trip (no client option found).
The report's 139 s "incremental build, run directly" was not reproduced as
such; under the same load the branch's direct build is 13–19 s.
