# Note on these results

Phase 8 baseline: Actions run 42005356393 (`perf/in-graft-workloads`,
02a9328), on a GitHub Actions `macos-latest` runner (a virtual machine with 3
threads, load average above 30 at the start), medium fixture, reflink
(`graft`) and NFS (`graft-nfs`) backends.

Timings on this runner are **not reproducible**: only 1 of 24 workload values
is within ±10% across 3 runs. Most `graft` rows are therefore "ties" in
`gate.md`: the spread is larger than the difference. Do not read a win or a
loss into them; reproducible numbers need a dedicated macOS machine.

`write-10k-small` on `graft-nfs` errored: the harness's cleanup of the
written files failed with `Directory not empty (os error 66)` on the NFS
mount, so that workload has no value for NFS.

`gate.md` was regenerated locally from `workloads.csv` with
`graft-bench gate <dir> --default graft`.
