# Grafts vs worktrees

Default backend's method: `graft`. It must **beat** `worktree` on every create-matrix value (lower, by more than either side's spread; below the floor of 100 ms or 1 MiB, not higher) and must not lose on any workload inside a workspace. No graft method may **lose** anywhere.

Verdict: **fail**

| method | beats | ties | loses |
| --- | ---: | ---: | ---: |
| graft (default) | 2 | 1 | 6 |
| graft-nfs | 6 | 3 | 0 |

## large

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 44.73 s ±43% | 24.56 s ±22% | 1.82× | loses ✗ |
| ready, first agent x1 | graft | 45.03 s ±43% | 25.47 s ±23% | 1.77× | loses ✗ |
| disk added x1 | graft | 75.6 MiB ±2% | 1.4 GiB ±0% | 0.05× | beats |
| prune x1 | graft | 16.18 s ±30% | 12.00 s ±27% | 1.35× | ties ⚠ |
| create x4 | graft | 39.63 s ±15% | 24.31 s ±44% | 1.63× | loses ✗ |
| ready, first agent x4 | graft | 42.21 s ±15% | 26.85 s ±40% | 1.57× | loses ✗ |
| ready, later agents x4 | graft | 38.95 s ±11% | 25.29 s ±46% | 1.54× | loses ✗ |
| disk added x4 | graft | 302.4 MiB ±2% | 5.5 GiB ±0% | 0.05× | beats |
| prune x4 | graft | 19.45 s ±10% | 12.98 s ±50% | 1.50× | loses ✗ |
| create x1 | graft-nfs | 46.03 s ±49% | 24.56 s ±22% | 1.87× | ties ⚠ |
| ready, first agent x1 | graft-nfs | 46.31 s ±49% | 25.47 s ±23% | 1.82× | ties ⚠ |
| disk added x1 | graft-nfs | 7.7 MiB ±26% | 1.4 GiB ±0% | 0.01× | beats |
| prune x1 | graft-nfs | 55 ms ±51% | 12.00 s ±27% | 0.00× | beats |
| create x4 | graft-nfs | 49 ms ±46% | 24.31 s ±44% | 0.00× | beats |
| ready, first agent x4 | graft-nfs | 38.56 s ±49% | 26.85 s ±40% | 1.44× | ties ⚠ |
| ready, later agents x4 | graft-nfs | 243 ms ±46% | 25.29 s ±46% | 0.01× | beats |
| disk added x4 | graft-nfs | 7.8 MiB ±26% | 5.5 GiB ±0% | 0.00× | beats |
| prune x4 | graft-nfs | 21 ms ±35% | 12.98 s ±50% | 0.00× | beats |
