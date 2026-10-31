# 0002: Root identity and lookup
Status: accepted

## Context

AGENTS.md §5 says a Root ID is derived from the repository's canonical path
plus its first commit hash, and names the Root's directory
`$GRAFT_HOME/roots/<root-id>/`. Two things are left open:

1. **Which "first commit".** A repository can have several root commits
   (unrelated histories merged in, orphan branches such as `gh-pages`), and
   which ones are reachable depends on where HEAD is.
2. **How to find a repository's Root later.** Recomputing the ID on every
   command means walking the whole first-parent history from HEAD, which
   costs time proportional to history length (hundreds of thousands of
   commits in large repositories). It is also unstable: switching HEAD to an
   orphan branch would produce a different ID for the same repository.

## Options considered

**First commit**

- A. The oldest root commit reachable from any ref. Changes whenever an
  unrelated ref is fetched.
- B. The root commit reached by following first parents from HEAD. Cheap to
  explain, deterministic for a given HEAD, and unaffected by merging in
  unrelated histories.
- C. The root of the remote default branch (`origin/HEAD`). Often missing
  locally, and absent for repositories without remotes.

**Lookup**

- D. Recompute the ID on every command. Has the cost and instability above.
- E. Keep a global path index (`$GRAFT_HOME/roots.db` or similar). Fast, but
  it is a second source of truth to keep consistent with the per-Root
  databases, and a write hot spot shared by every Root.
- F. Scan `$GRAFT_HOME/roots/*/graft.db` and match on the recorded path,
  confirming the repository still contains the recorded first commit.
  Costs one SQLite open per registered Root; with no extra state to keep
  consistent.

## Decision

Use B for the first commit, computed once by `graft init`, and F for lookup.

- `graft init` first looks the repository up (F). Only if nothing matches
  does it walk the history, derive the ID, and create the Root directory.
  `init` therefore stays idempotent even when HEAD has moved to an orphan
  branch since the first registration.
- If the same path is registered more than once (the repository was deleted
  and an unrelated one created in its place), the Root whose first commit the
  repository still contains wins, and on ties the most recent registration
  wins. A replaced repository therefore gets a new Root rather than
  inheriting the old one's grafts.
- A Root directory whose database cannot be read is logged and skipped, so
  one damaged Root never blocks the others.

## Consequences

- Each Graft command opens one SQLite database per registered Root to find
  its own. That is cheap for the tens of Roots a developer has; if it ever
  shows up in profiles, the daemon can cache the mapping in memory
  (`graftd` arrives with the virtual filesystem), or E can be added as a
  cache. The per-Root databases stay the source of truth either way.
- Moving a repository to a new path makes it a new Root. Graft cannot tell a
  moved repository from a copied one, and grafts are tied to their Root's
  directory, so this is the safe default. A `graft root move` command can be
  added later if needed.
- The derivation is pinned by a test (`root_id_is_stable_across_releases`).
  Changing it is an on-disk format change.
