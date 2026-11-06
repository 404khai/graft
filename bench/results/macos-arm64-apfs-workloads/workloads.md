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
| load average at start | 30.52 34.51 17.79 |
| command | `/Users/runner/work/graft/graft/target/release/graft-bench workloads --graft /Users/runner/work/graft/graft/target/release/graft --work /Users/runner/work/_temp/graft-bench/bench --out /Users/runner/work/graft/graft/bench/results/macos-arm64-apfs-workloads --machine GitHub Actions macos-latest runner --runs 3 --backends reflink,nfs` |

Each run creates one ready workspace per method (setup not timed: grafts build or reuse the `node_modules` layer, worktrees run `npm ci`), then times each workload inside it. Values are medians over runs of each run's median sample, ± the spread ((max − min) / median). Workloads: ADR 0010.

## medium

| workload | graft | graft-nfs | worktree |
| --- | ---: | ---: | ---: |
| build-cold | 1.29 s ±16% ⚠ | 3.08 s ±509% ⚠ | 1.59 s ±25% ⚠ |
| rebuild | 340 ms ±10% | 375 ms ±14% ⚠ | 324 ms ±36% ⚠ |
| test | 233 ms ±10% ⚠ | 243 ms ±19% ⚠ | 222 ms ±21% ⚠ |
| git-status | 62 ms (±32%) | 143 ms ±70% ⚠ | 63 ms (±28%) |
| git-diff | 41 ms (±57%) | 87 ms (±77%) | 32 ms (±29%) |
| git-commit | 76 ms (±33%) | 250 ms ±140% ⚠ | 58 ms (±15%) |
| rg | 199 ms ±13% ⚠ | 274 ms ±15% ⚠ | 176 ms ±51% ⚠ |
| find | 54 ms (±59%) | 153 ms ±146% ⚠ | 72 ms (±16%) |
| ls-R | 332 ms ±11% ⚠ | 492 ms ±39% ⚠ | 287 ms ±19% ⚠ |
| write-10k-small | 1.04 s ±28% ⚠ | 47.82 s ±11% ⚠ | 852 ms ±37% ⚠ |
| write-1gib | 424 ms ±15% ⚠ | 18.26 s ±4% | 395 ms ±62% ⚠ |

## Reproducibility

2 of 24 values are within ±10% across 3 runs (others marked ⚠). Values below 100 ms, or from a single run, show the spread in parentheses and are not counted.
