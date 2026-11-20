# 0039: Preserve integration history
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
Replayed commits have new hashes. Merge commits are replayed relative to their
first parent; the landed history is linear when replay is necessary.
Dry runs construct candidate commits but move no Root refs.
