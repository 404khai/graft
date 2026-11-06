# graft new profile: medium fixture

| | |
| --- | --- |
| OS | Linux 7.0.0-1019-azure (x86_64) |
| volume | /mnt/graft-bench/mnt/ext4/profile (ext4) |
| runs | 3 |

| run | graft | step | ms |
| ---: | --- | --- | ---: |
| 1 | cold | plan layers | 0 |
| 1 | cold | check out tree | 585 |
| 1 | cold | pin base | 1 |
| 1 | cold | lock layer | 0 |
| 1 | cold | build layer | 2126 |
| 1 | cold | install layer | 0 |
| 1 | cold | record layer | 0 |
| 1 | cold | clone layer | 381 |
| 1 | cold | exclude layers | 0 |
| 1 | cold | graft new | 3123 |
| 1 | warm | plan layers | 0 |
| 1 | warm | check out tree | 754 |
| 1 | warm | pin base | 1 |
| 1 | warm | lock layer | 0 |
| 1 | warm | record layer | 0 |
| 1 | warm | clone layer | 385 |
| 1 | warm | exclude layers | 0 |
| 1 | warm | graft new | 1343 |
| 1 | worktree | git worktree add | 1659 |
| 1 | worktree | npm ci | 1636 |
| 2 | cold | plan layers | 0 |
| 2 | cold | check out tree | 511 |
| 2 | cold | pin base | 1 |
| 2 | cold | lock layer | 0 |
| 2 | cold | build layer | 1472 |
| 2 | cold | install layer | 0 |
| 2 | cold | record layer | 0 |
| 2 | cold | clone layer | 338 |
| 2 | cold | exclude layers | 0 |
| 2 | cold | graft new | 2345 |
| 2 | warm | plan layers | 0 |
| 2 | warm | check out tree | 495 |
| 2 | warm | pin base | 1 |
| 2 | warm | lock layer | 0 |
| 2 | warm | record layer | 0 |
| 2 | warm | clone layer | 338 |
| 2 | warm | exclude layers | 0 |
| 2 | warm | graft new | 847 |
| 2 | worktree | git worktree add | 1346 |
| 2 | worktree | npm ci | 1496 |
| 3 | cold | plan layers | 0 |
| 3 | cold | check out tree | 491 |
| 3 | cold | pin base | 1 |
| 3 | cold | lock layer | 0 |
| 3 | cold | build layer | 1484 |
| 3 | cold | install layer | 0 |
| 3 | cold | record layer | 0 |
| 3 | cold | clone layer | 337 |
| 3 | cold | exclude layers | 0 |
| 3 | cold | graft new | 2335 |
| 3 | warm | plan layers | 0 |
| 3 | warm | check out tree | 490 |
| 3 | warm | pin base | 1 |
| 3 | warm | lock layer | 0 |
| 3 | warm | record layer | 0 |
| 3 | warm | clone layer | 336 |
| 3 | warm | exclude layers | 0 |
| 3 | warm | graft new | 839 |
| 3 | worktree | git worktree add | 1358 |
| 3 | worktree | npm ci | 1460 |
