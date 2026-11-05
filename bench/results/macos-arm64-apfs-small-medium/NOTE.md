# Note on these results

Phase 8 baseline: Actions run 42005356393 (`perf/in-graft-workloads`,
02a9328), on a GitHub Actions `macos-latest` runner (a virtual machine with 3
threads), on the runner's own APFS disk. Small and medium fixtures, 1 to 16
agents, reflink (`graft`) and NFS (`graft-nfs`) backends. The large fixture
is in `../macos-arm64-apfs-large/`.

Timings on this runner are **not reproducible**: 35 of 138 values are within
±10% across 3 runs, and the load average at the start was above 30. Disk
numbers mostly are. Treat timings as indicative only; the Linux results in
`../linux-x86_64-xfs/` are the reference (188 of 189 within ±10%).

`gate.md` was regenerated locally from `results.csv` with
`graft-bench gate <dir> --default graft` (the gate output is not part of the
uploaded artifact).
