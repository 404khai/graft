# 0022: Keeping the macOS default `$GRAFT_HOME` (with a space)
Status: accepted

## Context

On macOS the default `$GRAFT_HOME` is `~/Library/Application
Support/graft` (AGENTS.md §5), so every graft path contains a space.
The owner's benchmark noted that `cd $(graft path x)` breaks unless
quoted. Reflink grafts' working trees and virtual grafts' mount points
both live under `$GRAFT_HOME/roots/<root-id>/mounts/<name>/`.

## Options considered

- A. **Keep the default and make quoting visible.** Docs already quote
  (`cd "$(graft path auth)"`); `graft doctor` warns when the home path
  contains whitespace and says how to quote or override it.
- B. **Move new homes to a space-free default** (for example
  `~/.local/share/graft`), keeping an existing `Application Support` home
  when one is found. Two defaults on one platform: which one is in use
  depends on history, and a user who deletes one silently switches.
- C. **Move only `mounts/` to a space-free path** (for example
  `~/.graft-mounts/<root-id>/<name>`), with a format bump and a migration
  of existing grafts. Migrating means moving reflink working trees
  (whose `.git` and any build outputs may hold absolute paths, and in
  which agents may have shells open) and remounting live NFS grafts;
  a failure half-way leaves grafts at two locations. It also places Graft
  data outside `$GRAFT_HOME`, which AGENTS.md §3.8 asks to avoid.
- D. **A space-free symlink to the home.** `graft path` would print the
  link, but tools that canonicalise (`pwd -P`, git, the sandbox profile)
  would show the real path, so the space still appears.

## Decision

A. The default stays `~/Library/Application Support/graft`, which follows
Apple's convention, and existing installs are untouched (no format
change). `graft doctor` prints a `spaces` row when the home path contains
whitespace; README and docs keep quoting `$(graft path …)`, and point to
`GRAFT_HOME` for users who want a space-free location.

## Consequences

- Scripts must quote graft paths on macOS, as they must for any path in
  `~/Library`. This is documented, not enforced.
- A user can opt out with `GRAFT_HOME=~/.local/share/graft` (or any
  absolute path) before creating grafts; moving an existing home is a
  manual step (prune, move, recreate), as today.
- Test: `warns_when_graft_paths_contain_a_space` (tests/e2e/doctor.rs).
