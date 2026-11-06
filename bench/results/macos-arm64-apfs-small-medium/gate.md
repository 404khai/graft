# Grafts vs worktrees

Default backend's method: `graft`. It must **beat** `worktree` on every create-matrix value (lower, by more than either side's spread; below the floor of 100 ms or 1 MiB, not higher) and must not lose on any workload inside a workspace. No graft method may **lose** anywhere.

Verdict: **fail**

| method | beats | ties | loses |
| --- | ---: | ---: | ---: |
| graft (default) | 17 | 19 | 2 |
| graft-nfs | 28 | 10 | 0 |

## small

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 666 ms ±65% | 296 ms ±39% | 2.25× | ties ⚠ |
| ready, first agent x1 | graft | 956 ms ±57% | 1.15 s ±30% | 0.83× | ties ⚠ |
| disk added x1 | graft | 880.0 KiB ±867% | 7.6 MiB ±16% | 0.11× | ties ⚠ |
| prune x1 | graft | 64 ms ±28% | 121 ms ±42% | 0.53× | beats |
| create x4 | graft | 152 ms ±18% | 211 ms ±38% | 0.72× | ties ⚠ |
| ready, first agent x4 | graft | 915 ms ±19% | 913 ms ±28% | 1.00× | ties ⚠ |
| ready, later agents x4 | graft | 387 ms ±11% | 831 ms ±31% | 0.47× | beats |
| disk added x4 | graft | 2.7 MiB ±86% | 31.0 MiB ±50% | 0.09× | beats |
| prune x4 | graft | 79 ms ±6% | 97 ms ±39% | 0.81× | beats |
| create x8 | graft | 139 ms ±39% | 232 ms ±39% | 0.60× | beats |
| ready, first agent x8 | graft | 1.01 s ±19% | 931 ms ±26% | 1.08× | ties ⚠ |
| ready, later agents x8 | graft | 373 ms ±37% | 818 ms ±33% | 0.46× | beats |
| disk added x8 | graft | 3.9 MiB ±25% | 61.7 MiB ±27% | 0.06× | beats |
| prune x8 | graft | 84 ms ±18% | 77 ms ±37% | 1.09× | loses ✗ |
| create x16 | graft | 152 ms ±5% | 194 ms ±22% | 0.78× | ties ⚠ |
| ready, first agent x16 | graft | 863 ms ±19% | 856 ms ±60% | 1.01× | ties ⚠ |
| ready, later agents x16 | graft | 410 ms ±1% | 808 ms ±24% | 0.51× | beats |
| disk added x16 | graft | 7.9 MiB ±74% | 124.1 MiB ±12% | 0.06× | beats |
| prune x16 | graft | 66 ms ±87% | 95 ms ±15% | 0.70× | beats |
| create x1 | graft-nfs | 713 ms ±79% | 296 ms ±39% | 2.41× | ties ⚠ |
| ready, first agent x1 | graft-nfs | 1.01 s ±75% | 1.15 s ±30% | 0.88× | ties ⚠ |
| disk added x1 | graft-nfs | 556.0 KiB ±46% | 7.6 MiB ±16% | 0.07× | beats |
| prune x1 | graft-nfs | 38 ms ±83% | 121 ms ±42% | 0.31× | beats |
| create x4 | graft-nfs | 64 ms ±25% | 211 ms ±38% | 0.31× | beats |
| ready, first agent x4 | graft-nfs | 988 ms ±51% | 913 ms ±28% | 1.08× | ties ⚠ |
| ready, later agents x4 | graft-nfs | 339 ms ±44% | 831 ms ±31% | 0.41× | beats |
| disk added x4 | graft-nfs | 714.0 KiB ±514% | 31.0 MiB ±50% | 0.02× | beats |
| prune x4 | graft-nfs | 30 ms ±41% | 97 ms ±39% | 0.31× | beats |
| create x8 | graft-nfs | 50 ms ±31% | 232 ms ±39% | 0.22× | beats |
| ready, first agent x8 | graft-nfs | 1.04 s ±22% | 931 ms ±26% | 1.12× | ties ⚠ |
| ready, later agents x8 | graft-nfs | 336 ms ±14% | 818 ms ±33% | 0.41× | beats |
| disk added x8 | graft-nfs | 1.0 MiB ±96% | 61.7 MiB ±27% | 0.02× | beats |
| prune x8 | graft-nfs | 35 ms ±23% | 77 ms ±37% | 0.46× | beats |
| create x16 | graft-nfs | 54 ms ±22% | 194 ms ±22% | 0.28× | beats |
| ready, first agent x16 | graft-nfs | 1.09 s ±15% | 856 ms ±60% | 1.28× | ties ⚠ |
| ready, later agents x16 | graft-nfs | 337 ms ±20% | 808 ms ±24% | 0.42× | beats |
| disk added x16 | graft-nfs | 1.1 MiB ±345% | 124.1 MiB ±12% | 0.01× | beats |
| prune x16 | graft-nfs | 30 ms ±24% | 95 ms ±15% | 0.32× | beats |

## medium

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 5.36 s ±23% | 3.44 s ±24% | 1.56× | loses ✗ |
| ready, first agent x1 | graft | 5.60 s ±22% | 5.14 s ±27% | 1.09× | ties ⚠ |
| disk added x1 | graft | 52.9 MiB ±1% | 185.7 MiB ±0% | 0.28× | beats |
| prune x1 | graft | 1.51 s ±11% | 1.83 s ±47% | 0.83× | ties ⚠ |
| create x4 | graft | 3.96 s ±21% | 3.30 s ±30% | 1.20× | ties ⚠ |
| ready, first agent x4 | graft | 5.90 s ±20% | 5.65 s ±25% | 1.04× | ties ⚠ |
| ready, later agents x4 | graft | 4.19 s ±22% | 5.88 s ±28% | 0.71× | beats |
| disk added x4 | graft | 83.4 MiB ±3% | 744.4 MiB ±0% | 0.11× | beats |
| prune x4 | graft | 1.83 s ±26% | 2.33 s ±17% | 0.79× | beats |
| create x8 | graft | 3.54 s ±42% | 3.24 s ±27% | 1.09× | ties ⚠ |
| ready, first agent x8 | graft | 6.09 s ±26% | 5.26 s ±29% | 1.16× | ties ⚠ |
| ready, later agents x8 | graft | 3.65 s ±59% | 5.36 s ±30% | 0.68× | ties ⚠ |
| disk added x8 | graft | 127.0 MiB ±1% | 1.5 GiB ±0% | 0.09× | beats |
| prune x8 | graft | 2.62 s ±11% | 2.86 s ±14% | 0.91× | ties ⚠ |
| create x16 | graft | 3.78 s ±5% | 3.18 s ±21% | 1.19× | ties ⚠ |
| ready, first agent x16 | graft | 6.74 s ±37% | 4.76 s ±43% | 1.42× | ties ⚠ |
| ready, later agents x16 | graft | 4.05 s ±2% | 5.30 s ±18% | 0.76× | beats |
| disk added x16 | graft | 208.6 MiB ±2% | 2.9 GiB ±1% | 0.07× | beats |
| prune x16 | graft | 2.78 s ±25% | 3.12 s ±3% | 0.89× | ties ⚠ |
| create x1 | graft-nfs | 5.15 s ±38% | 3.44 s ±24% | 1.50× | ties ⚠ |
| ready, first agent x1 | graft-nfs | 5.55 s ±37% | 5.14 s ±27% | 1.08× | ties ⚠ |
| disk added x1 | graft-nfs | 43.4 MiB ±0% | 185.7 MiB ±0% | 0.23× | beats |
| prune x1 | graft-nfs | 28 ms ±31% | 1.83 s ±47% | 0.02× | beats |
| create x4 | graft-nfs | 54 ms ±40% | 3.30 s ±30% | 0.02× | beats |
| ready, first agent x4 | graft-nfs | 5.56 s ±29% | 5.65 s ±25% | 0.98× | ties ⚠ |
| ready, later agents x4 | graft-nfs | 376 ms ±36% | 5.88 s ±28% | 0.06× | beats |
| disk added x4 | graft-nfs | 43.5 MiB ±2% | 744.4 MiB ±0% | 0.06× | beats |
| prune x4 | graft-nfs | 34 ms ±32% | 2.33 s ±17% | 0.01× | beats |
| create x8 | graft-nfs | 52 ms ±69% | 3.24 s ±27% | 0.02× | beats |
| ready, first agent x8 | graft-nfs | 5.92 s ±58% | 5.26 s ±29% | 1.13× | ties ⚠ |
| ready, later agents x8 | graft-nfs | 359 ms ±82% | 5.36 s ±30% | 0.07× | beats |
| disk added x8 | graft-nfs | 43.7 MiB ±2% | 1.5 GiB ±0% | 0.03× | beats |
| prune x8 | graft-nfs | 32 ms ±53% | 2.86 s ±14% | 0.01× | beats |
| create x16 | graft-nfs | 52 ms ±35% | 3.18 s ±21% | 0.02× | beats |
| ready, first agent x16 | graft-nfs | 6.00 s ±60% | 4.76 s ±43% | 1.26× | ties ⚠ |
| ready, later agents x16 | graft-nfs | 376 ms ±45% | 5.30 s ±18% | 0.07× | beats |
| disk added x16 | graft-nfs | 44.8 MiB ±2% | 2.9 GiB ±1% | 0.02× | beats |
| prune x16 | graft-nfs | 30 ms ±39% | 3.12 s ±3% | 0.01× | beats |
