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
