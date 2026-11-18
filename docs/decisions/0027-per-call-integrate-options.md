# 0027: Per-call integrate gate and branch creation
Status: accepted

## Context

The Astrid field report (`reports/graft_report_astrid.md`) landed two
grafts over MCP. Two steps needed work outside Graft:

- `integrate --onto feat/x` failed with "no branch feat/x": the agent had
  to create the branch in Root first.
- A test gate exists only as `[integrate] test` in `graft.toml`, read from
  the target's head. To gate a landing the agent would have had to commit
  `graft.toml` to the target branch, and then `graft trust` it.

Gates from `graft.toml` need `graft trust` because they are repository
contents (AGENTS.md §11): anyone who can push can change them.

## Options considered

- A. Keep both as they are and document the extra steps.
- B. `--create-branch` and `--test COMMAND` on `graft integrate`
  (`create_branch`, `test` on `graft_integrate`). A gate given this way
  comes from the operator, so it needs no trust approval.
- C. As B, but require `graft trust` for `--test` as well.

## Decision

B.

- `--create-branch` creates a missing target at Root's HEAD when the
  first request lands. The ref is written with an all-zero old value, so
  it is refused if the branch appeared meanwhile. A dry run creates
  nothing; it checks against Root's HEAD.
- `--test` replaces `[integrate] test` for the grafts named in this call
  (not for other requests the same queue run drains). The replaced gate
  never runs, so its approval is not checked; a resolver from
  `graft.toml` still needs its approval. The gate runs exactly like a
  configured one: `sh -c`, confined to the scratch checkout, with
  `[integrate] timeout_secs`.

C was rejected: an approval records "I reviewed this repository's
command". A command the operator types, or an agent the operator runs
passes over MCP, is the operator's own, like `graft run`'s command.

## Consequences

- No `graft.toml` commit is needed to gate or to land on a new branch.
- `graft_integrate` over MCP can now run a command chosen by the agent,
  confined to a scratch checkout like `graft_run`; §11 still holds.
- Tests: `create_branch_makes_a_missing_target_at_roots_head`,
  `a_per_call_gate_needs_no_graft_toml`, `a_per_call_gate_replaces_graft_tomls`
  (`tests/e2e/integrate.rs`).
