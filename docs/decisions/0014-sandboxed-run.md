# 0014: Sandboxed run
Status: accepted

## Context

AGENTS.md Phase 10: `graft run <name> -- <command...>` starts any command
(an agent, a build) so that it can only write inside its own graft. The
graft directory is read-write; Root and layers are read-only through the
mount; the temp dir is private; extra read-write paths (tool caches) can
be declared in `graft.toml`. `GRAFT_NAME` and `GRAFT_ID` are set, and the
op log can record the run session as the actor. A process inside must fail
to write to Root's working tree, another graft and `$HOME`; common
toolchains must keep working; a kernel without Landlock gets a clear error.

Decisions needed: the mechanism per platform, how the policy is applied to
a command Graft does not control, where extra paths come from, and what
"private temp dir" means.

## Options considered

**Linux mechanism**

- A. Landlock only: restrict the *write* access rights to a list of
  directories. Reads stay unrestricted, so toolchains see the whole system.
  No privileges needed; inherited by every child; cannot be lifted.
- B. Landlock plus user, mount and PID namespaces, to give the run its own
  `/tmp` and process tree. Unprivileged user namespaces are disabled on many
  distributions (Ubuntu 24.04's AppArmor restriction, hardened kernels), and
  namespaces need `fork` plus `unshare` with careful `unsafe` code.

**macOS mechanism**

- C. A generated Seatbelt profile run through `/usr/bin/sandbox-exec`:
  `(allow default)`, `(deny file-write*)`, then allow writes below the
  permitted paths. Deprecated as a public tool but shipped and used by
  Apple, Chromium and agent tools; no approval or extension needed.
- D. App Sandbox / entitlements. Needs code signing and does not apply to an
  arbitrary child command.

**Applying a Landlock policy to a command**

- E. `Command::pre_exec` with the Landlock calls. Runs between `fork` and
  `exec`, where allocating is unsound; needs `unsafe`.
- F. Re-exec: `graft run` starts `graft __confine --write <p>… -- <cmd>`,
  which restricts *itself* (an ordinary, safe call in a single-threaded
  process) and then `exec`s the command. No `unsafe`.

**Extra writable paths**

- G. Read `[sandbox] write = [...]` from the graft's working tree. The agent
  could edit it to grant itself `/`.
- H. Read it from `graft.toml` at the graft's base commit (as layers are,
  ADR 0004), and require approval with `graft trust`, like layer commands:
  repository contents are untrusted (AGENTS.md §11). The operator can add
  paths for one run with `graft run --write <path>`.

**Temp dir**

- I. Allow `/tmp`. Writes there are outside every graft, and tests and
  repositories often live under `/tmp`; Landlock cannot carve Root back out
  of an allowed directory, so this would open Root and other grafts.
- J. A private directory per graft, `$GRAFT_HOME/roots/<root-id>/tmp/<graft-id>/`,
  passed as `TMPDIR` (and `TMP`, `TEMP`), writable; `/tmp` itself is not.
  Tools that honour `TMPDIR` (cargo, rustc, npm, node, git, Python's
  `tempfile`, `mktemp`) work; a tool that hard-codes `/tmp` fails to write
  there, visibly.

## Decision

A + C + F + H + J.

- **Policy.** Writable: the graft's directory (its mount point for a
  virtual graft), its private temp dir, the approved `[sandbox] write`
  paths, and `--write` paths. Everything is canonicalised before use; a
  path that does not exist is refused, except the temp dir, which Graft
  creates. Terminal and null devices (`/dev/null`, `/dev/zero`,
  `/dev/full`, `/dev/tty`, `/dev/ptmx`, `/dev/pts`, and on macOS `/dev/fd`,
  `/dev/ttys*`, `/dev/dtracehelper`) stay writable. Reads and the network
  are not restricted.
- **Linux.** The best Landlock ABI the kernel has (best effort from ABI 1 up),
  handling every write right (including `Refer` and `Truncate` where
  available). If Landlock is not enforced at all, `graft run` stops with an
  error naming the kernel requirement (5.13+, `landlock` in the LSM list).
  If only partially enforced (an old ABI), it warns and runs.
- **macOS.** Paths are canonical (`/private/var/…`), so the profile matches
  what the kernel checks; strings are escaped. If `sandbox-exec` is missing
  or cannot apply a profile (for example inside another sandbox), the run
  fails with its message.
- **Run.** The command runs in the graft (or the same subdirectory, if
  `graft run` was started inside it), with `GRAFT_NAME`, `GRAFT_ID`,
  `GRAFT_RUN` (a random session ID) and the temp variables set. Its exit
  status is `graft run`'s (128 + signal if it was killed). A virtual graft
  is mounted first if needed.
- **Actor.** For a virtual graft, `graft run` sets graftd's actor for the
  graft to `run:<session>` while the command runs and clears it after, so
  op log rows name the run. Two concurrent runs in one graft share the
  latest actor. Reflink grafts keep no op log.
- **Prune** removes the graft's temp dir.

## Consequences

- No `unsafe`, no privileges, no new system configuration.
- New dependency `landlock` (Linux only).
- Namespaces (B) are not used: no private `/tmp` mount and no separate PID
  namespace. A sandboxed process can still signal the user's other
  processes, and a tool that insists on `/tmp` cannot write there. A later
  phase can add namespaces where the kernel allows them, behind the same
  policy.
- `graft trust` now also approves `[sandbox] write` paths; `graft run`
  refuses unapproved ones and lists them.
- On-disk format 8: `tmp/<graft-id>/` under each Root directory.
