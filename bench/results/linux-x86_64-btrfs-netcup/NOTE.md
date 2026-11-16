# Note on these results

First btrfs results, and the first Linux results from a machine other than
a GitHub Actions runner. `main` at cd964c4 (graft 0.1.1), so without PRs
#41–#46 (in particular prune still deletes before returning).

- Machine: netcup VPS 1000 G12.5, 4 **shared** AMD EPYC-Rome vCPUs,
  8 GiB, Debian 13, Linux 6.12. The work volume is a 60 GiB btrfs loop
  image on the VPS's disk. The host also runs unrelated Docker services
  (load average 1.4–3 during the run).
- Command: `bench/run.sh --suite all --backends reflink,fuse --agents
  1,4,8,16 --no-gate --work /mnt/bench`, about 4 hours. `gate.md` was
  generated afterwards with `graft-bench gate <dir> --default graft`.

**Reproducibility fails the ±10% criterion**: 137 of 189 matrix values are
within ±10% across 3 runs. The CPU is shared, so read these for their
shape, not to the percent. The large fixture's 16-agent `graft` disk value
(±759%) is one noisy run; its per-agent value (16.9 MiB) matches the other
agent counts.

## What they show

Reflink grafts (the default backend) against worktrees:

- **Disk:** 1.7 MiB per agent against 119.8 MiB (medium), 16.9 MiB against
  1.2 GiB (large). At 16 agents on the large fixture: 270 MiB against
  18.6 GiB.
- **Create and ready, after the first graft:** medium 1.4–1.6 s against
  2.1 s to create and 2.2–2.3 s against 5.2–5.5 s to a passing test;
  large about 11 s against 22 s, and 12 s against 23.5 s.
- **Inside a workspace:** the same as a worktree for every workload
  (build, rebuild, test, git, `rg`, `find`, `ls -R`, writes), as a real
  directory on the same filesystem should be.

The gate still **fails** for `graft`, on:

- `create x1`: the first graft builds the shared layer (847 ms against
  164 ms small, 3.92 s against 2.10 s medium). Ready-to-test for that
  first agent is level with a worktree, which pays for `npm ci` instead.
- `prune` on the small fixture (60–63 ms against 54–56 ms) and `rebuild`
  (911 ms against 857 ms, 1.06×): small, and within this machine's noise
  for other cells.
- `git-diff` (23 ms against 23 ms, 1.01×), below any meaningful
  difference.

FUSE grafts create and prune in 15–50 ms at any repo size and add under
1 MiB, but **lose badly inside the workspace**: `rg` 22×, `ls -R` 5×,
`write-10k-small` 36× (16.4 s against 0.46 s), cold build 3.6×,
`git status` 2.4×. That is the virtual write and read path, the same
cost seen with NFS on macOS (`profiles/nfs-tsc-macos-arm64-apfs.md`), and
the open Phase 8 work for virtual backends.
