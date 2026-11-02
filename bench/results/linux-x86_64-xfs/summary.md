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
| load average at start | 3.60, 1.52, 0.58 |
| command | `/home/runner/work/graft/graft/target/release/graft-bench run --graft /home/runner/work/graft/graft/target/release/graft --work /mnt/graft-bench/mnt/xfs/bench --out /home/runner/work/graft/graft/bench/results/linux-x86_64-xfs --machine GitHub Actions ubuntu-latest runner --runs 3 --agents 1,4,8,16 --backends reflink,fuse` |

Each cell creates N workspaces one after another from the same fixture.
**create** is the workspace command alone. **ready** (first agent, and median of the later agents) adds `npm ci --offline` (worktree and clone need one per workspace; grafts reuse a `node_modules` layer that the first graft builds) and a passing `npm test`. **disk added** is the drop in free space on the volume with all N workspaces present, including Graft's layer cache and index. Local clones hardlink git objects, as `git clone` does by default. Values are medians over runs, ± the spread ((max − min) / median).

## small (1007 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 373 ms ±12% ⚠ | 504 ms ±11% ⚠ | 504 ms ±11% ⚠ | 864.0 KiB (±13%, below floor) | 864.0 KiB | 45 ms (±9%, below floor) |
| graft | 4 | 81 ms (±8%, below floor) | 492 ms ±9% | 206 ms ±2% | 3.1 MiB ±1% | 781.0 KiB | 51 ms (±32%, below floor) |
| graft | 8 | 71 ms (±162%, below floor) | 514 ms ±54% ⚠ | 204 ms ±50% ⚠ | 5.9 MiB ±1% | 760.0 KiB | 52 ms (±97%, below floor) |
| graft | 16 | 76 ms (±3%, below floor) | 501 ms ±11% ⚠ | 206 ms ±5% | 11.7 MiB ±0% | 747.2 KiB | 42 ms (±18%, below floor) |
| graft-fuse | 1 | 393 ms ±28% ⚠ | 532 ms ±21% ⚠ | 532 ms ±21% ⚠ | 576.0 KiB (±6%, below floor) | 576.0 KiB | 11 ms (±487%, below floor) |
| graft-fuse | 4 | 38 ms (±53%, below floor) | 551 ms ±60% ⚠ | 172 ms ±10% ⚠ | 606.0 KiB (±5%, below floor) | 151.5 KiB | 9 ms (±16%, below floor) |
| graft-fuse | 8 | 24 ms (±452%, below floor) | 557 ms ±60% ⚠ | 170 ms ±31% ⚠ | 644.0 KiB (±5%, below floor) | 80.5 KiB | 8 ms (±3%, below floor) |
| graft-fuse | 16 | 22 ms (±8%, below floor) | 654 ms ±21% ⚠ | 158 ms ±1% | 684.0 KiB (±5%, below floor) | 42.8 KiB | 8 ms (±65%, below floor) |
| worktree | 1 | 72 ms (±3%, below floor) | 488 ms ±8% | 488 ms ±8% | 7.5 MiB ±0% | 7.5 MiB | 30 ms (±8%, below floor) |
| worktree | 4 | 74 ms (±0%, below floor) | 485 ms ±7% | 492 ms ±3% | 30.8 MiB ±0% | 7.7 MiB | 29 ms (±17%, below floor) |
| worktree | 8 | 74 ms (±2%, below floor) | 480 ms ±4% | 492 ms ±4% | 62.0 MiB ±0% | 7.7 MiB | 33 ms (±19%, below floor) |
| worktree | 16 | 75 ms (±2%, below floor) | 487 ms ±11% ⚠ | 496 ms ±5% | 124.3 MiB ±0% | 7.8 MiB | 34 ms (±27%, below floor) |
| clone | 1 | 106 ms ±4% | 524 ms ±4% | 524 ms ±4% | 8.1 MiB ±1% | 8.1 MiB | 44 ms (±11%, below floor) |
| clone | 4 | 107 ms ±3% | 513 ms ±8% | 525 ms ±5% | 33.1 MiB ±0% | 8.3 MiB | 45 ms (±12%, below floor) |
| clone | 8 | 107 ms ±5% | 516 ms ±5% | 529 ms ±4% | 66.6 MiB ±0% | 8.3 MiB | 42 ms (±15%, below floor) |
| clone | 16 | 107 ms ±3% | 518 ms ±2% | 523 ms ±3% | 133.5 MiB ±0% | 8.3 MiB | 47 ms (±11%, below floor) |

## medium (20025 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 4.10 s ±9% | 4.23 s ±9% | 4.23 s ±9% | 61.6 MiB ±0% | 61.6 MiB | 1.12 s ±10% ⚠ |
| graft | 4 | 1.77 s ±11% ⚠ | 4.71 s ±14% ⚠ | 1.88 s ±9% | 114.5 MiB ±0% | 28.6 MiB | 1.19 s ±15% ⚠ |
| graft | 8 | 1.86 s ±4% | 4.35 s ±6% | 1.97 s ±6% | 184.9 MiB ±0% | 23.1 MiB | 1.15 s ±6% |
| graft | 16 | 1.89 s ±9% | 4.39 s ±10% ⚠ | 2.01 s ±7% | 325.9 MiB ±0% | 20.4 MiB | 1.24 s ±12% ⚠ |
| graft-fuse | 1 | 3.10 s ±4% | 3.25 s ±3% | 3.25 s ±3% | 44.3 MiB ±0% | 44.3 MiB | 19 ms (±269%, below floor) |
| graft-fuse | 4 | 86 ms (±69%, below floor) | 3.30 s ±9% | 180 ms ±45% ⚠ | 44.4 MiB ±0% | 11.1 MiB | 10 ms (±20%, below floor) |
| graft-fuse | 8 | 21 ms (±8%, below floor) | 3.22 s ±9% | 166 ms ±7% | 44.4 MiB ±0% | 5.5 MiB | 10 ms (±24%, below floor) |
| graft-fuse | 16 | 22 ms (±87%, below floor) | 3.10 s ±8% | 166 ms ±9% | 44.4 MiB ±0% | 2.8 MiB | 8 ms (±17%, below floor) |
| worktree | 1 | 1.18 s ±1% | 2.49 s ±3% | 2.49 s ±3% | 190.1 MiB ±0% | 190.1 MiB | 1.12 s ±17% ⚠ |
| worktree | 4 | 1.19 s ±1% | 2.45 s ±6% | 2.48 s ±2% | 761.2 MiB ±0% | 190.3 MiB | 1.12 s ±10% |
| worktree | 8 | 1.23 s ±6% | 2.53 s ±7% | 2.49 s ±5% | 1.5 GiB ±0% | 190.3 MiB | 1.15 s ±6% |
| worktree | 16 | 1.30 s ±9% | 2.52 s ±4% | 2.74 s ±4% | 3.0 GiB ±0% | 190.4 MiB | 1.15 s ±10% ⚠ |
| clone | 1 | 1.18 s ±1% | 2.49 s ±5% | 2.49 s ±5% | 190.2 MiB ±0% | 190.2 MiB | 1.15 s ±3% |
| clone | 4 | 1.19 s ±1% | 2.47 s ±7% | 2.51 s ±4% | 761.6 MiB ±0% | 190.4 MiB | 1.14 s ±3% |
| clone | 8 | 1.23 s ±2% | 2.51 s ±7% | 2.59 s ±7% | 1.5 GiB ±0% | 190.4 MiB | 1.19 s ±5% |
| clone | 16 | 1.31 s ±15% ⚠ | 2.52 s ±7% | 2.70 s ±13% ⚠ | 3.0 GiB ±0% | 190.5 MiB | 1.14 s ±6% |

## large (200007 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 11.13 s ±6% | 11.26 s ±6% | 11.26 s ±6% | 123.6 MiB ±0% | 123.6 MiB | 7.12 s ±4% |
| graft | 4 | 10.89 s ±4% | 11.38 s ±7% | 10.97 s ±4% | 494.0 MiB ±0% | 123.5 MiB | 7.12 s ±6% |
| graft | 8 | 10.43 s ±2% | 11.20 s ±6% | 10.51 s ±1% | 987.9 MiB ±0% | 123.5 MiB | 7.07 s ±0% |
| graft | 16 | 11.02 s ±3% | 11.07 s ±4% | 11.09 s ±5% | 1.9 GiB ±0% | 123.5 MiB | 7.81 s ±7% |
| graft-fuse | 1 | 18.10 s ±0% | 18.23 s ±0% | 18.23 s ±0% | 592.0 KiB (±3%, below floor) | 592.0 KiB | 11 ms (±6%, below floor) |
| graft-fuse | 4 | 47 ms (±35%, below floor) | 17.85 s ±4% | 173 ms ±4% | 614.0 KiB (±5%, below floor) | 153.5 KiB | 10 ms (±15%, below floor) |
| graft-fuse | 8 | 85 ms (±104%, below floor) | 17.76 s ±7% | 175 ms ±42% ⚠ | 644.0 KiB (±7%, below floor) | 80.5 KiB | 10 ms (±45%, below floor) |
| graft-fuse | 16 | 47 ms (±94%, below floor) | 18.69 s ±6% | 177 ms ±21% ⚠ | 700.0 KiB (±14%, below floor) | 43.8 KiB | 9 ms (±12%, below floor) |
| worktree | 1 | 12.80 s ±3% | 13.26 s ±4% | 13.26 s ±4% | 1.4 GiB ±0% | 1.4 GiB | 6.68 s ±1% |
| worktree | 4 | 14.51 s ±3% | 13.24 s ±3% | 15.62 s ±4% | 5.7 GiB ±0% | 1.4 GiB | 7.02 s ±2% |
| worktree | 8 | 14.79 s ±4% | 13.36 s ±5% | 15.30 s ±5% | 11.3 GiB ±0% | 1.4 GiB | 9.32 s ±6% |
| worktree | 16 | 14.98 s ±2% | 13.45 s ±4% | 15.46 s ±2% | 22.7 GiB ±0% | 1.4 GiB | 9.57 s ±19% ⚠ |
| clone | 1 | 12.58 s ±3% | 13.02 s ±3% | 13.02 s ±3% | 1.4 GiB ±0% | 1.4 GiB | 6.86 s ±5% |
| clone | 4 | 14.97 s ±1% | 13.19 s ±8% | 15.73 s ±1% | 5.7 GiB ±0% | 1.4 GiB | 7.32 s ±3% |
| clone | 8 | 14.94 s ±5% | 13.28 s ±7% | 15.89 s ±5% | 11.3 GiB ±0% | 1.4 GiB | 8.26 s ±44% ⚠ |
| clone | 16 | 14.77 s ±3% | 13.50 s ±9% | 15.38 s ±4% | 22.7 GiB ±0% | 1.4 GiB | 8.49 s ±28% ⚠ |

## Reproducibility

160 of 191 values are within ±10% across 3 runs (31 outside, marked ⚠). 49 values below the noise floor (100 ms, 1.0 MiB) are shown but not counted.
