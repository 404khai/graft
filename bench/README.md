# Benchmarks

`graft-bench` compares grafts with `git worktree` and full `git clone` on
create time, time to a passing test, disk added and prune time, for 1, 4, 8
and 16 agents on three fixtures. Methodology:
[ADR 0005](../docs/decisions/0005-benchmark-methodology.md).

## Run

```sh
bench/run.sh                                  # the whole matrix, 3 runs
bench/run.sh --fixtures small --agents 1,4 --runs 1   # a quick look
```

`run.sh` builds `graft` and `graft-bench` in release mode and measures on a
dedicated volume:

- **macOS**: creates a sparse APFS disk image (no sudo), capped at the host's
  free space minus 4 GiB, with Spotlight indexing off, and removes it
  afterwards. `--image-size GIB` sets the cap.
- **Linux**: pass `--work DIR` on a dedicated filesystem (CI uses a btrfs loop
  mount). Without it, `$TMPDIR` is used with a warning.

Needs `git`, `node` and `npm`. Nothing touches the network.

Results go to `bench/results/<os>-<arch>-<fs>/`:

| File | Contents |
| --- | --- |
| `results.csv` | One row per run × fixture × method × agents |
| `summary.md` | Medians over runs with spreads, the machine, and the reproducibility verdict |
| `header.md` | The summary's header (machine and method), used by `graft-bench summarize` |

Cells that would not fit on the volume are skipped and listed as such.

## Fixtures

| Name | Tracked files | `node_modules` |
| --- | ---: | ---: |
| small | ≈1k | 100 files |
| medium | ≈20k | 10,000 files |
| large | ≈200k | 100 files |

Fixtures are generated deterministically (blake3-keyed contents, fixed
timestamps), cached in the work directory, and regenerated when the
generator changes.

## In CI

`.github/workflows/bench.yml` runs the matrix on demand (`workflow_dispatch`)
on `ubuntu-latest` (btrfs on the runner's large disk) and `macos-latest`
(APFS), and uploads `bench/results/` as an artifact. It never gates merges.
The quick `graft disk use` job runs on pull requests labelled `bench`.
