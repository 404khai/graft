# 0039: Run one command in a graft without changing directories

Status: accepted

## Context

Agents that reset the working directory on each tool call have to repeat
`cd "$(graft path <name>)"` before every command. This adds quoting and
the lookup fails when the caller is outside a repository. A misplaced
`cd` can run the command in Root instead of the graft.

## Options considered

- Add only an explicit Root option to `graft path`. This fixes lookup from
  arbitrary directories but still requires shell substitution and quoting.
- Add the option to `graft run`. This helps confined agents, but does not
  replace `cd` for trusted commands that need normal user permissions.
- Add a local `graft exec <name> [--root <path>] -- <command...>` that sets
  the working directory and replaces itself with the requested program.
  The program gets the original arguments, streams, and signal handling.

## Decision

Use `graft exec`. Resolve the named live graft using the Root registry,
from the current directory by default or from `--root` when given. Mount
virtual grafts on demand. Start at the top of the graft, never at the
caller's directory. Pass arguments directly without invoking a shell.

## Consequences

This is a working-directory convenience for trusted commands; it does
not add confinement, Blueprint environment variables, or a private temp
directory. `graft run` remains the command for confining agents. There is
no Graft process left to collect command-completion telemetry after exec.
