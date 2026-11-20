# 0041: Preserve integration history
Status: accepted
## Context
Squashing by default discards the ordered commits agents and reviewers rely on.
## Options considered
Always squash; preserve commits by default with explicit squash.
## Decision
Preserve commits in topological order, reusing their objects when the target
has not moved and replaying their changes with the existing resolver otherwise.
Keep messages and authors. Append uncommitted changes as a final commit.
Test the final tree before the existing compare-and-swap landing.
Expose squash per call in CLI and MCP, not in Blueprint configuration:
IntegrateMode selects the destination, not history policy, and repository
configuration should not silently discard a caller's commit structure.
## Consequences
Replayed commits have new hashes. Each commit is replayed on its mapped first
parent (or the target head for parents before the graft's base). Merge commits
apply their first-parent diff and retain their replayed additional parents, so
side-branch commits stay reachable without leaking discarded changes into the
merge's tree.
Dry runs construct candidate commits but move no Root refs.
