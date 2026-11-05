# Graft benchmark: grafts vs git worktrees vs full clones

| | |
| --- | --- |
| machine | GitHub Actions macos-latest runner |
| OS | macOS 26.7.2 (aarch64) |
| CPU | Apple M1 (Virtual), 3 threads |
| filesystem | apfs, reflinks supported (dedicated volume) |
| graft | graft 0.0.0 |
| git | git version 2.56.0 |
| node / npm | v24.22.2 / 11.19.1 |
| runs | 3 (after one discarded warm-up cell per fixture and method); cells with fewer than 5 agents repeat until they have 5 timed creations |
| load average at start | 56.08 47.97 24.13 |
| command | `/Users/runner/work/graft/graft/target/release/graft-bench run --graft /Users/runner/work/graft/graft/target/release/graft --work /Users/runner/work/_temp/graft-bench/bench --out /Users/runner/work/graft/graft/bench/results/macos-arm64-apfs --machine GitHub Actions macos-latest runner --fixtures small,medium --runs 3 --agents 1,4,8,16 --backends reflink,nfs` |

Each cell creates N workspaces one after another from the same fixture.
**create** is the workspace command alone. **ready** (first agent, and median of the later agents) adds `npm ci --offline` (worktree and clone need one per workspace; grafts reuse a `node_modules` layer that the first graft builds) and a passing `npm test`. **disk added** is the drop in free space on the volume with all N workspaces present, including Graft's layer cache and index. Local clones hardlink git objects, as `git clone` does by default. Values are medians over runs, ± the spread ((max − min) / median).

## small (1008 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 825 ms ±44% ⚠ | 1.10 s ±51% ⚠ | 1.10 s ±51% ⚠ | 900.0 KiB (±123%, below floor) | 900.0 KiB | 89 ms (±40%, below floor) |
| graft | 4 | 166 ms ±27% ⚠ | 979 ms ±15% ⚠ | 415 ms ±23% ⚠ | 2.3 MiB ±88% ⚠ | 578.0 KiB | 71 ms (±11%, below floor) |
| graft | 8 | 170 ms ±16% ⚠ | 814 ms ±31% ⚠ | 429 ms ±12% ⚠ | 4.0 MiB ±146% ⚠ | 514.5 KiB | 74 ms (±23%, below floor) |
| graft | 16 | 162 ms ±26% ⚠ | 1.00 s ±12% ⚠ | 416 ms ±24% ⚠ | 7.9 MiB ±11% ⚠ | 506.8 KiB | 72 ms (±47%, below floor) |
| graft-nfs | 1 | 761 ms ±6% | 1.09 s ±15% ⚠ | 1.09 s ±15% ⚠ | 572.0 KiB (±76%, below floor) | 572.0 KiB | 41 ms (±30%, below floor) |
| graft-nfs | 4 | 59 ms (±36%, below floor) | 1.06 s ±30% ⚠ | 352 ms ±42% ⚠ | 1.2 MiB ±434% ⚠ | 298.5 KiB | 31 ms (±55%, below floor) |
| graft-nfs | 8 | 55 ms (±23%, below floor) | 1.38 s ±44% ⚠ | 339 ms ±32% ⚠ | 1.8 MiB ±900% ⚠ | 229.5 KiB | 34 ms (±24%, below floor) |
| graft-nfs | 16 | 57 ms (±27%, below floor) | 1.23 s ±71% ⚠ | 381 ms ±34% ⚠ | 1.4 MiB ±19% ⚠ | 92.2 KiB | 30 ms (±13%, below floor) |
| worktree | 1 | 281 ms ±23% ⚠ | 1.04 s ±8% | 1.04 s ±8% | 7.6 MiB ±18% ⚠ | 7.6 MiB | 118 ms ±29% ⚠ |
| worktree | 4 | 263 ms ±20% ⚠ | 1.02 s ±23% ⚠ | 1.01 s ±19% ⚠ | 30.9 MiB ±56% ⚠ | 7.7 MiB | 100 ms ±10% |
| worktree | 8 | 243 ms ±22% ⚠ | 988 ms ±8% | 952 ms ±16% ⚠ | 61.0 MiB ±13% ⚠ | 7.6 MiB | 99 ms (±16%, below floor) |
| worktree | 16 | 246 ms ±16% ⚠ | 1.12 s ±35% ⚠ | 905 ms ±26% ⚠ | 122.5 MiB ±14% ⚠ | 7.7 MiB | 89 ms (±4%, below floor) |
| clone | 1 | 669 ms ±14% ⚠ | 1.41 s ±6% | 1.41 s ±6% | 8.0 MiB ±1% | 8.0 MiB | 156 ms ±20% ⚠ |
| clone | 4 | 646 ms ±28% ⚠ | 1.38 s ±13% ⚠ | 1.35 s ±33% ⚠ | 39.9 MiB ±28% ⚠ | 10.0 MiB | 171 ms ±16% ⚠ |
| clone | 8 | 639 ms ±5% | 1.34 s ±9% | 1.33 s ±3% | 66.0 MiB ±37% ⚠ | 8.3 MiB | 166 ms ±33% ⚠ |
| clone | 16 | 687 ms ±15% ⚠ | 1.44 s ±9% | 1.41 s ±11% ⚠ | 136.3 MiB ±66% ⚠ | 8.5 MiB | 178 ms ±13% ⚠ |

## medium (20026 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 6.59 s ±36% ⚠ | 6.81 s ±36% ⚠ | 6.81 s ±36% ⚠ | 52.9 MiB ±0% | 52.9 MiB | 1.95 s ±26% ⚠ |
| graft | 4 | 4.38 s ±31% ⚠ | 7.28 s ±14% ⚠ | 4.34 s ±17% ⚠ | 84.2 MiB ±2% | 21.0 MiB | 1.86 s ±23% ⚠ |
| graft | 8 | 4.26 s ±13% ⚠ | 6.03 s ±19% ⚠ | 4.63 s ±11% ⚠ | 125.5 MiB ±5% | 15.7 MiB | 2.50 s ±54% ⚠ |
| graft | 16 | 4.16 s ±7% | 6.58 s ±30% ⚠ | 4.37 s ±5% | 209.8 MiB ±1% | 13.1 MiB | 3.10 s ±13% ⚠ |
| graft-nfs | 1 | 7.16 s ±32% ⚠ | 7.60 s ±32% ⚠ | 7.60 s ±32% ⚠ | 43.4 MiB ±0% | 43.4 MiB | 56 ms (±19%, below floor) |
| graft-nfs | 4 | 61 ms (±36%, below floor) | 7.23 s ±30% ⚠ | 432 ms ±32% ⚠ | 43.5 MiB ±1% | 10.9 MiB | 39 ms (±52%, below floor) |
| graft-nfs | 8 | 67 ms (±44%, below floor) | 6.97 s ±53% ⚠ | 462 ms ±35% ⚠ | 44.7 MiB ±3% | 5.6 MiB | 37 ms (±36%, below floor) |
| graft-nfs | 16 | 50 ms (±36%, below floor) | 5.85 s ±37% ⚠ | 336 ms ±43% ⚠ | 44.4 MiB ±2% | 2.8 MiB | 36 ms (±30%, below floor) |
| worktree | 1 | 4.53 s ±36% ⚠ | 7.22 s ±28% ⚠ | 7.22 s ±28% ⚠ | 186.7 MiB ±0% | 186.7 MiB | 2.88 s ±39% ⚠ |
| worktree | 4 | 3.91 s ±22% ⚠ | 6.46 s ±26% ⚠ | 6.74 s ±18% ⚠ | 744.3 MiB ±0% | 186.1 MiB | 2.37 s ±30% ⚠ |
| worktree | 8 | 4.12 s ±22% ⚠ | 6.79 s ±10% | 6.88 s ±23% ⚠ | 1.5 GiB ±0% | 185.9 MiB | 2.51 s ±39% ⚠ |
| worktree | 16 | 3.23 s ±39% ⚠ | 5.70 s ±33% ⚠ | 5.78 s ±35% ⚠ | 2.9 GiB ±1% | 186.7 MiB | 3.41 s ±38% ⚠ |
| clone | 1 | 4.05 s ±26% ⚠ | 6.75 s ±22% ⚠ | 6.75 s ±22% ⚠ | 186.1 MiB ±0% | 186.1 MiB | 2.33 s ±20% ⚠ |
| clone | 4 | 4.23 s ±15% ⚠ | 7.06 s ±16% ⚠ | 6.95 s ±4% | 745.3 MiB ±0% | 186.3 MiB | 2.51 s ±3% |
| clone | 8 | 4.39 s ±12% ⚠ | 7.12 s ±21% ⚠ | 7.30 s ±12% ⚠ | 1.5 GiB ±0% | 186.0 MiB | 3.13 s ±9% |
| clone | 16 | 4.26 s ±12% ⚠ | 7.31 s ±18% ⚠ | 7.41 s ±8% | 2.9 GiB ±1% | 186.7 MiB | 3.02 s ±16% ⚠ |

## Reproducibility

35 of 138 values are within ±10% across 3 runs (103 outside, marked ⚠). 22 values below the noise floor (100 ms, 1.0 MiB) are shown but not counted.
