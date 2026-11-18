# Graft benchmark: workloads inside grafts vs git worktrees

| | |
| --- | --- |
| machine | netcup VPS 1000 G12.5 (4 shared vCPU, 8 GiB), Debian 13, btrfs loop volume; host also runs unrelated Docker services |
| OS | Linux 6.12.113+deb13-amd64 (x86_64) |
| CPU | AMD EPYC-Rome Processor, 4 threads |
| filesystem | btrfs, reflinks supported (dedicated volume) |
| graft | graft 0.1.1 |
| git | git version 2.47.3 |
| node / npm | v20.19.2 / 9.2.0 |
| runs | 3 (a fresh workspace per method each run); 5 timed samples per workload after one discarded warm-up |
| load average at start | 2.26, 2.56, 2.61 |
| command | `/mnt/btrfs/graft/target/release/graft-bench workloads --graft /mnt/btrfs/graft/target/release/graft --work /mnt/bench/bench --out /mnt/btrfs/graft/bench/results/linux-x86_64-btrfs-netcup --machine netcup VPS 1000 G12.5 (4 shared vCPU, 8 GiB), Debian 13, btrfs loop volume; host also runs unrelated Docker services --backends reflink,fuse` |

Each run creates one ready workspace per method (setup not timed: grafts build or reuse the `node_modules` layer, worktrees run `npm ci`), then times each workload inside it. Values are medians over runs of each run's median sample, ± the spread ((max − min) / median). Workloads: ADR 0010.

## medium

| workload | graft | graft-fuse | worktree |
| --- | ---: | ---: | ---: |
| build-cold | 2.34 s ±14% ⚠ | 8.26 s ±9% | 2.29 s ±12% ⚠ |
| rebuild | 911 ms ±4% | 1.03 s ±12% ⚠ | 857 ms ±5% |
| test | 730 ms ±14% ⚠ | 862 ms ±13% ⚠ | 758 ms ±7% |
| git-status | 46 ms (±11%) | 117 ms ±19% ⚠ | 49 ms (±2%) |
| git-diff | 23 ms (±15%) | 33 ms (±6%) | 23 ms (±3%) |
| git-commit | 43 ms (±19%) | 76 ms (±22%) | 45 ms (±6%) |
| rg | 76 ms (±17%) | 1.70 s ±2% | 77 ms (±17%) |
| find | 27 ms (±13%) | 151 ms ±92% ⚠ | 29 ms (±7%) |
| ls-R | 242 ms ±7% | 1.28 s ±8% | 253 ms ±16% ⚠ |
| write-10k-small | 444 ms ±6% | 16.40 s ±4% | 460 ms ±12% ⚠ |
| write-1gib | 1.65 s ±12% ⚠ | 3.65 s ±4% | 1.70 s ±18% ⚠ |

## Reproducibility

10 of 21 values are within ±10% across 3 runs (others marked ⚠). Values below 100 ms, or from a single run, show the spread in parentheses and are not counted.
