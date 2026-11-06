# graft new profile: medium fixture

| | |
| --- | --- |
| OS | Linux 7.0.0-1019-azure (x86_64) |
| volume | /mnt/graft-bench/mnt/xfs/profile (xfs) |
| runs | 3 |

| run | graft | step | ms |
| ---: | --- | --- | ---: |
| 1 | cold | plan layers | 0 |
| 1 | cold | check out tree | 1935 |
| 1 | cold | pin base | 1 |
| 1 | cold | lock layer | 0 |
| 1 | cold | build layer | 1864 |
| 1 | cold | install layer | 0 |
| 1 | cold | record layer | 0 |
| 1 | cold | clone layer | 1418 |
| 1 | cold | exclude layers | 0 |
| 1 | cold | graft new | 5304 |
| 1 | warm | plan layers | 0 |
| 1 | warm | check out tree | 992 |
| 1 | warm | pin base | 1 |
| 1 | warm | lock layer | 0 |
| 1 | warm | record layer | 0 |
| 1 | warm | clone layer | 454 |
| 1 | warm | exclude layers | 0 |
| 1 | warm | graft new | 1480 |
| 1 | worktree | git worktree add | 1081 |
| 1 | worktree | npm ci | 1308 |
| 2 | cold | plan layers | 0 |
| 2 | cold | check out tree | 623 |
| 2 | cold | pin base | 1 |
| 2 | cold | lock layer | 0 |
| 2 | cold | build layer | 1333 |
| 2 | cold | install layer | 0 |
| 2 | cold | record layer | 0 |
| 2 | cold | clone layer | 1130 |
| 2 | cold | exclude layers | 0 |
| 2 | cold | graft new | 3142 |
| 2 | warm | plan layers | 0 |
| 2 | warm | check out tree | 684 |
| 2 | warm | pin base | 1 |
| 2 | warm | lock layer | 0 |
| 2 | warm | record layer | 0 |
| 2 | warm | clone layer | 417 |
| 2 | warm | exclude layers | 0 |
| 2 | warm | graft new | 1131 |
| 2 | worktree | git worktree add | 1075 |
| 2 | worktree | npm ci | 1296 |
| 3 | cold | plan layers | 0 |
| 3 | cold | check out tree | 650 |
| 3 | cold | pin base | 1 |
| 3 | cold | lock layer | 0 |
| 3 | cold | build layer | 1363 |
| 3 | cold | install layer | 0 |
| 3 | cold | record layer | 0 |
| 3 | cold | clone layer | 1131 |
| 3 | cold | exclude layers | 0 |
| 3 | cold | graft new | 3196 |
| 3 | warm | plan layers | 0 |
| 3 | warm | check out tree | 697 |
| 3 | warm | pin base | 1 |
| 3 | warm | lock layer | 0 |
| 3 | warm | record layer | 0 |
| 3 | warm | clone layer | 438 |
| 3 | warm | exclude layers | 0 |
| 3 | warm | graft new | 1164 |
| 3 | worktree | git worktree add | 1092 |
| 3 | worktree | npm ci | 1305 |
