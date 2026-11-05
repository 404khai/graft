# Note on these results

NFS backend against the reflink backend on the same machine (Phase 7's
benchmark criterion): Actions run 41999454838 (`feat/macos-nfs-backend`,
8ae14dd, before the Phase 8 harness changes), on a GitHub Actions
`macos-latest` runner (a virtual machine with 3 threads), on the runner's own
APFS disk.

- `macos-arm64-apfs-small-medium/`: small and medium fixtures, 1 to 16 agents.
- `macos-arm64-apfs-large/`: large fixture, 1 and 4 agents only.

Timings on this runner are **not reproducible**: 31 of 135 (small, medium)
and 8 of 32 (large) values are within ±10% across 3 runs; the load average
was above 30 at the start of the small/medium run. Disk numbers mostly are.

On the large fixture, 4 of the 6 reflink (`graft`) runs failed in
`graft prune` with `Directory not empty (os error 66)`: the 1-agent cell has
2 good runs out of 3, and the 4-agent cell none. That failure is being fixed
in `fix/prune-dir-not-empty`.

`gate.md` in each directory was regenerated locally with the Phase 8 gate
(`graft-bench gate <dir> --default graft`); this run predates it.
