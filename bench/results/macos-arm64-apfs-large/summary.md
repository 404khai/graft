# Graft benchmark: grafts vs git worktrees vs full clones

| | |
| --- | --- |
| machine | GitHub Actions macos-latest runner (large: 1 and 4 agents only) |
| OS | macOS 26.7.2 (aarch64) |
| CPU | Apple M1 (Virtual), 3 threads |
| filesystem | apfs, reflinks supported (dedicated volume) |
| graft | graft 0.0.0 |
| git | git version 2.56.0 |
| node / npm | v24.22.2 / 11.19.1 |
| runs | 3 (after one discarded warm-up cell per fixture and method); cells with fewer than 5 agents repeat until they have 5 timed creations |
| load average at start | 5.17 5.17 5.83 |
| command | `/Users/runner/work/graft/graft/target/release/graft-bench run --graft /Users/runner/work/graft/graft/target/release/graft --work /Users/runner/work/_temp/graft-bench/bench --out /Users/runner/work/graft/graft/bench/results/macos-arm64-apfs --machine GitHub Actions macos-latest runner (large: 1 and 4 agents only) --fixtures large --runs 3 --agents 1,4 --backends reflink,nfs` |

Each cell creates N workspaces one after another from the same fixture.
**create** is the workspace command alone. **ready** (first agent, and median of the later agents) adds `npm ci --offline` (worktree and clone need one per workspace; grafts reuse a `node_modules` layer that the first graft builds) and a passing `npm test`. **disk added** is the drop in free space on the volume with all N workspaces present, including Graft's layer cache and index. Local clones hardlink git objects, as `git clone` does by default. Values are medians over runs, ± the spread ((max − min) / median).

## large (200008 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 44.73 s ±43% ⚠ | 45.03 s ±43% ⚠ | 45.03 s ±43% ⚠ | 75.6 MiB ±2% | 75.6 MiB | 16.18 s ±30% ⚠ |
| graft | 4 | 39.63 s ±15% ⚠ | 42.21 s ±15% ⚠ | 38.95 s ±11% ⚠ | 302.4 MiB ±2% | 75.6 MiB | 19.45 s ±10% |
| | | errors: run 3: error: graft prune failed (exit status: 1): graft: removing /Users/runner/work/_temp/graft-bench/bench/cell-large-graft-4/graft-home/roots/9a9119ca97ed14f38716b3ba386138e5/mounts/agent1: directory not empty after 6 attempts; still there: src/d0224/m022499.js, src/d0224/m022489.js, src/d0224/m022498.js, src/d0224/m022488.js, src/d0224/m022450.js, src/d0224/m022401.js, src/d0224/m022497.js, src/d0224/m022435.js, src/d0224/m022464.js, src/d0224/m022474.js, ... | | | | | |
| graft-nfs | 1 | 46.03 s ±49% ⚠ | 46.31 s ±49% ⚠ | 46.31 s ±49% ⚠ | 7.7 MiB ±26% ⚠ | 7.7 MiB | 55 ms (±51%, below floor) |
| graft-nfs | 4 | 49 ms (±46%, below floor) | 38.56 s ±49% ⚠ | 243 ms ±46% ⚠ | 7.8 MiB ±26% ⚠ | 2.0 MiB | 21 ms (±35%, below floor) |
| worktree | 1 | 24.56 s ±22% ⚠ | 25.47 s ±23% ⚠ | 25.47 s ±23% ⚠ | 1.4 GiB ±0% | 1.4 GiB | 12.00 s ±27% ⚠ |
| worktree | 4 | 24.31 s ±44% ⚠ | 26.85 s ±40% ⚠ | 25.29 s ±46% ⚠ | 5.5 GiB ±0% | 1.4 GiB | 12.98 s ±50% ⚠ |
| clone | 1 | 24.84 s ±16% ⚠ | 26.48 s ±18% ⚠ | 26.48 s ±18% ⚠ | 1.4 GiB ±0% | 1.4 GiB | 11.08 s ±13% ⚠ |
| clone | 4 | 32.07 s ±33% ⚠ | 33.26 s ±34% ⚠ | 33.85 s ±37% ⚠ | 5.5 GiB ±1% | 1.4 GiB | 16.25 s ±40% ⚠ |

## Reproducibility

7 of 37 values are within ±10% across 3 runs (30 outside, marked ⚠). 3 values below the noise floor (100 ms, 1.0 MiB) are shown but not counted.
