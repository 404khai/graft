# Graft benchmark: workloads inside grafts vs git worktrees

| | |
| --- | --- |
| machine | GitHub Actions ubuntu-latest runner |
| OS | Linux 7.0.0-1019-azure (x86_64) |
| CPU | AMD EPYC 9V74 80-Core Processor, 4 threads |
| filesystem | xfs, reflinks supported (dedicated volume) |
| graft | graft 0.0.0 |
| git | git version 2.56.0 |
| node / npm | v24.22.2 / 11.19.1 |
| runs | 3 (a fresh workspace per method each run); 5 timed samples per workload after one discarded warm-up |
| load average at start | 3.66, 1.40, 0.52 |
| command | `/home/runner/work/graft/graft/target/release/graft-bench workloads --graft /home/runner/work/graft/graft/target/release/graft --work /mnt/graft-bench/mnt/xfs/bench --out /home/runner/work/graft/graft/bench/results/linux-x86_64-xfs-workloads --machine GitHub Actions ubuntu-latest runner --runs 3 --backends reflink,fuse` |

Each run creates one ready workspace per method (setup not timed: grafts build or reuse the `node_modules` layer, worktrees run `npm ci`), then times each workload inside it. Values are medians over runs of each run's median sample, ± the spread ((max − min) / median). Workloads: ADR 0010.

## medium

| workload | graft | graft-fuse | worktree |
| --- | ---: | ---: | ---: |
| build-cold | 553 ms ±0% | 4.01 s ±3% | 552 ms ±2% |
| rebuild | 188 ms ±3% | 234 ms ±1% | 186 ms ±0% |
| test | 122 ms ±5% | 129 ms ±1% | 118 ms ±3% |
| git-status | 30 ms (±10%) | 66 ms (±4%) | 24 ms (±3%) |
| git-diff | 14 ms (±15%) | 19 ms (±4%) | 14 ms (±20%) |
| git-commit | 26 ms (±11%) | 48 ms (±30%) | 24 ms (±12%) |
| rg | 53 ms (±3%) | 1.05 s ±1% | 52 ms (±3%) |
| find | 13 ms (±5%) | 83 ms (±0%) | 13 ms (±4%) |
| ls-R | 173 ms ±2% | 1.30 s ±1% | 174 ms ±1% |
| write-10k-small | 225 ms ±19% ⚠ | 8.60 s ±11% ⚠ | 223 ms ±4% |
| write-1gib | 4.01 s ±33% ⚠ | 4.74 s ±7% | 4.09 s ±3% |

## Reproducibility

16 of 19 values are within ±10% across 3 runs (others marked ⚠). Values below 100 ms, or from a single run, show the spread in parentheses and are not counted.
