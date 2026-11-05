# Grafts vs worktrees

Default backend's method: `graft`. It must **beat** `worktree` on every create-matrix value (lower, by more than either side's spread; below the floor of 100 ms or 1 MiB, not higher) and must not lose on any workload inside a workspace. No graft method may **lose** anywhere.

Verdict: **fail**

| method | beats | ties | loses |
| --- | ---: | ---: | ---: |
| graft (default) | 14 | 19 | 5 |
| graft-nfs | 28 | 8 | 2 |

## small

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 717 ms ±17% | 202 ms ±47% | 3.55× | loses ✗ |
| ready, first agent x1 | graft | 1.02 s ±28% | 752 ms ±34% | 1.35× | ties ⚠ |
| disk added x1 | graft | 900.0 KiB ±11% | 7.6 MiB ±0% | 0.12× | beats |
| prune x1 | graft | 85 ms ±22% | 75 ms ±39% | 1.13× | loses ✗ |
| create x4 | graft | 182 ms ±39% | 217 ms ±34% | 0.84× | ties ⚠ |
| ready, first agent x4 | graft | 921 ms ±38% | 827 ms ±36% | 1.11× | ties ⚠ |
| ready, later agents x4 | graft | 496 ms ±33% | 794 ms ±32% | 0.62× | beats |
| disk added x4 | graft | 2.3 MiB ±2909% | 30.4 MiB ±260% | 0.07× | ties ⚠ |
| prune x4 | graft | 92 ms ±29% | 86 ms ±39% | 1.06× | loses ✗ |
| create x8 | graft | 174 ms ±47% | 230 ms ±43% | 0.76× | ties ⚠ |
| ready, first agent x8 | graft | 975 ms ±27% | 783 ms ±37% | 1.25× | ties ⚠ |
| ready, later agents x8 | graft | 481 ms ±36% | 830 ms ±34% | 0.58× | beats |
| disk added x8 | graft | 3.9 MiB ±14803% | 61.7 MiB ±87% | 0.06× | ties ⚠ |
| prune x8 | graft | 77 ms ±50% | 67 ms ±68% | 1.15× | loses ✗ |
| create x16 | graft | 134 ms ±49% | 237 ms ±30% | 0.56× | beats |
| ready, first agent x16 | graft | 977 ms ±10% | 911 ms ±17% | 1.07× | ties ⚠ |
| ready, later agents x16 | graft | 343 ms ±44% | 900 ms ±25% | 0.38× | beats |
| disk added x16 | graft | −5.2 MiB ±1625% | 137.6 MiB ±11% | -0.04× | beats |
| prune x16 | graft | 62 ms ±68% | 81 ms ±50% | 0.77× | beats |
| create x1 | graft-nfs | 586 ms ±39% | 202 ms ±47% | 2.90× | loses ✗ |
| ready, first agent x1 | graft-nfs | 821 ms ±40% | 752 ms ±34% | 1.09× | ties ⚠ |
| disk added x1 | graft-nfs | 604.0 KiB ±14% | 7.6 MiB ±0% | 0.08× | beats |
| prune x1 | graft-nfs | 27 ms ±47% | 75 ms ±39% | 0.36× | beats |
| create x4 | graft-nfs | 58 ms ±28% | 217 ms ±34% | 0.27× | beats |
| ready, first agent x4 | graft-nfs | 944 ms ±46% | 827 ms ±36% | 1.14× | ties ⚠ |
| ready, later agents x4 | graft-nfs | 309 ms ±56% | 794 ms ±32% | 0.39× | beats |
| disk added x4 | graft-nfs | 734.0 KiB ±8% | 30.4 MiB ±260% | 0.02× | ties ⚠ |
| prune x4 | graft-nfs | 28 ms ±36% | 86 ms ±39% | 0.32× | beats |
| create x8 | graft-nfs | 51 ms ±40% | 230 ms ±43% | 0.22× | beats |
| ready, first agent x8 | graft-nfs | 990 ms ±42% | 783 ms ±37% | 1.26× | ties ⚠ |
| ready, later agents x8 | graft-nfs | 305 ms ±47% | 830 ms ±34% | 0.37× | beats |
| disk added x8 | graft-nfs | 1.8 MiB ±45% | 61.7 MiB ±87% | 0.03× | beats |
| prune x8 | graft-nfs | 29 ms ±51% | 67 ms ±68% | 0.42× | beats |
| create x16 | graft-nfs | 54 ms ±29% | 237 ms ±30% | 0.23× | beats |
| ready, first agent x16 | graft-nfs | 981 ms ±28% | 911 ms ±17% | 1.08× | ties ⚠ |
| ready, later agents x16 | graft-nfs | 332 ms ±27% | 900 ms ±25% | 0.37× | beats |
| disk added x16 | graft-nfs | 2.3 MiB ±1069% | 137.6 MiB ±11% | 0.02× | beats |
| prune x16 | graft-nfs | 27 ms ±47% | 81 ms ±50% | 0.33× | beats |

## medium

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 5.53 s ±13% | 3.35 s ±21% | 1.65× | loses ✗ |
| ready, first agent x1 | graft | 5.82 s ±12% | 5.35 s ±23% | 1.09× | ties ⚠ |
| disk added x1 | graft | 52.9 MiB ±1% | 185.8 MiB ±0% | 0.28× | beats |
| prune x1 | graft | 1.58 s ±25% | 1.94 s ±23% | 0.82× | ties ⚠ |
| create x4 | graft | 3.95 s ±5% | 3.30 s ±22% | 1.20× | ties ⚠ |
| ready, first agent x4 | graft | 5.56 s ±7% | 4.96 s ±26% | 1.12× | ties ⚠ |
| ready, later agents x4 | graft | 4.17 s ±4% | 6.23 s ±21% | 0.67× | beats |
| disk added x4 | graft | 84.2 MiB ±1% | 744.5 MiB ±0% | 0.11× | beats |
| prune x4 | graft | 1.96 s ±3% | 2.12 s ±28% | 0.92× | ties ⚠ |
| create x8 | graft | 3.99 s ±12% | 4.00 s ±35% | 1.00× | ties ⚠ |
| ready, first agent x8 | graft | 6.94 s ±38% | 6.55 s ±43% | 1.06× | ties ⚠ |
| ready, later agents x8 | graft | 4.30 s ±12% | 6.89 s ±32% | 0.62× | beats |
| disk added x8 | graft | 125.7 MiB ±0% | 1.5 GiB ±0% | 0.08× | beats |
| prune x8 | graft | 2.25 s ±51% | 2.99 s ±39% | 0.75× | ties ⚠ |
| create x16 | graft | 3.74 s ±14% | 3.99 s ±34% | 0.94× | ties ⚠ |
| ready, first agent x16 | graft | 7.92 s ±20% | 6.88 s ±22% | 1.15× | ties ⚠ |
| ready, later agents x16 | graft | 3.99 s ±12% | 6.72 s ±33% | 0.59× | beats |
| disk added x16 | graft | 209.4 MiB ±1% | 2.9 GiB ±0% | 0.07× | beats |
| prune x16 | graft | 2.72 s ±32% | 3.18 s ±29% | 0.86× | ties ⚠ |
| create x1 | graft-nfs | 6.40 s ±26% | 3.35 s ±21% | 1.91× | loses ✗ |
| ready, first agent x1 | graft-nfs | 6.83 s ±26% | 5.35 s ±23% | 1.28× | ties ⚠ |
| disk added x1 | graft-nfs | 43.4 MiB ±0% | 185.8 MiB ±0% | 0.23× | beats |
| prune x1 | graft-nfs | 42 ms ±34% | 1.94 s ±23% | 0.02× | beats |
| create x4 | graft-nfs | 59 ms ±22% | 3.30 s ±22% | 0.02× | beats |
| ready, first agent x4 | graft-nfs | 5.30 s ±10% | 4.96 s ±26% | 1.07× | ties ⚠ |
| ready, later agents x4 | graft-nfs | 389 ms ±10% | 6.23 s ±21% | 0.06× | beats |
| disk added x4 | graft-nfs | 44.0 MiB ±0% | 744.5 MiB ±0% | 0.06× | beats |
| prune x4 | graft-nfs | 31 ms ±7% | 2.12 s ±28% | 0.01× | beats |
| create x8 | graft-nfs | 51 ms ±11% | 4.00 s ±35% | 0.01× | beats |
| ready, first agent x8 | graft-nfs | 5.54 s ±8% | 6.55 s ±43% | 0.85× | ties ⚠ |
| ready, later agents x8 | graft-nfs | 362 ms ±10% | 6.89 s ±32% | 0.05× | beats |
| disk added x8 | graft-nfs | 43.7 MiB ±1% | 1.5 GiB ±0% | 0.03× | beats |
| prune x8 | graft-nfs | 32 ms ±44% | 2.99 s ±39% | 0.01× | beats |
| create x16 | graft-nfs | 50 ms ±18% | 3.99 s ±34% | 0.01× | beats |
| ready, first agent x16 | graft-nfs | 5.25 s ±24% | 6.88 s ±22% | 0.76× | beats |
| ready, later agents x16 | graft-nfs | 343 ms ±18% | 6.72 s ±33% | 0.05× | beats |
| disk added x16 | graft-nfs | 44.9 MiB ±1% | 2.9 GiB ±0% | 0.02× | beats |
| prune x16 | graft-nfs | 26 ms ±43% | 3.18 s ±29% | 0.01× | beats |
