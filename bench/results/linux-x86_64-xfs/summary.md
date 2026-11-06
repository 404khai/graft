# Graft benchmark: grafts vs git worktrees vs full clones

| | |
| --- | --- |
| machine | GitHub Actions ubuntu-latest runner |
| OS | Linux 7.0.0-1019-azure (x86_64) |
| CPU | AMD EPYC 9V74 80-Core Processor, 4 threads |
| filesystem | xfs, reflinks supported (dedicated volume) |
| graft | graft 0.0.0 |
| git | git version 2.56.0 |
| node / npm | v24.22.2 / 11.19.1 |
| runs | 3 (after one discarded warm-up cell per fixture and method); cells with fewer than 5 agents repeat until they have 5 timed creations |
| load average at start | 4.03, 1.69, 0.65 |
| command | `/home/runner/work/graft/graft/target/release/graft-bench run --graft /home/runner/work/graft/graft/target/release/graft --work /mnt/graft-bench/mnt/xfs/bench --out /home/runner/work/graft/graft/bench/results/linux-x86_64-xfs --machine GitHub Actions ubuntu-latest runner --runs 3 --agents 1,4,8,16 --backends reflink,fuse` |

Each cell creates N workspaces one after another from the same fixture.
**create** is the workspace command alone. **ready** (first agent, and median of the later agents) adds `npm ci --offline` (worktree and clone need one per workspace; grafts reuse a `node_modules` layer that the first graft builds) and a passing `npm test`. **disk added** is the drop in free space on the volume with all N workspaces present, including Graft's layer cache and index. Local clones hardlink git objects, as `git clone` does by default. Values are medians over runs, ± the spread ((max − min) / median).

## small (1008 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 313 ms ±13% ⚠ | 432 ms ±12% ⚠ | 432 ms ±12% ⚠ | 992.0 KiB (±10%, below floor) | 992.0 KiB | 48 ms (±202%, below floor) |
| graft | 4 | 72 ms (±25%, below floor) | 476 ms ±15% ⚠ | 187 ms ±3% | 3.1 MiB ±0% | 803.0 KiB | 40 ms (±16%, below floor) |
| graft | 8 | 72 ms (±46%, below floor) | 433 ms ±12% ⚠ | 193 ms ±9% | 6.0 MiB ±0% | 772.0 KiB | 45 ms (±21%, below floor) |
| graft | 16 | 123 ms ±47% ⚠ | 488 ms ±17% ⚠ | 250 ms ±29% ⚠ | 11.8 MiB ±0% | 754.2 KiB | 46 ms (±18%, below floor) |
| graft-fuse | 1 | 334 ms ±5% | 463 ms ±7% | 463 ms ±7% | 648.0 KiB (±0%, below floor) | 648.0 KiB | 10 ms (±3%, below floor) |
| graft-fuse | 4 | 26 ms (±18%, below floor) | 451 ms ±3% | 141 ms ±2% | 766.0 KiB (±0%, below floor) | 191.5 KiB | 8 ms (±6%, below floor) |
| graft-fuse | 8 | 19 ms (±10%, below floor) | 451 ms ±6% | 141 ms ±14% ⚠ | 936.0 KiB (±0%, below floor) | 117.0 KiB | 7 ms (±19%, below floor) |
| graft-fuse | 16 | 19 ms (±13%, below floor) | 469 ms ±23% ⚠ | 141 ms ±4% | 1.2 MiB ±0% | 76.8 KiB | 6 ms (±18%, below floor) |
| worktree | 1 | 73 ms (±1%, below floor) | 425 ms ±4% | 425 ms ±4% | 7.6 MiB ±0% | 7.6 MiB | 28 ms (±4%, below floor) |
| worktree | 4 | 74 ms (±2%, below floor) | 430 ms ±10% ⚠ | 424 ms ±3% | 30.9 MiB ±0% | 7.7 MiB | 32 ms (±10%, below floor) |
| worktree | 8 | 74 ms (±2%, below floor) | 414 ms ±4% | 414 ms ±6% | 62.1 MiB ±0% | 7.8 MiB | 32 ms (±26%, below floor) |
| worktree | 16 | 75 ms (±2%, below floor) | 425 ms ±4% | 425 ms ±5% | 124.4 MiB ±0% | 7.8 MiB | 32 ms (±29%, below floor) |
| clone | 1 | 110 ms ±6% | 463 ms ±7% | 463 ms ±7% | 8.1 MiB ±1% | 8.1 MiB | 38 ms (±6%, below floor) |
| clone | 4 | 109 ms ±6% | 478 ms ±6% | 451 ms ±7% | 33.2 MiB ±0% | 8.3 MiB | 45 ms (±12%, below floor) |
| clone | 8 | 109 ms ±6% | 455 ms ±1% | 452 ms ±6% | 66.6 MiB ±0% | 8.3 MiB | 47 ms (±14%, below floor) |
| clone | 16 | 108 ms ±3% | 451 ms ±3% | 452 ms ±4% | 133.4 MiB ±0% | 8.3 MiB | 47 ms (±22%, below floor) |

## medium (20026 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 4.18 s ±7% | 4.30 s ±6% | 4.30 s ±6% | 61.7 MiB ±0% | 61.7 MiB | 1.11 s ±9% |
| graft | 4 | 1.75 s ±5% | 4.12 s ±5% | 1.86 s ±2% | 114.6 MiB ±0% | 28.6 MiB | 1.19 s ±7% |
| graft | 8 | 1.80 s ±6% | 4.21 s ±8% | 1.91 s ±5% | 185.1 MiB ±0% | 23.1 MiB | 1.16 s ±4% |
| graft | 16 | 1.78 s ±1% | 4.46 s ±6% | 1.91 s ±1% | 326.1 MiB ±0% | 20.4 MiB | 1.15 s ±2% |
| graft-fuse | 1 | 2.82 s ±4% | 2.96 s ±4% | 2.96 s ±4% | 44.5 MiB ±0% | 44.5 MiB | 11 ms (±16%, below floor) |
| graft-fuse | 4 | 24 ms (±5%, below floor) | 2.88 s ±6% | 154 ms ±2% | 44.6 MiB ±0% | 11.2 MiB | 8 ms (±14%, below floor) |
| graft-fuse | 8 | 18 ms (±11%, below floor) | 2.84 s ±11% ⚠ | 148 ms ±1% | 44.8 MiB ±0% | 5.6 MiB | 8 ms (±9%, below floor) |
| graft-fuse | 16 | 19 ms (±9%, below floor) | 3.00 s ±6% | 153 ms ±2% | 45.0 MiB ±0% | 2.8 MiB | 6 ms (±1%, below floor) |
| worktree | 1 | 1.19 s ±1% | 2.41 s ±1% | 2.41 s ±1% | 190.2 MiB ±0% | 190.2 MiB | 1.13 s ±5% |
| worktree | 4 | 1.20 s ±2% | 2.49 s ±6% | 2.41 s ±2% | 761.3 MiB ±0% | 190.3 MiB | 1.13 s ±3% |
| worktree | 8 | 1.23 s ±6% | 2.40 s ±10% | 2.61 s ±11% ⚠ | 1.5 GiB ±0% | 190.4 MiB | 1.14 s ±10% |
| worktree | 16 | 1.29 s ±5% | 2.43 s ±4% | 2.57 s ±9% | 3.0 GiB ±0% | 190.4 MiB | 1.12 s ±6% |
| clone | 1 | 1.21 s ±2% | 2.45 s ±4% | 2.45 s ±4% | 190.3 MiB ±0% | 190.3 MiB | 1.16 s ±6% |
| clone | 4 | 1.22 s ±3% | 2.46 s ±3% | 2.46 s ±5% | 761.8 MiB ±0% | 190.4 MiB | 1.14 s ±8% |
| clone | 8 | 1.23 s ±4% | 2.43 s ±2% | 2.55 s ±3% | 1.5 GiB ±0% | 190.5 MiB | 1.14 s ±5% |
| clone | 16 | 1.37 s ±8% | 2.53 s ±3% | 2.79 s ±6% | 3.0 GiB ±0% | 190.5 MiB | 1.17 s ±1% |

## large (200008 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 10.90 s ±1% | 11.02 s ±1% | 11.02 s ±1% | 123.7 MiB ±0% | 123.7 MiB | 7.05 s ±4% |
| graft | 4 | 10.63 s ±1% | 11.01 s ±1% | 10.72 s ±1% | 494.1 MiB ±0% | 123.5 MiB | 6.79 s ±2% |
| graft | 8 | 10.39 s ±2% | 11.07 s ±1% | 10.42 s ±2% | 988.0 MiB ±0% | 123.5 MiB | 6.94 s ±1% |
| graft | 16 | 10.55 s ±1% | 10.81 s ±2% | 10.65 s ±0% | 1.9 GiB ±0% | 123.5 MiB | 6.97 s ±4% |
| graft-fuse | 1 | 17.54 s ±1% | 17.66 s ±1% | 17.66 s ±1% | 640.0 KiB (±1%, below floor) | 640.0 KiB | 11 ms (±6%, below floor) |
| graft-fuse | 4 | 25 ms (±13%, below floor) | 17.10 s ±1% | 143 ms ±7% | 750.0 KiB (±1%, below floor) | 187.5 KiB | 9 ms (±24%, below floor) |
| graft-fuse | 8 | 18 ms (±1%, below floor) | 16.82 s ±2% | 138 ms ±0% | 872.0 KiB (±0%, below floor) | 109.0 KiB | 7 ms (±16%, below floor) |
| graft-fuse | 16 | 18 ms (±14%, below floor) | 16.82 s ±1% | 135 ms ±5% | 1.1 MiB ±0% | 72.8 KiB | 6 ms (±16%, below floor) |
| worktree | 1 | 12.58 s ±2% | 13.17 s ±3% | 13.17 s ±3% | 1.4 GiB ±0% | 1.4 GiB | 6.76 s ±3% |
| worktree | 4 | 15.11 s ±5% | 13.18 s ±0% | 15.86 s ±2% | 5.7 GiB ±0% | 1.4 GiB | 6.89 s ±2% |
| worktree | 8 | 14.64 s ±0% | 13.24 s ±4% | 15.10 s ±2% | 11.3 GiB ±0% | 1.4 GiB | 7.11 s ±3% |
| worktree | 16 | 15.12 s ±3% | 13.13 s ±2% | 15.56 s ±2% | 22.7 GiB ±0% | 1.4 GiB | 7.63 s ±27% ⚠ |
| clone | 1 | 12.65 s ±3% | 13.03 s ±3% | 13.03 s ±3% | 1.4 GiB ±0% | 1.4 GiB | 6.93 s ±3% |
| clone | 4 | 15.19 s ±0% | 13.07 s ±2% | 16.15 s ±5% | 5.7 GiB ±0% | 1.4 GiB | 7.04 s ±1% |
| clone | 8 | 15.07 s ±2% | 13.19 s ±1% | 15.56 s ±3% | 11.3 GiB ±0% | 1.4 GiB | 7.18 s ±4% |
| clone | 16 | 15.06 s ±4% | 13.29 s ±4% | 15.52 s ±3% | 22.7 GiB ±0% | 1.4 GiB | 7.49 s ±17% ⚠ |

## Reproducibility

179 of 194 values are within ±10% across 3 runs (15 outside, marked ⚠). 46 values below the noise floor (100 ms, 1.0 MiB) are shown but not counted.
