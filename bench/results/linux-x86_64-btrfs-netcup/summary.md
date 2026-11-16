# Graft benchmark: grafts vs git worktrees vs full clones

| | |
| --- | --- |
| machine | netcup VPS 1000 G12.5 (4 shared vCPU, 8 GiB), Debian 13, btrfs loop volume; host also runs unrelated Docker services |
| OS | Linux 6.12.113+deb13-amd64 (x86_64) |
| CPU | AMD EPYC-Rome Processor, 4 threads |
| filesystem | btrfs, reflinks supported (dedicated volume) |
| graft | graft 0.1.1 |
| git | git version 2.47.3 |
| node / npm | v20.19.2 / 9.2.0 |
| runs | 3 (after one discarded warm-up cell per fixture and method); cells with fewer than 5 agents repeat until they have 5 timed creations |
| load average at start | 1.95, 1.78, 1.43 |
| command | `/mnt/btrfs/graft/target/release/graft-bench run --graft /mnt/btrfs/graft/target/release/graft --work /mnt/bench/bench --out /mnt/btrfs/graft/bench/results/linux-x86_64-btrfs-netcup --machine netcup VPS 1000 G12.5 (4 shared vCPU, 8 GiB), Debian 13, btrfs loop volume; host also runs unrelated Docker services --backends reflink,fuse --agents 1,4,8,16` |

Each cell creates N workspaces one after another from the same fixture.
**create** is the workspace command alone. **ready** (first agent, and median of the later agents) adds `npm ci --offline` (worktree and clone need one per workspace; grafts reuse a `node_modules` layer that the first graft builds) and a passing `npm test`. **disk added** is the drop in free space on the volume with all N workspaces present, including Graft's layer cache and index. Local clones hardlink git objects, as `git clone` does by default. Values are medians over runs, ± the spread ((max − min) / median).

## small (1008 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 847 ms ±4% | 1.69 s ±4% | 1.69 s ±4% | 88.0 KiB (±0%, below floor) | 88.0 KiB | 63 ms (±3%, below floor) |
| graft | 4 | 88 ms (±15%, below floor) | 1.68 s ±4% | 881 ms ±10% ⚠ | 374.0 KiB (±0%, below floor) | 93.5 KiB | 62 ms (±36%, below floor) |
| graft | 8 | 91 ms (±22%, below floor) | 1.61 s ±19% ⚠ | 852 ms ±23% ⚠ | 740.0 KiB (±0%, below floor) | 92.5 KiB | 60 ms (±12%, below floor) |
| graft | 16 | 83 ms (±4%, below floor) | 1.75 s ±19% ⚠ | 823 ms ±6% | 1.4 MiB ±0% | 90.2 KiB | 56 ms (±10%, below floor) |
| graft-fuse | 1 | 836 ms ±3% | 1.62 s ±3% | 1.62 s ±3% | 72.0 KiB (±0%, below floor) | 72.0 KiB | 22 ms (±4%, below floor) |
| graft-fuse | 4 | 42 ms (±15%, below floor) | 1.65 s ±10% ⚠ | 834 ms ±6% | 156.0 KiB (±0%, below floor) | 39.0 KiB | 18 ms (±6%, below floor) |
| graft-fuse | 8 | 26 ms (±2%, below floor) | 1.75 s ±11% ⚠ | 808 ms ±9% | 268.0 KiB (±0%, below floor) | 33.5 KiB | 15 ms (±10%, below floor) |
| graft-fuse | 16 | 29 ms (±15%, below floor) | 1.68 s ±10% ⚠ | 806 ms ±14% ⚠ | 496.0 KiB (±0%, below floor) | 31.0 KiB | 16 ms (±23%, below floor) |
| worktree | 1 | 163 ms ±6% | 1.62 s ±5% | 1.62 s ±5% | 6.0 MiB ±0% | 6.0 MiB | 54 ms (±12%, below floor) |
| worktree | 4 | 165 ms ±4% | 1.67 s ±14% ⚠ | 1.68 s ±6% | 23.9 MiB ±0% | 6.0 MiB | 56 ms (±8%, below floor) |
| worktree | 8 | 167 ms ±5% | 1.71 s ±13% ⚠ | 1.66 s ±3% | 47.8 MiB ±0% | 6.0 MiB | 54 ms (±2%, below floor) |
| worktree | 16 | 165 ms ±10% ⚠ | 1.60 s ±39% ⚠ | 1.65 s ±12% ⚠ | 95.5 MiB ±0% | 6.0 MiB | 58 ms (±8%, below floor) |
| clone | 1 | 236 ms ±8% | 1.81 s ±9% | 1.81 s ±9% | 6.0 MiB ±0% | 6.0 MiB | 95 ms (±13%, below floor) |
| clone | 4 | 233 ms ±9% | 1.73 s ±5% | 1.80 s ±5% | 24.0 MiB ±0% | 6.0 MiB | 98 ms (±4%, below floor) |
| clone | 8 | 229 ms ±8% | 1.79 s ±9% | 1.74 s ±8% | 48.0 MiB ±0% | 6.0 MiB | 93 ms (±8%, below floor) |
| clone | 16 | 232 ms ±1% | 1.69 s ±6% | 1.73 s ±2% | 95.9 MiB ±0% | 6.0 MiB | 96 ms (±19%, below floor) |

## medium (20026 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 3.92 s ±5% | 4.61 s ±3% | 4.61 s ±3% | 1.7 MiB ±0% | 1.7 MiB | 1.28 s ±2% |
| graft | 4 | 1.62 s ±7% | 4.72 s ±4% | 2.31 s ±6% | 6.8 MiB ±0% | 1.7 MiB | 1.30 s ±1% |
| graft | 8 | 1.42 s ±5% | 4.62 s ±13% ⚠ | 2.17 s ±5% | 13.5 MiB ±0% | 1.7 MiB | 1.34 s ±10% ⚠ |
| graft | 16 | 1.50 s ±12% ⚠ | 5.04 s ±12% ⚠ | 2.31 s ±11% ⚠ | 27.0 MiB ±0% | 1.7 MiB | 1.28 s ±4% |
| graft-fuse | 1 | 4.45 s ±5% | 5.30 s ±4% | 5.30 s ±4% | 100.0 KiB (±0%, below floor) | 100.0 KiB | 23 ms (±24%, below floor) |
| graft-fuse | 4 | 44 ms (±7%, below floor) | 5.15 s ±11% ⚠ | 842 ms ±2% | 188.0 KiB (±0%, below floor) | 47.0 KiB | 19 ms (±13%, below floor) |
| graft-fuse | 8 | 34 ms (±23%, below floor) | 5.31 s ±11% ⚠ | 853 ms ±6% | 300.0 KiB (±0%, below floor) | 37.5 KiB | 15 ms (±33%, below floor) |
| graft-fuse | 16 | 32 ms (±30%, below floor) | 5.23 s ±25% ⚠ | 811 ms ±12% ⚠ | 528.0 KiB (±0%, below floor) | 33.0 KiB | 17 ms (±18%, below floor) |
| worktree | 1 | 2.10 s ±7% | 5.30 s ±9% | 5.30 s ±9% | 119.8 MiB ±0% | 119.8 MiB | 1.62 s ±8% |
| worktree | 4 | 2.08 s ±11% ⚠ | 5.71 s ±12% ⚠ | 5.47 s ±6% | 479.0 MiB ±0% | 119.8 MiB | 1.51 s ±10% ⚠ |
| worktree | 8 | 2.07 s ±2% | 5.47 s ±31% ⚠ | 5.24 s ±4% | 958.1 MiB ±0% | 119.8 MiB | 1.51 s ±5% |
| worktree | 16 | 2.10 s ±1% | 5.27 s ±10% ⚠ | 5.41 s ±4% | 1.9 GiB ±0% | 119.8 MiB | 1.59 s ±1% |
| clone | 1 | 2.15 s ±9% | 5.56 s ±9% | 5.56 s ±9% | 119.8 MiB ±0% | 119.8 MiB | 1.48 s ±5% |
| clone | 4 | 2.08 s ±13% ⚠ | 5.77 s ±16% ⚠ | 5.41 s ±19% ⚠ | 479.1 MiB ±0% | 119.8 MiB | 1.51 s ±22% ⚠ |
| clone | 8 | 2.11 s ±12% ⚠ | 6.32 s ±20% ⚠ | 5.41 s ±12% ⚠ | 958.3 MiB ±0% | 119.8 MiB | 1.37 s ±26% ⚠ |
| clone | 16 | 2.14 s ±11% ⚠ | 6.26 s ±18% ⚠ | 5.46 s ±11% ⚠ | 1.9 GiB ±0% | 119.8 MiB | 1.54 s ±13% ⚠ |

## large (200008 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 11.09 s ±13% ⚠ | 11.98 s ±12% ⚠ | 11.98 s ±12% ⚠ | 16.8 MiB ±0% | 16.8 MiB | 8.63 s ±13% ⚠ |
| graft | 4 | 10.54 s ±11% ⚠ | 11.08 s ±19% ⚠ | 11.85 s ±9% | 67.4 MiB ±0% | 16.9 MiB | 9.29 s ±5% |
| graft | 8 | 11.46 s ±8% | 11.45 s ±19% ⚠ | 12.50 s ±8% | 134.8 MiB ±0% | 16.9 MiB | 9.26 s ±10% |
| graft | 16 | 11.02 s ±4% | 11.82 s ±8% | 11.80 s ±4% | 270.0 MiB ±759% ⚠ | 16.9 MiB | 12.49 s ±11% ⚠ |
| graft-fuse | 1 | 19.10 s ±7% | 19.88 s ±7% | 19.88 s ±7% | 92.0 KiB (±4%, below floor) | 92.0 KiB | 22 ms (±8%, below floor) |
| graft-fuse | 4 | 48 ms (±13%, below floor) | 21.78 s ±7% | 845 ms ±4% | 186.0 KiB (±1%, below floor) | 46.5 KiB | 21 ms (±26%, below floor) |
| graft-fuse | 8 | 34 ms (±8%, below floor) | 22.07 s ±7% | 839 ms ±12% ⚠ | 296.0 KiB (±1%, below floor) | 37.0 KiB | 18 ms (±10%, below floor) |
| graft-fuse | 16 | 36 ms (±11%, below floor) | 20.36 s ±2% | 845 ms ±1% | 524.0 KiB (±2%, below floor) | 32.8 KiB | 18 ms (±45%, below floor) |
| worktree | 1 | 22.12 s ±5% | 23.65 s ±5% | 23.65 s ±5% | 1.2 GiB ±0% | 1.2 GiB | 10.19 s ±11% ⚠ |
| worktree | 4 | 22.12 s ±8% | 23.34 s ±1% | 23.78 s ±8% | 4.6 GiB ±0% | 1.2 GiB | 11.27 s ±11% ⚠ |
| worktree | 8 | 22.17 s ±1% | 23.17 s ±3% | 23.80 s ±1% | 9.3 GiB ±0% | 1.2 GiB | 12.06 s ±3% |
| worktree | 16 | 21.94 s ±5% | 23.96 s ±3% | 23.38 s ±5% | 18.6 GiB ±0% | 1.2 GiB | 13.49 s ±5% |
| clone | 1 | 22.07 s ±8% | 23.56 s ±8% | 23.56 s ±8% | 1.2 GiB ±0% | 1.2 GiB | 9.55 s ±2% |
| clone | 4 | 21.95 s ±4% | 23.80 s ±5% | 23.45 s ±4% | 4.6 GiB ±0% | 1.2 GiB | 10.10 s ±6% |
| clone | 8 | 22.03 s ±3% | 22.47 s ±6% | 23.78 s ±6% | 9.3 GiB ±0% | 1.2 GiB | 11.02 s ±23% ⚠ |
| clone | 16 | 22.31 s ±2% | 23.37 s ±9% | 23.72 s ±1% | 18.6 GiB ±0% | 1.2 GiB | 12.89 s ±9% |

## Reproducibility

137 of 189 values are within ±10% across 3 runs (52 outside, marked ⚠). 51 values below the noise floor (100 ms, 1.0 MiB) are shown but not counted.
