# Grafts vs worktrees

Default backend's method: `graft`. It must **beat** `worktree` on every create-matrix value (lower, by more than either side's spread; below the floor of 100 ms or 1 MiB, not higher) and must not lose on any workload inside a workspace. No graft method may **lose** anywhere.

Verdict: **fail**

| method | beats | ties | loses |
| --- | ---: | ---: | ---: |
| graft (default) | 2 | 7 | 2 |
| graft-nfs | 0 | 6 | 5 |

## medium

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| build-cold | graft | 1.29 s ±16% | 1.59 s ±25% | 0.81× | ties ⚠ |
| build-cold | graft-nfs | 3.08 s ±509% | 1.59 s ±25% | 1.94× | ties ⚠ |
| find | graft | 54 ms ±59% | 72 ms ±16% | 0.75× | beats |
| find | graft-nfs | 153 ms ±146% | 72 ms ±16% | 2.12× | ties ⚠ |
| git-commit | graft | 76 ms ±33% | 58 ms ±15% | 1.30× | loses ✗ |
| git-commit | graft-nfs | 250 ms ±140% | 58 ms ±15% | 4.28× | ties ⚠ |
| git-diff | graft | 41 ms ±57% | 32 ms ±30% | 1.26× | loses ✗ |
| git-diff | graft-nfs | 87 ms ±77% | 32 ms ±30% | 2.71× | loses ✗ |
| git-status | graft | 62 ms ±32% | 63 ms ±28% | 0.99× | beats |
| git-status | graft-nfs | 142 ms ±70% | 63 ms ±28% | 2.28× | ties ⚠ |
| ls-R | graft | 332 ms ±11% | 287 ms ±19% | 1.16× | ties ⚠ |
| ls-R | graft-nfs | 492 ms ±39% | 287 ms ±19% | 1.71× | loses ✗ |
| rebuild | graft | 340 ms ±10% | 324 ms ±36% | 1.05× | ties ⚠ |
| rebuild | graft-nfs | 375 ms ±14% | 324 ms ±36% | 1.16× | ties ⚠ |
| rg | graft | 199 ms ±13% | 176 ms ±51% | 1.13× | ties ⚠ |
| rg | graft-nfs | 274 ms ±15% | 176 ms ±51% | 1.55× | loses ✗ |
| test | graft | 233 ms ±10% | 222 ms ±21% | 1.05× | ties ⚠ |
| test | graft-nfs | 243 ms ±19% | 222 ms ±21% | 1.10× | ties ⚠ |
| write-10k-small | graft | 1.04 s ±28% | 852 ms ±37% | 1.22× | ties ⚠ |
| write-10k-small | graft-nfs | 47.82 s ±11% | 852 ms ±37% | 56.10× | loses ✗ |
| write-1gib | graft | 424 ms ±15% | 395 ms ±62% | 1.07× | ties ⚠ |
| write-1gib | graft-nfs | 18.26 s ±4% | 395 ms ±62% | 46.22× | loses ✗ |
