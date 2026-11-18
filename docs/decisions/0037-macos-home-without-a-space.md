# 0037: A macOS default `$GRAFT_HOME` without a space
Status: accepted (supersedes 0022)

## Context

ADR 0022 kept `~/Library/Application Support/graft` as the macOS
default and made quoting visible (`graft doctor` warns). Two field reports
since then (Astrid, `reports/graft_report_astrid.md`; Turborepo,
`reports/graft_report_turborepo.md`) hit the space anyway: every script
and every agent prompt had to quote the graft path, and some tools behave
worse with it. Both asked for a default like `~/.graft`. The owner asked
for it.

ADR 0022 turned down option B (a new default for new homes, keeping an
existing one) because which default applies would depend on history.

## Options considered

- A. Keep ADR 0022.
- B. New homes in `~/.graft`; an existing `~/Library/Application
  Support/graft` keeps being used.
- C. Move existing homes too, with a migration: rewrites absolute paths
  in grafts' `.git`, worktree registrations (ADR 0036) and live mounts,
  with agents possibly working in them. Rejected, as in ADR 0022.

## Decision

B, with the rule spelled out so it is predictable:

1. `$GRAFT_HOME` if set (unchanged).
2. macOS: `~/Library/Application Support/graft` **if that directory
   exists**, else `~/.graft`.
3. Linux: unchanged (`$XDG_DATA_HOME/graft` or `~/.local/share/graft`,
   which have no space).

`graft doctor` reports which applies (`default`, or `default before 0.2,
kept because it exists`), and keeps warning when the home has a space.
`~/.graft` is still `$GRAFT_HOME`, so AGENTS.md §3.8 ("nothing outside
`$GRAFT_HOME`") holds; AGENTS.md §5 is updated.

## Consequences

- New installs on macOS get paths without spaces; existing installs and
  their grafts are untouched.
- Deleting an old `Application Support/graft` switches the next command
  to `~/.graft` (with no Roots registered). That is what a user deleting
  their Graft data expects; `graft doctor` shows the path either way.
- To move an existing install by hand: prune or integrate its grafts,
  then delete the old directory (or set `GRAFT_HOME`).
- Tests: `macos_default_ignores_xdg_and_has_no_space`,
  `macos_keeps_an_existing_application_support_home`,
  `linux_ignores_an_application_support_dir` (`graft-core`).
