# Benchmarks

`graft-bench` compares grafts with `git worktree` and full `git clone` on
create time, time to a passing test, disk added and prune time, for 1, 4, 8
and 16 agents on three fixtures (the **create matrix**). It then measures
the **workloads** an agent runs inside a workspace (build, rebuild, test,
`git status`/`diff`/`commit`, `rg`, `find`, `ls -R`, small and large
writes), and a **gate** compares every graft method with worktrees.
Methodology: [ADR 0005](../docs/decisions/0005-benchmark-methodology.md)
(matrix) and [ADR 0010](../docs/decisions/0010-in-graft-workloads-and-worktree-gate.md)
(workloads and gate).

## Run

```sh
bench/run.sh                                  # matrix, workloads and gate, 3 runs
bench/run.sh --fixtures small --agents 1,4 --runs 1 --suite matrix   # a quick look
bench/run.sh --suite workloads --backends reflink,nfs --runs 1 --samples 3
```

`bench/run.sh` exits non-zero when the gate fails: the default backend's
method (`--default`, `graft` today) does not beat worktrees on some value, or
any graft method loses on one. `--no-gate` skips it.

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
| `workloads.csv` | One row per run × fixture × method × workload |
| `workloads.md` | Workload medians over runs, one column per method |
| `workloads-header.md` | Its header |
| `gate.md` | Every graft value against worktrees: beats, ties or loses |

Cells that would not fit on the volume are skipped and listed as such.

## Fixtures

| Name | Tracked files | `node_modules` |
| --- | ---: | ---: |
| small | ≈1k | 100 files |
| medium | ≈20k | 10,000 files |
| large | ≈200k | 100 files |

Each has `npm test` and an incremental `npm run build` (one bundle per
`src/` directory into `dist/`; a rebuild rewrites only what changed).

Fixtures are generated deterministically (blake3-keyed contents, fixed
timestamps), cached in the work directory, and regenerated when the
generator changes.

## In CI

`.github/workflows/bench.yml` runs the matrix on demand (`workflow_dispatch`)
on `ubuntu-latest` (XFS on the runner's large disk) and `macos-latest`
(APFS), and the workloads on XFS, ext4 and APFS. Each job uploads
`bench/results/` as an artifact, then runs the gate, and fails if it fails.
It never gates merges.
The quick `graft disk use` job runs on pull requests labelled `bench`.

## Reading the results today

Grafts currently use the reflink backend: `graft new` still writes a full
working tree, one reflink per file, so its **create time is close to a
worktree's** (slower on small repos, slightly faster on very large ones). What
grafts already save is disk (unchanged files share blocks; see
`graft ls`) and **time to ready for every agent after the first**, which
reuses the `node_modules` layer instead of reinstalling it.

The large wins come in later phases, and this harness is how they will be shown:

| Phase | Change | Target |
| --- | --- | --- |
| Virtual filesystem (5, 7) | `graft new` writes metadata only; files are read lazily | < 1 s regardless of repo size |
| Delta store (6) | A graft stores only what changed | disk ∝ edits, not repo |
| Bud and Ring (9) | Fork a graft by copying its delta index | < 200 ms |
