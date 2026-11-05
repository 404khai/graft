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
| load average at start | 5.54 5.64 6.22 |
| command | `/Users/runner/work/graft/graft/target/release/graft-bench run --graft /Users/runner/work/graft/graft/target/release/graft --work /Users/runner/work/_temp/graft-bench/bench --out /Users/runner/work/graft/graft/bench/results/macos-arm64-apfs --machine GitHub Actions macos-latest runner (large: 1 and 4 agents only) --fixtures large --runs 3 --agents 1,4 --backends reflink,nfs` |

Each cell creates N workspaces one after another from the same fixture.
**create** is the workspace command alone. **ready** (first agent, and median of the later agents) adds `npm ci --offline` (worktree and clone need one per workspace; grafts reuse a `node_modules` layer that the first graft builds) and a passing `npm test`. **disk added** is the drop in free space on the volume with all N workspaces present, including Graft's layer cache and index. Local clones hardlink git objects, as `git clone` does by default. Values are medians over runs, ± the spread ((max − min) / median).

## large (200008 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | not measured: run 1: error: graft prune failed (exit status: 1): graft: removing /Users/runner/work/_temp/graft-bench/bench/cell-large-graft-1/graft-home/roots/0ac1046d04628eb0f384d7cbc6e67da1/mounts/agent0: Directory not empty (os error 66): Directory not empty (os error 66); run 2: error: graft prune failed (exit status: 1): graft: removing /Users/runner/work/_temp/graft-bench/bench/cell-large-graft-1/graft-home/roots/0ac1046d04628eb0f384d7cbc6e67da1/mounts/agent0: Directory not empty (os error 66): Directory not empty (os error 66); run 3: error: graft prune failed (exit status: 1): graft: removing /Users/runner/work/_temp/graft-bench/bench/cell-large-graft-1/graft-home/roots/0ac1046d04628eb0f384d7cbc6e67da1/mounts/agent0: Directory not empty (os error 66): Directory not empty (os error 66) | | | | | |
| graft | 4 | not measured: run 1: error: graft prune failed (exit status: 1): graft: removing /Users/runner/work/_temp/graft-bench/bench/cell-large-graft-4/graft-home/roots/0ac1046d04628eb0f384d7cbc6e67da1/mounts/agent0: Directory not empty (os error 66): Directory not empty (os error 66); run 2: error: graft prune failed (exit status: 1): graft: removing /Users/runner/work/_temp/graft-bench/bench/cell-large-graft-4/graft-home/roots/0ac1046d04628eb0f384d7cbc6e67da1/mounts/agent1: Directory not empty (os error 66): Directory not empty (os error 66); run 3: error: graft prune failed (exit status: 1): graft: removing /Users/runner/work/_temp/graft-bench/bench/cell-large-graft-4/graft-home/roots/0ac1046d04628eb0f384d7cbc6e67da1/mounts/agent1: Directory not empty (os error 66): Directory not empty (os error 66) | | | | | |
| graft-nfs | 1 | 70.44 s ±11% ⚠ | 70.97 s ±11% ⚠ | 70.97 s ±11% ⚠ | 9.2 MiB ±22% ⚠ | 9.2 MiB | 59 ms (±9%, below floor) |
| graft-nfs | 4 | 57 ms (±9%, below floor) | 63.61 s ±10% ⚠ | 336 ms ±3% | 8.3 MiB ±23% ⚠ | 2.1 MiB | 34 ms (±17%, below floor) |
| worktree | 1 | 38.68 s ±28% ⚠ | 40.37 s ±28% ⚠ | 40.37 s ±28% ⚠ | 1.4 GiB ±0% | 1.4 GiB | 18.02 s ±23% ⚠ |
| worktree | 4 | 38.10 s ±38% ⚠ | 40.07 s ±36% ⚠ | 39.72 s ±37% ⚠ | 5.5 GiB ±0% | 1.4 GiB | 17.36 s ±56% ⚠ |
| clone | 1 | 36.23 s ±34% ⚠ | 37.91 s ±34% ⚠ | 37.91 s ±34% ⚠ | 1.4 GiB ±0% | 1.4 GiB | 17.10 s ±47% ⚠ |
| clone | 4 | 37.74 s ±43% ⚠ | 47.21 s ±35% ⚠ | 38.49 s ±43% ⚠ | 5.6 GiB ±0% | 1.4 GiB | 16.53 s ±66% ⚠ |

## Reproducibility

5 of 27 values are within ±10% across 3 runs (22 outside, marked ⚠). 3 values below the noise floor (100 ms, 1.0 MiB) are shown but not counted.
