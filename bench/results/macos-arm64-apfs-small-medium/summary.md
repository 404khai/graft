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
| load average at start | 38.30 34.59 17.17 |
| command | `/Users/runner/work/graft/graft/target/release/graft-bench run --graft /Users/runner/work/graft/graft/target/release/graft --work /Users/runner/work/_temp/graft-bench/bench --out /Users/runner/work/graft/graft/bench/results/macos-arm64-apfs --machine GitHub Actions macos-latest runner --fixtures small,medium --runs 3 --agents 1,4,8,16 --backends reflink,nfs` |

Each cell creates N workspaces one after another from the same fixture.
**create** is the workspace command alone. **ready** (first agent, and median of the later agents) adds `npm ci --offline` (worktree and clone need one per workspace; grafts reuse a `node_modules` layer that the first graft builds) and a passing `npm test`. **disk added** is the drop in free space on the volume with all N workspaces present, including Graft's layer cache and index. Local clones hardlink git objects, as `git clone` does by default. Values are medians over runs, ± the spread ((max − min) / median).

## small (1008 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 666 ms ±65% ⚠ | 956 ms ±57% ⚠ | 956 ms ±57% ⚠ | 880.0 KiB (±867%, below floor) | 880.0 KiB | 64 ms (±28%, below floor) |
| graft | 4 | 152 ms ±18% ⚠ | 915 ms ±19% ⚠ | 387 ms ±11% ⚠ | 2.7 MiB ±86% ⚠ | 687.0 KiB | 79 ms (±6%, below floor) |
| graft | 8 | 139 ms ±39% ⚠ | 1.01 s ±19% ⚠ | 373 ms ±37% ⚠ | 3.9 MiB ±25% ⚠ | 498.0 KiB | 84 ms (±18%, below floor) |
| graft | 16 | 152 ms ±5% | 863 ms ±19% ⚠ | 410 ms ±1% | 7.9 MiB ±74% ⚠ | 505.8 KiB | 66 ms (±87%, below floor) |
| graft-nfs | 1 | 713 ms ±79% ⚠ | 1.01 s ±75% ⚠ | 1.01 s ±75% ⚠ | 556.0 KiB (±46%, below floor) | 556.0 KiB | 37 ms (±83%, below floor) |
| graft-nfs | 4 | 64 ms (±25%, below floor) | 989 ms ±51% ⚠ | 339 ms ±44% ⚠ | 714.0 KiB (±514%, below floor) | 178.5 KiB | 30 ms (±42%, below floor) |
| graft-nfs | 8 | 50 ms (±31%, below floor) | 1.04 s ±22% ⚠ | 337 ms ±14% ⚠ | 1.0 MiB ±96% ⚠ | 134.0 KiB | 35 ms (±23%, below floor) |
| graft-nfs | 16 | 54 ms (±22%, below floor) | 1.09 s ±15% ⚠ | 337 ms ±20% ⚠ | 1.1 MiB ±345% ⚠ | 71.2 KiB | 30 ms (±24%, below floor) |
| worktree | 1 | 296 ms ±39% ⚠ | 1.15 s ±30% ⚠ | 1.15 s ±30% ⚠ | 7.6 MiB ±16% ⚠ | 7.6 MiB | 121 ms ±42% ⚠ |
| worktree | 4 | 211 ms ±38% ⚠ | 913 ms ±28% ⚠ | 831 ms ±31% ⚠ | 31.0 MiB ±50% ⚠ | 7.8 MiB | 97 ms (±39%, below floor) |
| worktree | 8 | 232 ms ±39% ⚠ | 931 ms ±26% ⚠ | 818 ms ±33% ⚠ | 61.7 MiB ±27% ⚠ | 7.7 MiB | 77 ms (±37%, below floor) |
| worktree | 16 | 194 ms ±22% ⚠ | 856 ms ±60% ⚠ | 808 ms ±24% ⚠ | 124.1 MiB ±12% ⚠ | 7.8 MiB | 95 ms (±15%, below floor) |
| clone | 1 | 574 ms ±45% ⚠ | 1.27 s ±39% ⚠ | 1.27 s ±39% ⚠ | 7.9 MiB ±2% | 7.9 MiB | 152 ms ±56% ⚠ |
| clone | 4 | 565 ms ±19% ⚠ | 1.28 s ±18% ⚠ | 1.15 s ±12% ⚠ | 39.9 MiB ±20% ⚠ | 10.0 MiB | 128 ms ±19% ⚠ |
| clone | 8 | 545 ms ±34% ⚠ | 934 ms ±29% ⚠ | 1.13 s ±31% ⚠ | 67.8 MiB ±23% ⚠ | 8.5 MiB | 151 ms ±28% ⚠ |
| clone | 16 | 535 ms ±50% ⚠ | 1.12 s ±25% ⚠ | 1.17 s ±42% ⚠ | 144.3 MiB ±12% ⚠ | 9.0 MiB | 162 ms ±17% ⚠ |

## medium (20026 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 5.36 s ±23% ⚠ | 5.60 s ±22% ⚠ | 5.60 s ±22% ⚠ | 52.9 MiB ±1% | 52.9 MiB | 1.51 s ±11% ⚠ |
| graft | 4 | 3.96 s ±21% ⚠ | 5.90 s ±20% ⚠ | 4.19 s ±22% ⚠ | 83.4 MiB ±3% | 20.8 MiB | 1.83 s ±26% ⚠ |
| graft | 8 | 3.54 s ±42% ⚠ | 6.09 s ±26% ⚠ | 3.65 s ±59% ⚠ | 127.0 MiB ±1% | 15.9 MiB | 2.62 s ±11% ⚠ |
| graft | 16 | 3.78 s ±5% | 6.74 s ±37% ⚠ | 4.05 s ±2% | 208.6 MiB ±2% | 13.0 MiB | 2.78 s ±25% ⚠ |
| graft-nfs | 1 | 5.15 s ±38% ⚠ | 5.55 s ±37% ⚠ | 5.55 s ±37% ⚠ | 43.4 MiB ±0% | 43.4 MiB | 28 ms (±31%, below floor) |
| graft-nfs | 4 | 54 ms (±40%, below floor) | 5.56 s ±29% ⚠ | 376 ms ±36% ⚠ | 43.5 MiB ±2% | 10.9 MiB | 34 ms (±32%, below floor) |
| graft-nfs | 8 | 52 ms (±70%, below floor) | 5.92 s ±58% ⚠ | 359 ms ±82% ⚠ | 43.7 MiB ±2% | 5.5 MiB | 32 ms (±54%, below floor) |
| graft-nfs | 16 | 52 ms (±35%, below floor) | 6.00 s ±60% ⚠ | 376 ms ±45% ⚠ | 44.8 MiB ±2% | 2.8 MiB | 30 ms (±39%, below floor) |
| worktree | 1 | 3.44 s ±24% ⚠ | 5.14 s ±27% ⚠ | 5.14 s ±27% ⚠ | 185.7 MiB ±0% | 185.7 MiB | 1.83 s ±47% ⚠ |
| worktree | 4 | 3.30 s ±30% ⚠ | 5.65 s ±25% ⚠ | 5.88 s ±28% ⚠ | 744.4 MiB ±0% | 186.1 MiB | 2.33 s ±17% ⚠ |
| worktree | 8 | 3.24 s ±27% ⚠ | 5.26 s ±29% ⚠ | 5.36 s ±30% ⚠ | 1.5 GiB ±0% | 186.0 MiB | 2.86 s ±14% ⚠ |
| worktree | 16 | 3.18 s ±21% ⚠ | 4.76 s ±43% ⚠ | 5.30 s ±18% ⚠ | 2.9 GiB ±1% | 186.2 MiB | 3.12 s ±3% |
| clone | 1 | 3.39 s ±7% | 5.37 s ±8% | 5.37 s ±8% | 185.8 MiB ±0% | 185.8 MiB | 2.06 s ±19% ⚠ |
| clone | 4 | 3.61 s ±14% ⚠ | 5.85 s ±20% ⚠ | 5.89 s ±11% ⚠ | 744.6 MiB ±1% | 186.1 MiB | 2.32 s ±5% |
| clone | 8 | 3.82 s ±4% | 6.00 s ±6% | 6.35 s ±4% | 1.5 GiB ±0% | 186.0 MiB | 3.00 s ±10% |
| clone | 16 | 3.99 s ±11% ⚠ | 5.56 s ±32% ⚠ | 6.70 s ±11% ⚠ | 2.9 GiB ±0% | 185.9 MiB | 3.12 s ±15% ⚠ |

## Reproducibility

30 of 136 values are within ±10% across 3 runs (106 outside, marked ⚠). 24 values below the noise floor (100 ms, 1.0 MiB) are shown but not counted.
