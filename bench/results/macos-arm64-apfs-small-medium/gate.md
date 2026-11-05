# Grafts vs worktrees

Default backend's method: `graft`. It must **beat** `worktree` on every create-matrix value (lower, by more than either side's spread; below the floor of 100 ms or 1 MiB, not higher) and must not lose on any workload inside a workspace. No graft method may **lose** anywhere.

Verdict: **fail**

| method | beats | ties | loses |
| --- | ---: | ---: | ---: |
| graft (default) | 19 | 18 | 1 |
| graft-nfs | 28 | 8 | 2 |

## small

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 825 ms ±44% | 281 ms ±23% | 2.93× | loses ✗ |
| ready, first agent x1 | graft | 1.10 s ±51% | 1.04 s ±8% | 1.06× | ties ⚠ |
| disk added x1 | graft | 900.0 KiB ±123% | 7.6 MiB ±18% | 0.12× | beats |
| prune x1 | graft | 89 ms ±40% | 118 ms ±29% | 0.76× | ties ⚠ |
| create x4 | graft | 166 ms ±27% | 263 ms ±20% | 0.63× | beats |
| ready, first agent x4 | graft | 979 ms ±15% | 1.02 s ±23% | 0.96× | ties ⚠ |
| ready, later agents x4 | graft | 415 ms ±23% | 1.01 s ±19% | 0.41× | beats |
| disk added x4 | graft | 2.3 MiB ±88% | 30.9 MiB ±56% | 0.07× | beats |
| prune x4 | graft | 71 ms ±11% | 100 ms ±10% | 0.71× | beats |
| create x8 | graft | 170 ms ±16% | 243 ms ±22% | 0.70× | beats |
| ready, first agent x8 | graft | 814 ms ±31% | 988 ms ±8% | 0.82× | ties ⚠ |
| ready, later agents x8 | graft | 429 ms ±12% | 952 ms ±16% | 0.45× | beats |
| disk added x8 | graft | 4.0 MiB ±146% | 61.0 MiB ±13% | 0.07× | beats |
| prune x8 | graft | 74 ms ±23% | 99 ms ±16% | 0.75× | beats |
| create x16 | graft | 162 ms ±26% | 246 ms ±16% | 0.66× | beats |
| ready, first agent x16 | graft | 1.00 s ±12% | 1.12 s ±35% | 0.89× | ties ⚠ |
| ready, later agents x16 | graft | 416 ms ±24% | 905 ms ±26% | 0.46× | beats |
| disk added x16 | graft | 7.9 MiB ±11% | 122.5 MiB ±14% | 0.06× | beats |
| prune x16 | graft | 72 ms ±47% | 89 ms ±4% | 0.81× | beats |
| create x1 | graft-nfs | 761 ms ±6% | 281 ms ±23% | 2.70× | loses ✗ |
| ready, first agent x1 | graft-nfs | 1.09 s ±15% | 1.04 s ±8% | 1.05× | ties ⚠ |
| disk added x1 | graft-nfs | 572.0 KiB ±76% | 7.6 MiB ±18% | 0.07× | beats |
| prune x1 | graft-nfs | 41 ms ±30% | 118 ms ±29% | 0.35× | beats |
| create x4 | graft-nfs | 59 ms ±36% | 263 ms ±20% | 0.22× | beats |
| ready, first agent x4 | graft-nfs | 1.06 s ±30% | 1.02 s ±23% | 1.03× | ties ⚠ |
| ready, later agents x4 | graft-nfs | 352 ms ±42% | 1.01 s ±19% | 0.35× | beats |
| disk added x4 | graft-nfs | 1.2 MiB ±434% | 30.9 MiB ±56% | 0.04× | beats |
| prune x4 | graft-nfs | 30 ms ±56% | 100 ms ±10% | 0.30× | beats |
| create x8 | graft-nfs | 55 ms ±23% | 243 ms ±22% | 0.22× | beats |
| ready, first agent x8 | graft-nfs | 1.38 s ±44% | 988 ms ±8% | 1.40× | ties ⚠ |
| ready, later agents x8 | graft-nfs | 339 ms ±32% | 952 ms ±16% | 0.36× | beats |
| disk added x8 | graft-nfs | 1.8 MiB ±900% | 61.0 MiB ±13% | 0.03× | beats |
| prune x8 | graft-nfs | 34 ms ±24% | 99 ms ±16% | 0.35× | beats |
| create x16 | graft-nfs | 58 ms ±27% | 246 ms ±16% | 0.23× | beats |
| ready, first agent x16 | graft-nfs | 1.23 s ±71% | 1.12 s ±35% | 1.09× | ties ⚠ |
| ready, later agents x16 | graft-nfs | 380 ms ±34% | 905 ms ±26% | 0.42× | beats |
| disk added x16 | graft-nfs | 1.4 MiB ±19% | 122.5 MiB ±14% | 0.01× | beats |
| prune x16 | graft-nfs | 30 ms ±13% | 89 ms ±4% | 0.34× | beats |

## medium

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 6.59 s ±36% | 4.53 s ±36% | 1.45× | ties ⚠ |
| ready, first agent x1 | graft | 6.81 s ±36% | 7.22 s ±28% | 0.94× | ties ⚠ |
| disk added x1 | graft | 52.9 MiB ±0% | 186.7 MiB ±0% | 0.28× | beats |
| prune x1 | graft | 1.95 s ±26% | 2.88 s ±39% | 0.68× | ties ⚠ |
| create x4 | graft | 4.38 s ±31% | 3.91 s ±22% | 1.12× | ties ⚠ |
| ready, first agent x4 | graft | 7.28 s ±14% | 6.46 s ±26% | 1.13× | ties ⚠ |
| ready, later agents x4 | graft | 4.34 s ±17% | 6.74 s ±18% | 0.64× | beats |
| disk added x4 | graft | 84.2 MiB ±2% | 744.3 MiB ±0% | 0.11× | beats |
| prune x4 | graft | 1.86 s ±23% | 2.37 s ±30% | 0.79× | ties ⚠ |
| create x8 | graft | 4.26 s ±13% | 4.12 s ±22% | 1.03× | ties ⚠ |
| ready, first agent x8 | graft | 6.03 s ±19% | 6.79 s ±10% | 0.89× | ties ⚠ |
| ready, later agents x8 | graft | 4.63 s ±11% | 6.88 s ±23% | 0.67× | beats |
| disk added x8 | graft | 125.5 MiB ±5% | 1.5 GiB ±0% | 0.08× | beats |
| prune x8 | graft | 2.50 s ±54% | 2.51 s ±39% | 0.99× | ties ⚠ |
| create x16 | graft | 4.16 s ±7% | 3.23 s ±39% | 1.29× | ties ⚠ |
| ready, first agent x16 | graft | 6.58 s ±30% | 5.70 s ±33% | 1.15× | ties ⚠ |
| ready, later agents x16 | graft | 4.37 s ±5% | 5.78 s ±35% | 0.76× | ties ⚠ |
| disk added x16 | graft | 209.8 MiB ±1% | 2.9 GiB ±1% | 0.07× | beats |
| prune x16 | graft | 3.10 s ±13% | 3.41 s ±38% | 0.91× | ties ⚠ |
| create x1 | graft-nfs | 7.16 s ±32% | 4.53 s ±36% | 1.58× | loses ✗ |
| ready, first agent x1 | graft-nfs | 7.60 s ±32% | 7.22 s ±28% | 1.05× | ties ⚠ |
| disk added x1 | graft-nfs | 43.4 MiB ±0% | 186.7 MiB ±0% | 0.23× | beats |
| prune x1 | graft-nfs | 56 ms ±19% | 2.88 s ±39% | 0.02× | beats |
| create x4 | graft-nfs | 61 ms ±36% | 3.91 s ±22% | 0.02× | beats |
| ready, first agent x4 | graft-nfs | 7.23 s ±30% | 6.46 s ±26% | 1.12× | ties ⚠ |
| ready, later agents x4 | graft-nfs | 432 ms ±32% | 6.74 s ±18% | 0.06× | beats |
| disk added x4 | graft-nfs | 43.5 MiB ±1% | 744.3 MiB ±0% | 0.06× | beats |
| prune x4 | graft-nfs | 39 ms ±52% | 2.37 s ±30% | 0.02× | beats |
| create x8 | graft-nfs | 67 ms ±44% | 4.12 s ±22% | 0.02× | beats |
| ready, first agent x8 | graft-nfs | 6.97 s ±53% | 6.79 s ±10% | 1.03× | ties ⚠ |
| ready, later agents x8 | graft-nfs | 462 ms ±35% | 6.88 s ±23% | 0.07× | beats |
| disk added x8 | graft-nfs | 44.7 MiB ±3% | 1.5 GiB ±0% | 0.03× | beats |
| prune x8 | graft-nfs | 37 ms ±36% | 2.51 s ±39% | 0.01× | beats |
| create x16 | graft-nfs | 50 ms ±36% | 3.23 s ±39% | 0.02× | beats |
| ready, first agent x16 | graft-nfs | 5.85 s ±37% | 5.70 s ±33% | 1.03× | ties ⚠ |
| ready, later agents x16 | graft-nfs | 336 ms ±43% | 5.78 s ±35% | 0.06× | beats |
| disk added x16 | graft-nfs | 44.4 MiB ±2% | 2.9 GiB ±1% | 0.01× | beats |
| prune x16 | graft-nfs | 36 ms ±30% | 3.41 s ±38% | 0.01× | beats |
