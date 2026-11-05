# Grafts vs worktrees

Default backend's method: `graft`. It must **beat** `worktree` on every create-matrix value (lower, by more than either side's spread; below the floor of 100 ms or 1 MiB, not higher) and must not lose on any workload inside a workspace. No graft method may **lose** anywhere.

Verdict: **fail**

| method | beats | ties | loses |
| --- | ---: | ---: | ---: |
| graft-nfs | 6 | 0 | 3 |

## large

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft-nfs | 70.44 s ±11% | 38.68 s ±28% | 1.82× | loses ✗ |
| ready, first agent x1 | graft-nfs | 70.97 s ±11% | 40.37 s ±28% | 1.76× | loses ✗ |
| disk added x1 | graft-nfs | 9.2 MiB ±22% | 1.4 GiB ±0% | 0.01× | beats |
| prune x1 | graft-nfs | 59 ms ±9% | 18.02 s ±23% | 0.00× | beats |
| create x4 | graft-nfs | 57 ms ±9% | 38.10 s ±38% | 0.00× | beats |
| ready, first agent x4 | graft-nfs | 63.61 s ±10% | 40.07 s ±36% | 1.59× | loses ✗ |
| ready, later agents x4 | graft-nfs | 336 ms ±3% | 39.72 s ±37% | 0.01× | beats |
| disk added x4 | graft-nfs | 8.3 MiB ±23% | 5.5 GiB ±0% | 0.00× | beats |
| prune x4 | graft-nfs | 34 ms ±17% | 17.36 s ±56% | 0.00× | beats |
