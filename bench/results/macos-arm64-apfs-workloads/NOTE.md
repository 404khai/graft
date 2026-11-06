# Note on these results

Phase 8 baseline: Actions run 42113580319 (`main`, af576f2, with the
prune retry fix; replaces run 42005356393), on a GitHub Actions `macos-latest` runner (a virtual machine with 3
threads, load average above 30 at the start), medium fixture, reflink
(`graft`) and NFS (`graft-nfs`) backends.

Timings on this runner are **not reproducible**: only 2 of 24 workload values
are within ±10% across 3 runs. Most `graft` rows are therefore "ties" in
`gate.md`: the spread is larger than the difference. Do not read a win or a
loss into them; reproducible numbers need a dedicated macOS machine.

`write-10k-small` on `graft-nfs` errored in the previous run (42005356393)
during cleanup (`Directory not empty`); in this run it completed (47.8 s).

`gate.md` was regenerated locally from `workloads.csv` with
`graft-bench gate <dir> --default graft`.
