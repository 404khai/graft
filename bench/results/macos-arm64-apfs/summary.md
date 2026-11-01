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
| load average at start | 22.66 25.56 12.39 |
| command | `/Users/runner/work/graft/graft/target/release/graft-bench run --graft /Users/runner/work/graft/graft/target/release/graft --work /Users/runner/work/_temp/graft-bench/bench --out /Users/runner/work/graft/graft/bench/results/macos-arm64-apfs --machine GitHub Actions macos-latest runner --fixtures small,medium --runs 3 --agents 1,4,8,16` |

Each cell creates N workspaces one after another from the same fixture.
**create** is the workspace command alone. **ready** (first agent, and median of the later agents) adds `npm ci --offline` (worktree and clone need one per workspace; grafts reuse a `node_modules` layer that the first graft builds) and a passing `npm test`. **disk added** is the drop in free space on the volume with all N workspaces present, including Graft's layer cache and index. Local clones hardlink git objects, as `git clone` does by default. Values are medians over runs, ± the spread ((max − min) / median).

## small (1007 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 481 ms ±65% ⚠ | 695 ms ±59% ⚠ | 695 ms ±59% ⚠ | 892.0 KiB (±4%, below floor) | 892.0 KiB | 57 ms (±67%, below floor) |
| graft | 4 | 132 ms ±65% ⚠ | 688 ms ±87% ⚠ | 332 ms ±70% ⚠ | 2.2 MiB ±5% | 553.0 KiB | 50 ms (±82%, below floor) |
| graft | 8 | 144 ms ±45% ⚠ | 696 ms ±64% ⚠ | 392 ms ±47% ⚠ | 4.0 MiB ±56% ⚠ | 516.0 KiB | 50 ms (±71%, below floor) |
| graft | 16 | 134 ms ±45% ⚠ | 1.04 s ±34% ⚠ | 422 ms ±54% ⚠ | 10.3 MiB ±55% ⚠ | 658.2 KiB | 74 ms (±44%, below floor) |
| worktree | 1 | 238 ms ±62% ⚠ | 831 ms ±53% ⚠ | 831 ms ±53% ⚠ | 7.6 MiB ±22% ⚠ | 7.6 MiB | 82 ms (±49%, below floor) |
| worktree | 4 | 281 ms ±61% ⚠ | 1.03 s ±74% ⚠ | 1.12 s ±36% ⚠ | 34.5 MiB ±20% ⚠ | 8.6 MiB | 105 ms ±57% ⚠ |
| worktree | 8 | 313 ms ±43% ⚠ | 1.21 s ±32% ⚠ | 1.10 s ±40% ⚠ | 61.7 MiB ±28% ⚠ | 7.7 MiB | 102 ms ±71% ⚠ |
| worktree | 16 | 302 ms ±64% ⚠ | 1.07 s ±108% ⚠ | 1.07 s ±47% ⚠ | 122.3 MiB ±30% ⚠ | 7.6 MiB | 82 ms (±35%, below floor) |
| clone | 1 | 649 ms ±35% ⚠ | 1.41 s ±26% ⚠ | 1.41 s ±26% ⚠ | 7.9 MiB ±8% | 7.9 MiB | 161 ms ±41% ⚠ |
| clone | 4 | 749 ms ±16% ⚠ | 1.55 s ±19% ⚠ | 1.43 s ±18% ⚠ | 32.9 MiB ±24% ⚠ | 8.2 MiB | 183 ms ±28% ⚠ |
| clone | 8 | 522 ms ±58% ⚠ | 1.04 s ±171% ⚠ | 1.16 s ±54% ⚠ | 64.8 MiB ±36% ⚠ | 8.1 MiB | 149 ms ±32% ⚠ |
| clone | 16 | 726 ms ±40% ⚠ | 1.52 s ±33% ⚠ | 1.53 s ±37% ⚠ | 129.4 MiB ±4% | 8.1 MiB | 184 ms ±54% ⚠ |

## medium (20025 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 6.60 s ±58% ⚠ | 6.97 s ±58% ⚠ | 6.97 s ±58% ⚠ | 52.8 MiB ±1% | 52.8 MiB | 1.88 s ±35% ⚠ |
| graft | 4 | 4.83 s ±14% ⚠ | 7.13 s ±42% ⚠ | 4.92 s ±19% ⚠ | 84.9 MiB ±1% | 21.2 MiB | 1.96 s ±41% ⚠ |
| graft | 8 | 3.81 s ±44% ⚠ | 7.09 s ±62% ⚠ | 3.94 s ±44% ⚠ | 125.6 MiB ±4% | 15.7 MiB | 2.79 s ±50% ⚠ |
| graft | 16 | 4.26 s ±24% ⚠ | 8.89 s ±31% ⚠ | 4.60 s ±25% ⚠ | 208.1 MiB ±1% | 13.0 MiB | 2.73 s ±30% ⚠ |
| worktree | 1 | 4.16 s ±49% ⚠ | 6.77 s ±48% ⚠ | 6.77 s ±48% ⚠ | 186.5 MiB ±0% | 186.5 MiB | 2.39 s ±35% ⚠ |
| worktree | 4 | 4.12 s ±32% ⚠ | 6.53 s ±33% ⚠ | 6.81 s ±32% ⚠ | 743.9 MiB ±0% | 186.0 MiB | 2.70 s ±3% |
| worktree | 8 | 4.12 s ±25% ⚠ | 6.56 s ±24% ⚠ | 6.65 s ±33% ⚠ | 1.5 GiB ±0% | 185.9 MiB | 3.05 s ±29% ⚠ |
| worktree | 16 | 4.00 s ±18% ⚠ | 6.29 s ±23% ⚠ | 6.75 s ±20% ⚠ | 2.9 GiB ±0% | 185.9 MiB | 3.17 s ±26% ⚠ |
| clone | 1 | 4.20 s ±28% ⚠ | 6.87 s ±31% ⚠ | 6.87 s ±31% ⚠ | 185.9 MiB ±1% | 185.9 MiB | 2.32 s ±41% ⚠ |
| clone | 4 | 4.26 s ±15% ⚠ | 6.97 s ±33% ⚠ | 6.91 s ±15% ⚠ | 744.9 MiB ±0% | 186.2 MiB | 2.45 s ±13% ⚠ |
| clone | 8 | 4.47 s ±18% ⚠ | 7.34 s ±33% ⚠ | 7.38 s ±14% ⚠ | 1.5 GiB ±0% | 186.1 MiB | 3.20 s ±2% |
| clone | 16 | 4.13 s ±18% ⚠ | 6.72 s ±17% ⚠ | 6.75 s ±15% ⚠ | 2.9 GiB ±0% | 185.9 MiB | 3.22 s ±12% ⚠ |

## Reproducibility

17 of 113 values are within ±10% across 3 runs (96 outside, marked ⚠). 7 values below the noise floor (100 ms, 1.0 MiB) are shown but not counted.
