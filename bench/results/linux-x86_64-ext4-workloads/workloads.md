# Graft benchmark: workloads inside grafts vs git worktrees

| | |
| --- | --- |
| machine | GitHub Actions ubuntu-latest runner |
| OS | Linux 7.0.0-1019-azure (x86_64) |
| CPU | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz, 4 threads |
| filesystem | ext4, reflinks unsupported (dedicated volume) |
| graft | graft 0.0.0 |
| git | git version 2.56.0 |
| node / npm | v24.22.2 / 11.19.1 |
| runs | 3 (a fresh workspace per method each run); 5 timed samples per workload after one discarded warm-up |
| load average at start | 3.95, 1.74, 0.67 |
| command | `/home/runner/work/graft/graft/target/release/graft-bench workloads --graft /home/runner/work/graft/graft/target/release/graft --work /mnt/graft-bench/mnt/ext4/bench --out /home/runner/work/graft/graft/bench/results/linux-x86_64-ext4-workloads --machine GitHub Actions ubuntu-latest runner --runs 3 --backends reflink,fuse` |

Each run creates one ready workspace per method (setup not timed: grafts build or reuse the `node_modules` layer, worktrees run `npm ci`), then times each workload inside it. Values are medians over runs of each run's median sample, ± the spread ((max − min) / median). Workloads: ADR 0010.

## medium

| workload | graft | graft-fuse | worktree |
| --- | ---: | ---: | ---: |
| build-cold | 587 ms ±5% | 4.54 s ±0% | 580 ms ±4% |
| rebuild | 210 ms ±2% | 253 ms ±2% | 199 ms ±1% |
| test | 142 ms ±2% | 155 ms ±2% | 142 ms ±4% |
| git-status | 33 ms (±4%) | 76 ms (±5%) | 28 ms (±5%) |
| git-diff | 13 ms (±11%) | 16 ms (±7%) | 12 ms (±5%) |
| git-commit | 24 ms (±1%) | 51 ms (±15%) | 24 ms (±5%) |
| rg | 46 ms (±1%) | 1.32 s ±3% | 45 ms (±2%) |
| find | 16 ms (±0%) | 80 ms (±1%) | 16 ms (±1%) |
| ls-R | 111 ms ±2% | 1.61 s ±4% | 112 ms ±2% |
| write-10k-small | 109 ms ±1% | 4.89 s ±2% | 107 ms ±1% |
| write-1gib | 2.93 s ±9% | 4.02 s ±0% | 2.90 s ±4% |

## Reproducibility

19 of 19 values are within ±10% across 3 runs (others marked ⚠). Values below 100 ms, or from a single run, show the spread in parentheses and are not counted.
