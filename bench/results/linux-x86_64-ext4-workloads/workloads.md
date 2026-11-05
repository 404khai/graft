# Graft benchmark: workloads inside grafts vs git worktrees

| | |
| --- | --- |
| machine | GitHub Actions ubuntu-latest runner |
| OS | Linux 7.0.0-1017-azure (x86_64) |
| CPU | AMD EPYC 7763 64-Core Processor, 4 threads |
| filesystem | ext4, reflinks unsupported (dedicated volume) |
| graft | graft 0.0.0 |
| git | git version 2.56.0 |
| node / npm | v24.22.1 / 11.19.1 |
| runs | 3 (a fresh workspace per method each run); 5 timed samples per workload after one discarded warm-up |
| load average at start | 4.48, 2.06, 0.81 |
| command | `/home/runner/work/graft/graft/target/release/graft-bench workloads --graft /home/runner/work/graft/graft/target/release/graft --work /mnt/graft-bench/mnt/ext4/bench --out /home/runner/work/graft/graft/bench/results/linux-x86_64-ext4-workloads --machine GitHub Actions ubuntu-latest runner --runs 3 --backends reflink,fuse` |

Each run creates one ready workspace per method (setup not timed: grafts build or reuse the `node_modules` layer, worktrees run `npm ci`), then times each workload inside it. Values are medians over runs of each run's median sample, ± the spread ((max − min) / median). Workloads: ADR 0010.

## medium

| workload | graft | graft-fuse | worktree |
| --- | ---: | ---: | ---: |
| build-cold | 728 ms ±9% | 4.65 s ±4% | 715 ms ±4% |
| rebuild | 250 ms ±9% | 304 ms ±7% | 253 ms ±11% ⚠ |
| test | 168 ms ±6% | 177 ms ±5% | 166 ms ±4% |
| git-status | 42 ms (±5%) | 86 ms (±2%) | 35 ms (±5%) |
| git-diff | 18 ms (±10%) | 22 ms (±8%) | 17 ms (±7%) |
| git-commit | 32 ms (±9%) | 61 ms (±6%) | 30 ms (±1%) |
| rg | 58 ms (±3%) | 1.36 s ±2% | 57 ms (±3%) |
| find | 22 ms (±1%) | 106 ms ±2% | 22 ms (±1%) |
| ls-R | 199 ms ±3% | 1.62 s ±1% | 201 ms ±2% |
| write-10k-small | 240 ms ±2% | 6.34 s ±2% | 238 ms ±1% |
| write-1gib | 2.90 s ±4% | 4.15 s ±3% | 2.75 s ±11% ⚠ |

## Reproducibility

18 of 20 values are within ±10% across 3 runs (others marked ⚠). Values below 100 ms, or from a single run, show the spread in parentheses and are not counted.
