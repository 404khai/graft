# Graft benchmark: workloads inside grafts vs git worktrees

| | |
| --- | --- |
| machine | GitHub Actions ubuntu-latest runner |
| OS | Linux 7.0.0-1019-azure (x86_64) |
| CPU | AMD EPYC 7763 64-Core Processor, 4 threads |
| filesystem | xfs, reflinks supported (dedicated volume) |
| graft | graft 0.0.0 |
| git | git version 2.56.0 |
| node / npm | v24.22.2 / 11.19.1 |
| runs | 3 (a fresh workspace per method each run); 5 timed samples per workload after one discarded warm-up |
| load average at start | 4.00, 2.23, 0.95 |
| command | `/home/runner/work/graft/graft/target/release/graft-bench workloads --graft /home/runner/work/graft/graft/target/release/graft --work /mnt/graft-bench/mnt/xfs/bench --out /home/runner/work/graft/graft/bench/results/linux-x86_64-xfs-workloads --machine GitHub Actions ubuntu-latest runner --runs 3 --backends reflink,fuse` |

Each run creates one ready workspace per method (setup not timed: grafts build or reuse the `node_modules` layer, worktrees run `npm ci`), then times each workload inside it. Values are medians over runs of each run's median sample, ± the spread ((max − min) / median). Workloads: ADR 0010.

## medium

| workload | graft | graft-fuse | worktree |
| --- | ---: | ---: | ---: |
| build-cold | 641 ms ±1% | 4.62 s ±3% | 640 ms ±3% |
| rebuild | 223 ms ±3% | 284 ms ±2% | 221 ms ±1% |
| test | 146 ms ±2% | 164 ms ±21% ⚠ | 144 ms ±5% |
| git-status | 35 ms (±5%) | 83 ms (±3%) | 28 ms (±3%) |
| git-diff | 16 ms (±4%) | 22 ms (±30%) | 15 ms (±11%) |
| git-commit | 30 ms (±9%) | 57 ms (±5%) | 28 ms (±12%) |
| rg | 60 ms (±1%) | 1.34 s ±3% | 58 ms (±2%) |
| find | 15 ms (±1%) | 102 ms ±2% | 15 ms (±1%) |
| ls-R | 204 ms ±3% | 1.61 s ±1% | 205 ms ±1% |
| write-10k-small | 309 ms ±2% | 8.22 s ±2% | 292 ms ±1% |
| write-1gib | 2.59 s ±6% | 3.37 s ±1% | 2.60 s ±1% |

## Reproducibility

19 of 20 values are within ±10% across 3 runs (others marked ⚠). Values below 100 ms, or from a single run, show the spread in parentheses and are not counted.
