# Graft benchmark: grafts vs git worktrees vs full clones

| | |
| --- | --- |
| machine | GitHub Actions ubuntu-latest runner |
| OS | Linux 7.0.0-1017-azure (x86_64) |
| CPU | INTEL(R) XEON(R) PLATINUM 8573C, 4 threads |
| filesystem | xfs, reflinks supported (dedicated volume) |
| graft | graft 0.0.0 |
| git | git version 2.56.0 |
| node / npm | v24.22.1 / 11.19.1 |
| runs | 3 (after one discarded warm-up cell per fixture and method); cells with fewer than 5 agents repeat until they have 5 timed creations |
| load average at start | 3.67, 1.75, 0.69 |
| command | `/home/runner/work/graft/graft/target/release/graft-bench run --graft /home/runner/work/graft/graft/target/release/graft --work /mnt/graft-bench/mnt/xfs/bench --out /home/runner/work/graft/graft/bench/results/linux-x86_64-xfs --machine GitHub Actions ubuntu-latest runner --runs 3 --agents 1,4,8,16 --backends reflink,fuse` |

Each cell creates N workspaces one after another from the same fixture.
**create** is the workspace command alone. **ready** (first agent, and median of the later agents) adds `npm ci --offline` (worktree and clone need one per workspace; grafts reuse a `node_modules` layer that the first graft builds) and a passing `npm test`. **disk added** is the drop in free space on the volume with all N workspaces present, including Graft's layer cache and index. Local clones hardlink git objects, as `git clone` does by default. Values are medians over runs, ± the spread ((max − min) / median).

## small (1008 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 398 ms ±2% | 543 ms ±2% | 543 ms ±2% | 928.0 KiB (±10%, below floor) | 928.0 KiB | 39 ms (±5%, below floor) |
| graft | 4 | 66 ms (±6%, below floor) | 543 ms ±3% | 213 ms ±0% | 3.1 MiB ±1% | 791.0 KiB | 42 ms (±5%, below floor) |
| graft | 8 | 66 ms (±6%, below floor) | 540 ms ±1% | 216 ms ±9% | 6.0 MiB ±0% | 764.0 KiB | 41 ms (±5%, below floor) |
| graft | 16 | 67 ms (±2%, below floor) | 561 ms ±4% | 213 ms ±2% | 11.7 MiB ±0% | 750.2 KiB | 43 ms (±2%, below floor) |
| graft-fuse | 1 | 408 ms ±2% | 563 ms ±3% | 563 ms ±3% | 648.0 KiB (±0%, below floor) | 648.0 KiB | 12 ms (±3%, below floor) |
| graft-fuse | 4 | 28 ms (±18%, below floor) | 576 ms ±6% | 181 ms ±4% | 766.0 KiB (±0%, below floor) | 191.5 KiB | 8 ms (±23%, below floor) |
| graft-fuse | 8 | 20 ms (±8%, below floor) | 580 ms ±2% | 173 ms ±2% | 904.0 KiB (±0%, below floor) | 113.0 KiB | 7 ms (±8%, below floor) |
| graft-fuse | 16 | 20 ms (±3%, below floor) | 574 ms ±16% ⚠ | 171 ms ±3% | 1.2 MiB ±0% | 74.8 KiB | 7 ms (±34%, below floor) |
| worktree | 1 | 71 ms (±1%, below floor) | 530 ms ±3% | 530 ms ±3% | 7.5 MiB ±0% | 7.5 MiB | 33 ms (±12%, below floor) |
| worktree | 4 | 71 ms (±1%, below floor) | 532 ms ±1% | 535 ms ±1% | 30.9 MiB ±0% | 7.7 MiB | 33 ms (±8%, below floor) |
| worktree | 8 | 71 ms (±1%, below floor) | 536 ms ±1% | 532 ms ±3% | 62.1 MiB ±0% | 7.8 MiB | 32 ms (±25%, below floor) |
| worktree | 16 | 71 ms (±1%, below floor) | 526 ms ±3% | 529 ms ±2% | 124.4 MiB ±0% | 7.8 MiB | 34 ms (±17%, below floor) |
| clone | 1 | 93 ms (±0%, below floor) | 557 ms ±3% | 557 ms ±3% | 8.1 MiB ±0% | 8.1 MiB | 37 ms (±6%, below floor) |
| clone | 4 | 93 ms (±1%, below floor) | 561 ms ±4% | 559 ms ±1% | 33.1 MiB ±0% | 8.3 MiB | 41 ms (±6%, below floor) |
| clone | 8 | 93 ms (±1%, below floor) | 550 ms ±2% | 555 ms ±1% | 66.5 MiB ±0% | 8.3 MiB | 41 ms (±9%, below floor) |
| clone | 16 | 93 ms (±1%, below floor) | 550 ms ±2% | 550 ms ±2% | 133.3 MiB ±0% | 8.3 MiB | 44 ms (±7%, below floor) |

## medium (20026 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 3.74 s ±3% | 3.89 s ±3% | 3.89 s ±3% | 61.7 MiB ±0% | 61.7 MiB | 956 ms ±2% |
| graft | 4 | 1.51 s ±3% | 3.85 s ±2% | 1.65 s ±1% | 114.5 MiB ±0% | 28.6 MiB | 969 ms ±0% |
| graft | 8 | 1.55 s ±2% | 3.84 s ±1% | 1.71 s ±3% | 185.0 MiB ±0% | 23.1 MiB | 993 ms ±2% |
| graft | 16 | 1.52 s ±2% | 4.09 s ±9% | 1.68 s ±2% | 326.0 MiB ±0% | 20.4 MiB | 995 ms ±3% |
| graft-fuse | 1 | 2.82 s ±7% | 2.98 s ±6% | 2.98 s ±6% | 44.5 MiB ±0% | 44.5 MiB | 12 ms (±33%, below floor) |
| graft-fuse | 4 | 26 ms (±9%, below floor) | 3.01 s ±4% | 190 ms ±3% | 44.6 MiB ±0% | 11.2 MiB | 9 ms (±7%, below floor) |
| graft-fuse | 8 | 20 ms (±8%, below floor) | 2.96 s ±3% | 185 ms ±2% | 44.8 MiB ±0% | 5.6 MiB | 8 ms (±17%, below floor) |
| graft-fuse | 16 | 20 ms (±7%, below floor) | 2.97 s ±3% | 184 ms ±2% | 45.0 MiB ±0% | 2.8 MiB | 7 ms (±10%, below floor) |
| worktree | 1 | 1.21 s ±1% | 2.64 s ±1% | 2.64 s ±1% | 190.2 MiB ±0% | 190.2 MiB | 1.07 s ±2% |
| worktree | 4 | 1.21 s ±1% | 2.65 s ±1% | 2.66 s ±2% | 761.3 MiB ±0% | 190.3 MiB | 1.08 s ±1% |
| worktree | 8 | 1.26 s ±3% | 2.66 s ±0% | 2.79 s ±5% | 1.5 GiB ±0% | 190.4 MiB | 1.11 s ±3% |
| worktree | 16 | 1.23 s ±2% | 2.74 s ±6% | 2.73 s ±4% | 3.0 GiB ±0% | 190.4 MiB | 1.11 s ±2% |
| clone | 1 | 1.21 s ±1% | 2.65 s ±1% | 2.65 s ±1% | 190.3 MiB ±0% | 190.3 MiB | 1.07 s ±1% |
| clone | 4 | 1.22 s ±1% | 2.65 s ±1% | 2.68 s ±2% | 761.7 MiB ±0% | 190.4 MiB | 1.08 s ±4% |
| clone | 8 | 1.23 s ±0% | 2.63 s ±5% | 2.69 s ±7% | 1.5 GiB ±0% | 190.5 MiB | 1.10 s ±2% |
| clone | 16 | 1.25 s ±4% | 2.64 s ±1% | 2.87 s ±6% | 3.0 GiB ±0% | 190.5 MiB | 1.11 s ±2% |

## large (200008 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 10.29 s ±0% | 10.44 s ±1% | 10.44 s ±1% | 123.6 MiB ±0% | 123.6 MiB | 6.12 s ±3% |
| graft | 4 | 10.18 s ±1% | 10.56 s ±1% | 10.33 s ±0% | 494.1 MiB ±0% | 123.5 MiB | 6.08 s ±1% |
| graft | 8 | 10.19 s ±1% | 11.00 s ±3% | 10.29 s ±2% | 987.9 MiB ±0% | 123.5 MiB | 6.11 s ±1% |
| graft | 16 | 10.24 s ±1% | 10.85 s ±5% | 10.34 s ±1% | 1.9 GiB ±0% | 123.5 MiB | 6.10 s ±2% |
| graft-fuse | 1 | 16.53 s ±2% | 16.69 s ±2% | 16.69 s ±2% | 616.0 KiB (±0%, below floor) | 616.0 KiB | 12 ms (±7%, below floor) |
| graft-fuse | 4 | 27 ms (±9%, below floor) | 16.07 s ±3% | 178 ms ±1% | 750.0 KiB (±0%, below floor) | 187.5 KiB | 9 ms (±3%, below floor) |
| graft-fuse | 8 | 20 ms (±9%, below floor) | 15.96 s ±3% | 172 ms ±2% | 904.0 KiB (±0%, below floor) | 113.0 KiB | 8 ms (±6%, below floor) |
| graft-fuse | 16 | 21 ms (±7%, below floor) | 15.92 s ±3% | 174 ms ±3% | 1.2 MiB ±0% | 74.8 KiB | 7 ms (±8%, below floor) |
| worktree | 1 | 12.76 s ±2% | 13.31 s ±2% | 13.31 s ±2% | 1.4 GiB ±0% | 1.4 GiB | 6.83 s ±3% |
| worktree | 4 | 14.69 s ±1% | 13.50 s ±3% | 15.50 s ±1% | 5.7 GiB ±0% | 1.4 GiB | 6.90 s ±1% |
| worktree | 8 | 14.84 s ±3% | 13.22 s ±4% | 15.41 s ±3% | 11.3 GiB ±0% | 1.4 GiB | 6.95 s ±4% |
| worktree | 16 | 14.92 s ±1% | 13.37 s ±4% | 15.50 s ±1% | 22.7 GiB ±0% | 1.4 GiB | 7.34 s ±1% |
| clone | 1 | 13.15 s ±2% | 13.73 s ±2% | 13.73 s ±2% | 1.4 GiB ±0% | 1.4 GiB | 6.71 s ±1% |
| clone | 4 | 14.98 s ±2% | 13.95 s ±5% | 15.67 s ±1% | 5.7 GiB ±0% | 1.4 GiB | 6.91 s ±1% |
| clone | 8 | 14.97 s ±1% | 13.50 s ±1% | 15.51 s ±1% | 11.3 GiB ±0% | 1.4 GiB | 7.21 s ±4% |
| clone | 16 | 14.98 s ±1% | 13.46 s ±2% | 15.54 s ±1% | 22.7 GiB ±0% | 1.4 GiB | 7.28 s ±2% |

## Reproducibility

188 of 189 values are within ±10% across 3 runs (1 outside, marked ⚠). 51 values below the noise floor (100 ms, 1.0 MiB) are shown but not counted.
