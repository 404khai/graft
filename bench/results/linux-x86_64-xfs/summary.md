# Graft benchmark: grafts vs git worktrees vs full clones

| | |
| --- | --- |
| machine | GitHub Actions ubuntu-latest runner |
| OS | Linux 7.0.0-1017-azure (x86_64) |
| CPU | AMD EPYC 9V74 80-Core Processor, 4 threads |
| filesystem | xfs, reflinks supported (dedicated volume) |
| graft | graft 0.0.0 |
| git | git version 2.56.0 |
| node / npm | v24.22.1 / 11.19.1 |
| runs | 3 (after one discarded warm-up cell per fixture and method); cells with fewer than 5 agents repeat until they have 5 timed creations |
| load average at start | 2.46, 0.67, 0.23 |
| command | `/home/runner/work/graft/graft/target/release/graft-bench run --graft /home/runner/work/graft/graft/target/release/graft --work /mnt/graft-bench/mnt/xfs/bench --out /home/runner/work/graft/graft/bench/results/linux-x86_64-xfs --machine GitHub Actions ubuntu-latest runner --runs 3 --agents 1,4,8,16` |

Each cell creates N workspaces one after another from the same fixture.
**create** is the workspace command alone. **ready** (first agent, and median of the later agents) adds `npm ci --offline` (worktree and clone need one per workspace; grafts reuse a `node_modules` layer that the first graft builds) and a passing `npm test`. **disk added** is the drop in free space on the volume with all N workspaces present, including Graft's layer cache and index. Local clones hardlink git objects, as `git clone` does by default. Values are medians over runs, ± the spread ((max − min) / median).

## small (1007 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 442 ms ±2% | 599 ms ±1% | 599 ms ±1% | 832.0 KiB (±8%, below floor) | 832.0 KiB | 45 ms (±8%, below floor) |
| graft | 4 | 79 ms (±15%, below floor) | 591 ms ±3% | 234 ms ±6% | 3.0 MiB ±1% | 767.0 KiB | 45 ms (±33%, below floor) |
| graft | 8 | 79 ms (±15%, below floor) | 593 ms ±2% | 240 ms ±3% | 5.9 MiB ±0% | 752.0 KiB | 43 ms (±22%, below floor) |
| graft | 16 | 80 ms (±2%, below floor) | 599 ms ±1% | 238 ms ±2% | 11.7 MiB ±0% | 746.2 KiB | 47 ms (±18%, below floor) |
| worktree | 1 | 91 ms (±1%, below floor) | 607 ms ±2% | 607 ms ±2% | 7.5 MiB ±1% | 7.5 MiB | 34 ms (±10%, below floor) |
| worktree | 4 | 94 ms (±1%, below floor) | 625 ms ±8% | 609 ms ±2% | 30.8 MiB ±0% | 7.7 MiB | 37 ms (±12%, below floor) |
| worktree | 8 | 94 ms (±1%, below floor) | 604 ms ±1% | 604 ms ±0% | 61.9 MiB ±0% | 7.7 MiB | 37 ms (±16%, below floor) |
| worktree | 16 | 94 ms (±0%, below floor) | 666 ms ±12% ⚠ | 603 ms ±1% | 124.1 MiB ±0% | 7.8 MiB | 40 ms (±10%, below floor) |
| clone | 1 | 130 ms ±1% | 640 ms ±1% | 640 ms ±1% | 8.0 MiB ±0% | 8.0 MiB | 47 ms (±13%, below floor) |
| clone | 4 | 131 ms ±1% | 642 ms ±8% | 642 ms ±0% | 33.1 MiB ±0% | 8.3 MiB | 49 ms (±11%, below floor) |
| clone | 8 | 132 ms ±3% | 641 ms ±3% | 646 ms ±3% | 66.5 MiB ±0% | 8.3 MiB | 52 ms (±4%, below floor) |
| clone | 16 | 133 ms ±2% | 655 ms ±5% | 643 ms ±1% | 133.5 MiB ±0% | 8.3 MiB | 52 ms (±5%, below floor) |

## medium (20025 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 5.00 s ±4% | 5.17 s ±4% | 5.17 s ±4% | 61.5 MiB ±0% | 61.5 MiB | 1.23 s ±4% |
| graft | 4 | 2.12 s ±2% | 5.10 s ±2% | 2.28 s ±0% | 114.4 MiB ±0% | 28.6 MiB | 1.21 s ±4% |
| graft | 8 | 2.11 s ±0% | 5.08 s ±4% | 2.26 s ±0% | 184.9 MiB ±0% | 23.1 MiB | 1.22 s ±3% |
| graft | 16 | 2.12 s ±0% | 5.11 s ±5% | 2.28 s ±0% | 325.9 MiB ±0% | 20.4 MiB | 1.24 s ±1% |
| worktree | 1 | 1.49 s ±2% | 3.10 s ±3% | 3.10 s ±3% | 190.0 MiB ±0% | 190.0 MiB | 1.31 s ±2% |
| worktree | 4 | 1.51 s ±3% | 3.14 s ±4% | 3.14 s ±3% | 761.1 MiB ±0% | 190.3 MiB | 1.35 s ±5% |
| worktree | 8 | 1.54 s ±4% | 3.20 s ±4% | 3.45 s ±9% | 1.5 GiB ±0% | 190.3 MiB | 1.35 s ±3% |
| worktree | 16 | 1.61 s ±4% | 3.19 s ±2% | 3.42 s ±4% | 3.0 GiB ±0% | 190.4 MiB | 1.35 s ±2% |
| clone | 1 | 1.50 s ±2% | 3.10 s ±5% | 3.10 s ±5% | 190.1 MiB ±0% | 190.1 MiB | 1.29 s ±2% |
| clone | 4 | 1.53 s ±2% | 3.15 s ±2% | 3.26 s ±3% | 761.6 MiB ±0% | 190.4 MiB | 1.34 s ±5% |
| clone | 8 | 1.59 s ±4% | 3.16 s ±3% | 3.31 s ±4% | 1.5 GiB ±0% | 190.4 MiB | 1.32 s ±3% |
| clone | 16 | 1.70 s ±2% | 3.17 s ±2% | 3.47 s ±4% | 3.0 GiB ±0% | 190.5 MiB | 1.34 s ±3% |

## large (200007 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 13.49 s ±3% | 13.65 s ±3% | 13.65 s ±3% | 123.5 MiB ±0% | 123.5 MiB | 7.58 s ±4% |
| graft | 4 | 13.23 s ±0% | 13.66 s ±2% | 13.30 s ±1% | 494.0 MiB ±0% | 123.5 MiB | 7.50 s ±2% |
| graft | 8 | 12.91 s ±0% | 13.56 s ±2% | 13.04 s ±0% | 987.9 MiB ±0% | 123.5 MiB | 7.67 s ±2% |
| graft | 16 | 13.15 s ±1% | 13.46 s ±0% | 13.29 s ±1% | 1.9 GiB ±0% | 123.5 MiB | 7.72 s ±8% |
| worktree | 1 | 16.29 s ±1% | 16.86 s ±1% | 16.86 s ±1% | 1.4 GiB ±0% | 1.4 GiB | 7.94 s ±1% |
| worktree | 4 | 18.36 s ±2% | 16.81 s ±2% | 19.22 s ±1% | 5.7 GiB ±0% | 1.4 GiB | 8.25 s ±2% |
| worktree | 8 | 18.56 s ±1% | 16.96 s ±2% | 19.21 s ±2% | 11.3 GiB ±0% | 1.4 GiB | 8.65 s ±2% |
| worktree | 16 | 18.87 s ±2% | 16.86 s ±3% | 19.46 s ±3% | 22.7 GiB ±0% | 1.4 GiB | 8.47 s ±4% |
| clone | 1 | 16.58 s ±1% | 17.14 s ±0% | 17.14 s ±0% | 1.4 GiB ±0% | 1.4 GiB | 7.95 s ±1% |
| clone | 4 | 18.26 s ±2% | 17.08 s ±0% | 19.28 s ±1% | 5.7 GiB ±0% | 1.4 GiB | 8.21 s ±2% |
| clone | 8 | 18.63 s ±1% | 17.01 s ±2% | 19.45 s ±2% | 11.3 GiB ±0% | 1.4 GiB | 8.21 s ±1% |
| clone | 16 | 18.72 s ±1% | 16.88 s ±1% | 19.31 s ±2% | 22.7 GiB ±0% | 1.4 GiB | 8.30 s ±1% |

## Reproducibility

159 of 160 values are within ±10% across 3 runs (1 outside, marked ⚠). 20 values below the noise floor (100 ms, 1.0 MiB) are shown but not counted.
