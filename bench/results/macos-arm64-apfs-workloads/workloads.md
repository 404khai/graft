# Graft benchmark: workloads inside grafts vs git worktrees

| | |
| --- | --- |
| machine | GitHub Actions macos-latest runner |
| OS | macOS 26.7.2 (aarch64) |
| CPU | Apple M1 (Virtual), 3 threads |
| filesystem | apfs, reflinks supported (dedicated volume) |
| graft | graft 0.0.0 |
| git | git version 2.56.0 |
| node / npm | v24.22.2 / 11.19.1 |
| runs | 3 (a fresh workspace per method each run); 5 timed samples per workload after one discarded warm-up |
| load average at start | 31.25 38.00 19.97 |
| command | `/Users/runner/work/graft/graft/target/release/graft-bench workloads --graft /Users/runner/work/graft/graft/target/release/graft --work /Users/runner/work/_temp/graft-bench/bench --out /Users/runner/work/graft/graft/bench/results/macos-arm64-apfs-workloads --machine GitHub Actions macos-latest runner --runs 3 --backends reflink,nfs` |

Each run creates one ready workspace per method (setup not timed: grafts build or reuse the `node_modules` layer, worktrees run `npm ci`), then times each workload inside it. Values are medians over runs of each run's median sample, ± the spread ((max − min) / median). Workloads: ADR 0010.

## medium

| workload | graft | graft-nfs | worktree |
| --- | ---: | ---: | ---: |
| build-cold | 1.41 s ±20% ⚠ | 3.51 s ±404% ⚠ | 1.63 s ±43% ⚠ |
| rebuild | 375 ms ±32% ⚠ | 450 ms ±25% ⚠ | 390 ms ±39% ⚠ |
| test | 244 ms ±18% ⚠ | 299 ms ±18% ⚠ | 259 ms ±30% ⚠ |
| git-status | 68 ms (±17%) | 147 ms ±20% ⚠ | 62 ms (±7%) |
| git-diff | 41 ms (±31%) | 96 ms (±32%) | 38 ms (±16%) |
| git-commit | 102 ms ±54% ⚠ | 304 ms ±53% ⚠ | 62 ms (±41%) |
| rg | 234 ms ±53% ⚠ | 264 ms ±48% ⚠ | 198 ms ±21% ⚠ |
| find | 76 ms (±42%) | 171 ms ±77% ⚠ | 67 ms (±23%) |
| ls-R | 323 ms ±34% ⚠ | 488 ms ±39% ⚠ | 317 ms ±14% ⚠ |
| write-10k-small | 1.22 s ±20% ⚠ | error: removing written files: Directory not empty (os error 66) | 1.14 s ±1% |
| write-1gib | 452 ms ±47% ⚠ | 21.66 s ±16% ⚠ | 485 ms ±42% ⚠ |

## Reproducibility

1 of 24 values are within ±10% across 3 runs (others marked ⚠). Values below 100 ms, or from a single run, show the spread in parentheses and are not counted.
