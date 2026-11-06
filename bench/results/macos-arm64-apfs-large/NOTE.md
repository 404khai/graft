# Note on these results

Phase 8 baseline: Actions run 42113580319 (`main`, af576f2, with the
prune retry fix; replaces run 42005356393), on a GitHub Actions `macos-latest` runner (a virtual machine with 3
threads), on the runner's own APFS disk. Large fixture (≈200k files), 1 and 4
agents only (one 16-agent large cell alone took over an hour on this runner).

1 of the 6 reflink (`graft`) runs still failed in `graft prune`, now with
the retry fix's error: `directory not empty after 6 attempts; still there:
src/d0224/m022499.js, ...`. Six fresh passes leaving real source files
suggests something recreates or holds them, not a listing that missed
entries; under investigation. The other 5 runs give the `graft` rows.

Timings on this runner are **not reproducible**: 7 of 37 values are within
±10% across 3 runs. Disk numbers mostly are. Treat timings as indicative.

`gate.md` was regenerated locally from `results.csv` with
`graft-bench gate <dir> --default graft`.
