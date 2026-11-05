# Graft benchmark: grafts vs git worktrees vs full clones

| | |
| --- | --- |
| machine | GitHub Actions macos-latest runner (large: 1 and 4 agents only) |
| OS | macOS 26.6.2 (aarch64) |
| CPU | Apple M1 (Virtual), 3 threads |
| filesystem | apfs, reflinks supported (dedicated volume) |
| graft | graft 0.0.0 |
| git | git version 2.56.0 |
| node / npm | v24.22.1 / 11.19.1 |
| runs | 3 (after one discarded warm-up cell per fixture and method); cells with fewer than 5 agents repeat until they have 5 timed creations |
| load average at start | 4.99 5.22 5.94 |
| command | `/Users/runner/work/graft/graft/target/release/graft-bench run --graft /Users/runner/work/graft/graft/target/release/graft --work /Users/runner/work/_temp/graft-bench/bench --out /Users/runner/work/graft/graft/bench/results/macos-arm64-apfs --machine GitHub Actions macos-latest runner (large: 1 and 4 agents only) --fixtures large --runs 3 --agents 1,4 --backends reflink,nfs` |

Each cell creates N workspaces one after another from the same fixture.
**create** is the workspace command alone. **ready** (first agent, and median of the later agents) adds `npm ci --offline` (worktree and clone need one per workspace; grafts reuse a `node_modules` layer that the first graft builds) and a passing `npm test`. **disk added** is the drop in free space on the volume with all N workspaces present, including Graft's layer cache and index. Local clones hardlink git objects, as `git clone` does by default. Values are medians over runs, ± the spread ((max − min) / median).

## large (200007 tracked files)

| method | agents | create (median) | ready, first agent | ready, later agents (median) | disk added, all agents | disk per agent | prune (median) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| graft | 1 | 29.87 s ±11% ⚠ | 30.05 s ±11% ⚠ | 30.05 s ±11% ⚠ | 75.2 MiB ±1% | 75.2 MiB | 12.26 s ±12% ⚠ |
| | | errors: run 3: error: graft prune failed (exit status: 1): graft: removing /Users/runner/work/_temp/graft-bench/bench/cell-large-graft-1/graft-home/roots/42eaa9abe7bc89311a07b1da1f72f272/mounts/agent0: Directory not empty (os error 66): Directory not empty (os error 66) | | | | | |
| graft | 4 | not measured: run 1: error: graft prune failed (exit status: 1): graft: removing /Users/runner/work/_temp/graft-bench/bench/cell-large-graft-4/graft-home/roots/42eaa9abe7bc89311a07b1da1f72f272/mounts/agent0: Directory not empty (os error 66): Directory not empty (os error 66); run 2: error: graft prune failed (exit status: 1): graft: removing /Users/runner/work/_temp/graft-bench/bench/cell-large-graft-4/graft-home/roots/42eaa9abe7bc89311a07b1da1f72f272/mounts/agent1: Directory not empty (os error 66): Directory not empty (os error 66); run 3: error: graft prune failed (exit status: 1): graft: removing /Users/runner/work/_temp/graft-bench/bench/cell-large-graft-4/graft-home/roots/42eaa9abe7bc89311a07b1da1f72f272/mounts/agent0: Directory not empty (os error 66): Directory not empty (os error 66) | | | | | |
| graft-nfs | 1 | 59.88 s ±7% | 60.18 s ±7% | 60.18 s ±7% | 7.7 MiB ±39% ⚠ | 7.7 MiB | 50 ms (±27%, below floor) |
| graft-nfs | 4 | 63 ms (±23%, below floor) | 58.17 s ±26% ⚠ | 310 ms ±31% ⚠ | 7.9 MiB ±122% ⚠ | 2.0 MiB | 33 ms (±17%, below floor) |
| worktree | 1 | 26.52 s ±41% ⚠ | 28.09 s ±41% ⚠ | 28.09 s ±41% ⚠ | 1.4 GiB ±0% | 1.4 GiB | 16.96 s ±29% ⚠ |
| worktree | 4 | 32.07 s ±25% ⚠ | 33.68 s ±23% ⚠ | 33.35 s ±28% ⚠ | 5.6 GiB ±0% | 1.4 GiB | 17.82 s ±46% ⚠ |
| clone | 1 | 29.22 s ±40% ⚠ | 30.47 s ±40% ⚠ | 30.47 s ±40% ⚠ | 1.4 GiB ±0% | 1.4 GiB | 13.74 s ±47% ⚠ |
| clone | 4 | 29.06 s ±42% ⚠ | 32.09 s ±35% ⚠ | 31.25 s ±47% ⚠ | 5.6 GiB ±0% | 1.4 GiB | 16.47 s ±37% ⚠ |

## Reproducibility

8 of 32 values are within ±10% across 3 runs (24 outside, marked ⚠). 3 values below the noise floor (100 ms, 1.0 MiB) are shown but not counted.
