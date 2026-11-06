# graft new profile: medium fixture

| | |
| --- | --- |
| OS | Linux 7.0.0-1019-azure (x86_64) |
| volume | /mnt/graft-bench/mnt/xfs/profile (xfs) |
| runs | 3 |

| run | graft | step | ms |
| ---: | --- | --- | ---: |
| 1 | cold (default) | plan layers | 0 |
| 1 | cold (default) | check out tree | 1693 |
| 1 | cold (default) | pin base | 1 |
| 1 | cold (default) | lock layer | 0 |
| 1 | cold (default) | build layer | 1645 |
| 1 | cold (default) | install layer | 0 |
| 1 | cold (default) | record layer | 0 |
| 1 | cold (default) | clone layer | 1088 |
| 1 | cold (default) | exclude layers | 0 |
| 1 | cold (default) | graft new | 4796 |
| 1 | warm (default) | plan layers | 0 |
| 1 | warm (default) | check out tree | 876 |
| 1 | warm (default) | pin base | 1 |
| 1 | warm (default) | lock layer | 0 |
| 1 | warm (default) | record layer | 0 |
| 1 | warm (default) | clone layer | 422 |
| 1 | warm (default) | exclude layers | 0 |
| 1 | warm (default) | graft new | 1489 |
| 1 | cold (sync) | plan layers | 0 |
| 1 | cold (sync) | check out tree | 1195 |
| 1 | cold (sync) | pin base | 1 |
| 1 | cold (sync) | lock layer | 0 |
| 1 | cold (sync) | build layer | 1286 |
| 1 | cold (sync) | install layer | 0 |
| 1 | cold (sync) | sync layer | 2185 |
| 1 | cold (sync) | record layer | 0 |
| 1 | cold (sync) | clone layer | 280 |
| 1 | cold (sync) | exclude layers | 0 |
| 1 | cold (sync) | graft new | 5014 |
| 1 | warm (sync) | plan layers | 0 |
| 1 | warm (sync) | check out tree | 668 |
| 1 | warm (sync) | pin base | 1 |
| 1 | warm (sync) | lock layer | 0 |
| 1 | warm (sync) | record layer | 0 |
| 1 | warm (sync) | clone layer | 301 |
| 1 | warm (sync) | exclude layers | 0 |
| 1 | warm (sync) | graft new | 1059 |
| 1 | cold (keep) | plan layers | 0 |
| 1 | cold (keep) | check out tree | 633 |
| 1 | cold (keep) | pin base | 1 |
| 1 | cold (keep) | lock layer | 0 |
| 1 | cold (keep) | build layer | 1254 |
| 1 | cold (keep) | clone layer into cache | 487 |
| 1 | cold (keep) | install layer | 0 |
| 1 | cold (keep) | record layer | 0 |
| 1 | cold (keep) | exclude layers | 0 |
| 1 | cold (keep) | graft new | 2698 |
| 1 | warm (keep) | plan layers | 0 |
| 1 | warm (keep) | check out tree | 674 |
| 1 | warm (keep) | pin base | 1 |
| 1 | warm (keep) | lock layer | 0 |
| 1 | warm (keep) | record layer | 0 |
| 1 | warm (keep) | clone layer | 244 |
| 1 | warm (keep) | exclude layers | 0 |
| 1 | warm (keep) | graft new | 1009 |
| 1 | worktree | git worktree add | 1207 |
| 1 | worktree | npm ci | 1185 |
| 2 | cold (default) | plan layers | 0 |
| 2 | cold (default) | check out tree | 660 |
| 2 | cold (default) | pin base | 1 |
| 2 | cold (default) | lock layer | 0 |
| 2 | cold (default) | build layer | 1245 |
| 2 | cold (default) | install layer | 0 |
| 2 | cold (default) | record layer | 0 |
| 2 | cold (default) | clone layer | 477 |
| 2 | cold (default) | exclude layers | 0 |
| 2 | cold (default) | graft new | 2664 |
| 2 | warm (default) | plan layers | 0 |
| 2 | warm (default) | check out tree | 698 |
| 2 | warm (default) | pin base | 1 |
| 2 | warm (default) | lock layer | 0 |
| 2 | warm (default) | record layer | 0 |
| 2 | warm (default) | clone layer | 226 |
| 2 | warm (default) | exclude layers | 0 |
| 2 | warm (default) | graft new | 1014 |
| 2 | cold (sync) | plan layers | 0 |
| 2 | cold (sync) | check out tree | 612 |
| 2 | cold (sync) | pin base | 1 |
| 2 | cold (sync) | lock layer | 0 |
| 2 | cold (sync) | build layer | 1266 |
| 2 | cold (sync) | install layer | 0 |
| 2 | cold (sync) | sync layer | 1687 |
| 2 | cold (sync) | record layer | 0 |
| 2 | cold (sync) | clone layer | 273 |
| 2 | cold (sync) | exclude layers | 0 |
| 2 | cold (sync) | graft new | 3906 |
| 2 | warm (sync) | plan layers | 0 |
| 2 | warm (sync) | check out tree | 651 |
| 2 | warm (sync) | pin base | 1 |
| 2 | warm (sync) | lock layer | 0 |
| 2 | warm (sync) | record layer | 0 |
| 2 | warm (sync) | clone layer | 308 |
| 2 | warm (sync) | exclude layers | 0 |
| 2 | warm (sync) | graft new | 1051 |
| 2 | cold (keep) | plan layers | 0 |
| 2 | cold (keep) | check out tree | 624 |
| 2 | cold (keep) | pin base | 1 |
| 2 | cold (keep) | lock layer | 0 |
| 2 | cold (keep) | build layer | 1204 |
| 2 | cold (keep) | clone layer into cache | 541 |
| 2 | cold (keep) | install layer | 0 |
| 2 | cold (keep) | record layer | 0 |
| 2 | cold (keep) | exclude layers | 0 |
| 2 | cold (keep) | graft new | 2646 |
| 2 | warm (keep) | plan layers | 0 |
| 2 | warm (keep) | check out tree | 815 |
| 2 | warm (keep) | pin base | 1 |
| 2 | warm (keep) | lock layer | 0 |
| 2 | warm (keep) | record layer | 0 |
| 2 | warm (keep) | clone layer | 308 |
| 2 | warm (keep) | exclude layers | 0 |
| 2 | warm (keep) | graft new | 1194 |
| 2 | worktree | git worktree add | 1190 |
| 2 | worktree | npm ci | 1208 |
| 3 | cold (default) | plan layers | 0 |
| 3 | cold (default) | check out tree | 627 |
| 3 | cold (default) | pin base | 1 |
| 3 | cold (default) | lock layer | 0 |
| 3 | cold (default) | build layer | 1221 |
| 3 | cold (default) | install layer | 0 |
| 3 | cold (default) | record layer | 0 |
| 3 | cold (default) | clone layer | 474 |
| 3 | cold (default) | exclude layers | 0 |
| 3 | cold (default) | graft new | 2597 |
| 3 | warm (default) | plan layers | 0 |
| 3 | warm (default) | check out tree | 669 |
| 3 | warm (default) | pin base | 1 |
| 3 | warm (default) | lock layer | 0 |
| 3 | warm (default) | record layer | 0 |
| 3 | warm (default) | clone layer | 235 |
| 3 | warm (default) | exclude layers | 0 |
| 3 | warm (default) | graft new | 993 |
| 3 | cold (sync) | plan layers | 0 |
| 3 | cold (sync) | check out tree | 645 |
| 3 | cold (sync) | pin base | 1 |
| 3 | cold (sync) | lock layer | 0 |
| 3 | cold (sync) | build layer | 1221 |
| 3 | cold (sync) | install layer | 0 |
| 3 | cold (sync) | sync layer | 1706 |
| 3 | cold (sync) | record layer | 0 |
| 3 | cold (sync) | clone layer | 276 |
| 3 | cold (sync) | exclude layers | 0 |
| 3 | cold (sync) | graft new | 3913 |
| 3 | warm (sync) | plan layers | 0 |
| 3 | warm (sync) | check out tree | 675 |
| 3 | warm (sync) | pin base | 1 |
| 3 | warm (sync) | lock layer | 0 |
| 3 | warm (sync) | record layer | 0 |
| 3 | warm (sync) | clone layer | 326 |
| 3 | warm (sync) | exclude layers | 0 |
| 3 | warm (sync) | graft new | 1093 |
| 3 | cold (keep) | plan layers | 0 |
| 3 | cold (keep) | check out tree | 633 |
| 3 | cold (keep) | pin base | 1 |
| 3 | cold (keep) | lock layer | 0 |
| 3 | cold (keep) | build layer | 1268 |
| 3 | cold (keep) | clone layer into cache | 484 |
| 3 | cold (keep) | install layer | 0 |
| 3 | cold (keep) | record layer | 0 |
| 3 | cold (keep) | exclude layers | 0 |
| 3 | cold (keep) | graft new | 2662 |
| 3 | warm (keep) | plan layers | 0 |
| 3 | warm (keep) | check out tree | 699 |
| 3 | warm (keep) | pin base | 1 |
| 3 | warm (keep) | lock layer | 0 |
| 3 | warm (keep) | record layer | 0 |
| 3 | warm (keep) | clone layer | 218 |
| 3 | warm (keep) | exclude layers | 0 |
| 3 | warm (keep) | graft new | 1010 |
| 3 | worktree | git worktree add | 1187 |
| 3 | worktree | npm ci | 1205 |
