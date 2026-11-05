# Grafts vs worktrees

Default backend's method: `graft`. It must **beat** `worktree` on every create-matrix value (lower, by more than either side's spread; below the floor of 100 ms or 1 MiB, not higher) and must not lose on any workload inside a workspace. No graft method may **lose** anywhere.

Verdict: **fail**

| method | beats | ties | loses |
| --- | ---: | ---: | ---: |
| graft (default) | 1 | 3 | 0 |
| graft-nfs | 6 | 0 | 3 |

## large

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 29.87 s ±11% | 26.52 s ±41% | 1.13× | ties ⚠ |
| ready, first agent x1 | graft | 30.05 s ±11% | 28.09 s ±41% | 1.07× | ties ⚠ |
| disk added x1 | graft | 75.2 MiB ±1% | 1.4 GiB ±0% | 0.05× | beats |
| prune x1 | graft | 12.26 s ±12% | 16.96 s ±29% | 0.72× | ties ⚠ |
| create x1 | graft-nfs | 59.88 s ±7% | 26.52 s ±41% | 2.26× | loses ✗ |
| ready, first agent x1 | graft-nfs | 60.18 s ±7% | 28.09 s ±41% | 2.14× | loses ✗ |
| disk added x1 | graft-nfs | 7.7 MiB ±39% | 1.4 GiB ±0% | 0.01× | beats |
| prune x1 | graft-nfs | 50 ms ±27% | 16.96 s ±29% | 0.00× | beats |
| create x4 | graft-nfs | 63 ms ±23% | 32.07 s ±25% | 0.00× | beats |
| ready, first agent x4 | graft-nfs | 58.17 s ±26% | 33.68 s ±23% | 1.73× | loses ✗ |
| ready, later agents x4 | graft-nfs | 310 ms ±31% | 33.35 s ±28% | 0.01× | beats |
| disk added x4 | graft-nfs | 7.9 MiB ±122% | 5.6 GiB ±0% | 0.00× | beats |
| prune x4 | graft-nfs | 33 ms ±17% | 17.82 s ±46% | 0.00× | beats |
