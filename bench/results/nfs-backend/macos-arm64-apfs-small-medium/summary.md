# Graft benchmark: grafts vs git worktrees vs full clones

| | |
| --- | --- |
| machine | GitHub Actions macos-latest runner |
| OS | macOS 26.6.2 (aarch64) |
| CPU | Apple M1 (Virtual), 3 threads |
| filesystem | apfs, reflinks supported (dedicated volume) |
| graft | graft 0.0.0 |
| git | git version 2.56.0 |
| node / npm | v24.22.1 / 11.19.1 |
| runs | 3 (after one discarded warm-up cell per fixture and method); cells with fewer than 5 agents repeat until they have 5 timed creations |
| load average at start | 33.81 25.58 16.47 |
| command | `/Users/runner/work/graft/graft/target/release/graft-bench run --graft /Users/runner/work/graft/graft/target/release/graft --work /Users/runner/work/_temp/graft-bench/bench --out /Users/runner/work/graft/graft/bench/results/macos-arm64-apfs --machine GitHub Actions macos-latest runner --fixtures small,medium --runs 3 --agents 1,4,8,16 --backends reflink,nfs` |

Each cell creates N workspaces one after another from the same fixture.
**create** is the workspace command alone. **ready** (first agent, and median of the later agents) adds `npm ci --offline` (worktree and clone need one per workspace; grafts reuse a `node_modules` layer that the first graft builds) and a passing `npm test`. **disk added** is the drop in free space on the volume with all N workspaces present, including Graft's layer cache and index. Local clones hardlink git objects, as `git clone` does by default. Values are medians over runs, ± the spread ((max − min) / median).

## small (1007 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 717 ms ±17% ⚠ | 1.02 s ±28% ⚠ | 1.02 s ±28% ⚠ | 900.0 KiB (±11%, below floor) | 900.0 KiB | 85 ms (±22%, below floor) |
| graft | 4 | 182 ms ±39% ⚠ | 921 ms ±38% ⚠ | 496 ms ±33% ⚠ | 2.3 MiB ±2909% ⚠ | 582.0 KiB | 92 ms (±29%, below floor) |
| graft | 8 | 174 ms ±46% ⚠ | 975 ms ±27% ⚠ | 481 ms ±36% ⚠ | 3.9 MiB ±14803% ⚠ | 503.5 KiB | 77 ms (±50%, below floor) |
| graft | 16 | 133 ms ±49% ⚠ | 977 ms ±10% | 343 ms ±44% ⚠ | −5.2 MiB ±1625% ⚠ | −334.8 KiB | 62 ms (±68%, below floor) |
| graft-nfs | 1 | 586 ms ±39% ⚠ | 821 ms ±40% ⚠ | 821 ms ±40% ⚠ | 604.0 KiB (±14%, below floor) | 604.0 KiB | 27 ms (±47%, below floor) |
| graft-nfs | 4 | 58 ms (±28%, below floor) | 944 ms ±46% ⚠ | 309 ms ±55% ⚠ | 734.0 KiB (±8%, below floor) | 183.5 KiB | 28 ms (±37%, below floor) |
| graft-nfs | 8 | 51 ms (±40%, below floor) | 990 ms ±42% ⚠ | 305 ms ±47% ⚠ | 1.8 MiB ±45% ⚠ | 227.5 KiB | 29 ms (±51%, below floor) |
| graft-nfs | 16 | 54 ms (±29%, below floor) | 981 ms ±28% ⚠ | 332 ms ±27% ⚠ | 2.3 MiB ±1069% ⚠ | 150.2 KiB | 27 ms (±47%, below floor) |
| worktree | 1 | 202 ms ±47% ⚠ | 753 ms ±34% ⚠ | 753 ms ±34% ⚠ | 7.6 MiB ±0% | 7.6 MiB | 75 ms (±39%, below floor) |
| worktree | 4 | 217 ms ±34% ⚠ | 827 ms ±36% ⚠ | 794 ms ±32% ⚠ | 30.4 MiB ±260% ⚠ | 7.6 MiB | 86 ms (±39%, below floor) |
| worktree | 8 | 230 ms ±43% ⚠ | 783 ms ±37% ⚠ | 830 ms ±34% ⚠ | 61.7 MiB ±87% ⚠ | 7.7 MiB | 67 ms (±68%, below floor) |
| worktree | 16 | 237 ms ±30% ⚠ | 911 ms ±17% ⚠ | 900 ms ±25% ⚠ | 137.6 MiB ±11% ⚠ | 8.6 MiB | 81 ms (±50%, below floor) |
| clone | 1 | 577 ms ±15% ⚠ | 1.24 s ±16% ⚠ | 1.24 s ±16% ⚠ | 7.9 MiB ±1% | 7.9 MiB | 139 ms ±46% ⚠ |
| clone | 4 | 562 ms ±9% | 1.18 s ±16% ⚠ | 1.13 s ±13% ⚠ | 39.9 MiB ±168% ⚠ | 10.0 MiB | 132 ms ±20% ⚠ |
| clone | 8 | 500 ms ±20% ⚠ | 1.05 s ±24% ⚠ | 1.05 s ±14% ⚠ | 76.4 MiB ±701% ⚠ | 9.6 MiB | 119 ms ±56% ⚠ |
| clone | 16 | 528 ms ±21% ⚠ | 1.10 s ±24% ⚠ | 1.07 s ±23% ⚠ | 144.0 MiB ±12% ⚠ | 9.0 MiB | 150 ms ±36% ⚠ |

## medium (20025 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 5.53 s ±13% ⚠ | 5.82 s ±12% ⚠ | 5.82 s ±12% ⚠ | 52.9 MiB ±1% | 52.9 MiB | 1.58 s ±25% ⚠ |
| graft | 4 | 3.95 s ±5% | 5.56 s ±7% | 4.17 s ±4% | 84.2 MiB ±1% | 21.1 MiB | 1.96 s ±3% |
| graft | 8 | 3.99 s ±12% ⚠ | 6.94 s ±38% ⚠ | 4.30 s ±12% ⚠ | 125.7 MiB ±0% | 15.7 MiB | 2.25 s ±51% ⚠ |
| graft | 16 | 3.74 s ±14% ⚠ | 7.92 s ±20% ⚠ | 3.99 s ±12% ⚠ | 209.4 MiB ±1% | 13.1 MiB | 2.72 s ±32% ⚠ |
| graft-nfs | 1 | 6.40 s ±26% ⚠ | 6.83 s ±26% ⚠ | 6.83 s ±26% ⚠ | 43.4 MiB ±0% | 43.4 MiB | 42 ms (±34%, below floor) |
| graft-nfs | 4 | 59 ms (±22%, below floor) | 5.30 s ±10% | 389 ms ±10% | 44.0 MiB ±0% | 11.0 MiB | 31 ms (±8%, below floor) |
| graft-nfs | 8 | 51 ms (±11%, below floor) | 5.54 s ±8% | 362 ms ±10% | 43.7 MiB ±1% | 5.5 MiB | 32 ms (±44%, below floor) |
| graft-nfs | 16 | 50 ms (±18%, below floor) | 5.25 s ±24% ⚠ | 343 ms ±18% ⚠ | 44.9 MiB ±1% | 2.8 MiB | 26 ms (±43%, below floor) |
| worktree | 1 | 3.35 s ±21% ⚠ | 5.35 s ±23% ⚠ | 5.35 s ±23% ⚠ | 185.8 MiB ±0% | 185.8 MiB | 1.94 s ±23% ⚠ |
| worktree | 4 | 3.30 s ±22% ⚠ | 4.96 s ±26% ⚠ | 6.23 s ±21% ⚠ | 744.5 MiB ±0% | 186.1 MiB | 2.12 s ±28% ⚠ |
| worktree | 8 | 4.00 s ±35% ⚠ | 6.55 s ±43% ⚠ | 6.89 s ±32% ⚠ | 1.5 GiB ±0% | 185.9 MiB | 2.99 s ±39% ⚠ |
| worktree | 16 | 3.99 s ±34% ⚠ | 6.88 s ±22% ⚠ | 6.72 s ±33% ⚠ | 2.9 GiB ±0% | 186.8 MiB | 3.18 s ±29% ⚠ |
| clone | 1 | 4.11 s ±27% ⚠ | 6.54 s ±27% ⚠ | 6.54 s ±27% ⚠ | 185.9 MiB ±0% | 185.9 MiB | 2.27 s ±29% ⚠ |
| clone | 4 | 3.71 s ±13% ⚠ | 6.04 s ±19% ⚠ | 6.27 s ±8% | 744.8 MiB ±0% | 186.2 MiB | 2.31 s ±9% |
| clone | 8 | 3.86 s ±22% ⚠ | 7.80 s ±20% ⚠ | 6.16 s ±20% ⚠ | 1.5 GiB ±0% | 186.0 MiB | 2.78 s ±47% ⚠ |
| clone | 16 | 3.93 s ±19% ⚠ | 7.49 s ±12% ⚠ | 6.71 s ±24% ⚠ | 2.9 GiB ±1% | 186.5 MiB | 2.85 s ±9% |

## Reproducibility

31 of 135 values are within ±10% across 3 runs (104 outside, marked ⚠). 25 values below the noise floor (100 ms, 1.0 MiB) are shown but not counted.
