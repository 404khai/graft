# Grafts vs worktrees

Default backend's method: `graft`. It must **beat** `worktree` on every create-matrix value (lower, by more than either side's spread; below the floor of 100 ms or 1 MiB, not higher) and must not lose on any workload inside a workspace. No graft method may **lose** anywhere.

Verdict: **fail**

| method | beats | ties | loses |
| --- | ---: | ---: | ---: |
| graft (default) | 0 | 8 | 3 |
| graft-nfs | 0 | 6 | 4 |

## medium

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| build-cold | graft | 1.41 s ±20% | 1.63 s ±43% | 0.87× | ties ⚠ |
| build-cold | graft-nfs | 3.51 s ±404% | 1.63 s ±43% | 2.16× | ties ⚠ |
| find | graft | 76 ms ±42% | 67 ms ±23% | 1.13× | loses ✗ |
| find | graft-nfs | 172 ms ±77% | 67 ms ±23% | 2.54× | ties ⚠ |
| git-commit | graft | 102 ms ±54% | 62 ms ±41% | 1.64× | ties ⚠ |
| git-commit | graft-nfs | 304 ms ±53% | 62 ms ±41% | 4.89× | loses ✗ |
| git-diff | graft | 41 ms ±31% | 38 ms ±16% | 1.09× | loses ✗ |
| git-diff | graft-nfs | 96 ms ±32% | 38 ms ±16% | 2.54× | loses ✗ |
| git-status | graft | 68 ms ±17% | 62 ms ±7% | 1.09× | loses ✗ |
| git-status | graft-nfs | 147 ms ±20% | 62 ms ±7% | 2.38× | loses ✗ |
| ls-R | graft | 323 ms ±34% | 317 ms ±14% | 1.02× | ties ⚠ |
| ls-R | graft-nfs | 488 ms ±39% | 317 ms ±14% | 1.54× | ties ⚠ |
| rebuild | graft | 375 ms ±32% | 390 ms ±39% | 0.96× | ties ⚠ |
| rebuild | graft-nfs | 450 ms ±25% | 390 ms ±39% | 1.15× | ties ⚠ |
| rg | graft | 234 ms ±53% | 198 ms ±21% | 1.18× | ties ⚠ |
| rg | graft-nfs | 264 ms ±48% | 198 ms ±21% | 1.33× | ties ⚠ |
| test | graft | 244 ms ±18% | 260 ms ±30% | 0.94× | ties ⚠ |
| test | graft-nfs | 299 ms ±18% | 260 ms ±30% | 1.15× | ties ⚠ |
| write-10k-small | graft | 1.22 s ±20% | 1.14 s ±1% | 1.07× | ties ⚠ |
| write-1gib | graft | 452 ms ±47% | 485 ms ±42% | 0.93× | ties ⚠ |
| write-1gib | graft-nfs | 21.66 s ±16% | 485 ms ±42% | 44.63× | loses ✗ |
