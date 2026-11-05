# Note on these results

Phase 8 baseline: Actions run 42005356393 (`perf/in-graft-workloads`,
02a9328), on a GitHub Actions `macos-latest` runner (a virtual machine with 3
threads), on the runner's own APFS disk. Large fixture (≈200k files), 1 and 4
agents only (one 16-agent large cell alone took over an hour on this runner).

**Every reflink (`graft`) cell errored**: `graft prune` failed in all 6 runs
with `Directory not empty (os error 66)` while removing the graft's working
tree. The `graft` rows of `summary.md` are therefore "not measured", and
`gate.md` has no `graft (default)` row: the gate leaves unmeasured cells out
rather than counting them as losses. The prune failure did not reproduce on a
local macOS 27 machine (10 of 10 prunes of the large fixture succeeded); it
is being fixed in `fix/prune-dir-not-empty`. Only the `graft-nfs`, worktree
and clone rows are usable.

Timings on this runner are **not reproducible**: 5 of 27 values are within
±10% across 3 runs. Disk numbers mostly are. Treat timings as indicative.

`gate.md` was regenerated locally from `results.csv` with
`graft-bench gate <dir> --default graft`.
