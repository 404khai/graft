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
