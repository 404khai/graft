# 0039: Published ancestry for reflink status

Status: accepted

## Context

A pull inside a reflink graft moves Git HEAD but not its recorded base.
Counting published upstream changes as private work makes list and status
misleading. Integration still needs its recorded base; prune must retain its
existing checks for uncommitted files and commits not otherwise held by Root.

## Options considered

- Update the stored base after a pull.
- Always compare status with HEAD, hiding private commits.
- Derive a read-only display base from published ancestry.

## Decision

Only list and status derive an effective base. Consider merge-bases of HEAD
with the graft's remote-tracking refs and Root's local branches. Exclude the
registered graft branch even when HEAD is detached. For registered grafts,
only local branches checked out in Root's main workspace or another workspace
count as published; topic branches confined to this graft remain private.
A private repo's
same-named branch does not exclude an independent Root branch.

Keep only candidates descended from the recorded base. If HEAD is not
descended from it, retain the recorded base. Prefer descendant candidates;
for incomparable maximal candidates choose the first in a breadth-first
walk from HEAD (parent order breaks ties). Do not infer publication from
tags, graft-local branches, pins, or ring refs.

Skip unresolved publication refs, including dangling remote HEAD aliases.
An unborn HEAD retains the recorded base and reports zero committed paths.
Other display-base errors warn and fall back to the recorded base.

Status splits path counts into base-to-HEAD committed changes and
HEAD-to-working-files uncommitted changes. Virtual grafts retain delta-based
status and expose null split counts. Reflink rings explicitly identify
their snapshot commit, its HEAD parent, and the uncommitted path count.

## Consequences

No persisted format changes. Integration, rebase, diff, and Radar continue
using the recorded base; prune retains its existing work-at-risk checks.
JSON list/status include both bases;
the effective base can change when publication refs move.

Run results always include a sandbox object with confined, denied, and
hint fields. Completed runs are confined: unsupported confinement refuses
execution rather than falling back to an unrestricted child. Confined does
not claim every optional kernel write right is supported.
