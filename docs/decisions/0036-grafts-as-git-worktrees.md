# 0036: Grafts as git worktrees
Status: accepted

## Context

A graft is invisible to every tool pointed at Root. Its repository is a
private `.git` (ADR 0003) whose objects come from Root through
alternates, so `git worktree list`, editors (VS Code, JetBrains), git
clients (lazygit, GitHub Desktop, Tower) and agent hosts that list a
repository's worktrees never see it. Their worktree pickers are how
people choose where an agent works, so a graft cannot be picked there.

All of these tools read the same on-disk structure: a linked worktree is
a directory `$GIT_COMMON_DIR/worktrees/<id>/` holding `HEAD`, `commondir`
(the path back to the common directory), `gitdir` (the path of the
worktree's `.git` file) and `index`, plus a `.git` file in the working
tree that reads `gitdir: <that directory>`. git, libgit2, JGit and gix all
read these files directly; there is no registry or daemon behind them.
So a graft that writes them is a worktree to every tool, without any
tool knowing about Graft.

Checked with git 2.55:

- A hand-written registration is listed by `git worktree list`, `git
  status` works inside it, and its commits are visible from Root at once.
- `git worktree add` runs its checkout as a child `git reset --hard` from
  git's exec path, after writing the registration; creating grafts
  through git is out of scope (grafts are still made by `graft new`).
- `git worktree prune` drops a registration whose `.git` file is gone,
  unless it is `locked`. `git worktree remove` refuses a locked worktree.
- Removing a worktree deletes its files one by one and then the
  directory; a mount point cannot be removed that way.
- `info/exclude` is shared by every worktree. Per-worktree settings need
  `extensions.worktreeConfig = true` in the common config; git and gix
  honour it in a format-v0 repository, and older libgit2 and JGit ignore
  extensions there.

## Options considered

**Whether to register at all**

- A. Keep private repositories (ADR 0003) and expose grafts some other
  way (refs, a remote helper). Tools would still not list them as
  worktrees.
- B. Register every graft as a linked worktree. Git's rule that a branch
  can be checked out in one worktree only then applies to grafts: a graft
  could no longer check out `main` while Root has it. That breaks Phase
  2's criterion "a graft never shares a git branch lock with Root or
  another graft" (`grafts_do_not_take_branch_locks`).
- C. Register grafts only when the operator opts in, in
  `$GRAFT_HOME/config.toml`. The default stays ADR 0003 exactly.

**The graft's branch**

- D. `graft/<name>`, as in a private repository. Integrate's `branch` and
  `push` modes land on `refs/heads/graft/<name>` in Root (ADR 0017), so
  they would move the branch the graft has checked out.
- E. `graft-live/<name>`: in Root's refs, distinct from Integrate's
  branch, and clearly a live graft's.

**Removing a virtual graft through git**

- F. Leave it unlocked. `git worktree remove` would delete every file
  through the mount and then fail on the mount point.
- G. Lock it, with a reason naming `graft prune <name>`. git refuses to
  remove or prune it, so its registration also survives graftd being
  down or a dead FUSE mount.

**Per-graft settings** (layer excludes; `core.checkStat = minimal` and
`core.trustctime = false` for virtual grafts)

- H. `extensions.worktreeConfig = true` in Root's config and a
  `config.worktree` per graft, with `core.excludesFile` pointing at a
  file Graft writes: the user's global excludes, then the layer paths.
- I. Stable stat data from the mount, and a rule that layer paths must be
  ignored by Root's own `.gitignore`.

**Commits under `graft run`** (ADR 0014 allows writes only to the graft
and its temp dir; a registered graft's git state lives in Root's `.git`)

- J. Leave the policy alone: `git commit` fails inside `graft run`.
- K. Also allow writes to the graft's registration, Root's object store,
  and the graft's own ref namespace (`refs/heads/graft-live/<name>` is a
  file; its lock and reflog need `refs/heads/graft-live/` and
  `logs/refs/heads/graft-live/`). Root's working tree, config, hooks,
  index, `HEAD`, `packed-refs` and every other branch stay read-only.

## Decision

C + E + G + H + K.

- **Opt-in.** `[git] worktrees = true` in `config.toml` registers grafts
  created from then on. The layout of an existing graft never changes:
  each graft's layout is read from disk (a `.git` file for reflink
  grafts; a registration whose `gitdir` names the mount for virtual
  grafts), so turning the setting off or on affects only new grafts.
- **Registration.** `$GIT_COMMON_DIR/worktrees/graft-<name>/` holds
  `HEAD` (`ref: refs/heads/graft-live/<name>`), `commondir` (`../..`),
  `gitdir` (the absolute path of the graft's `.git`), `index` and
  `config.worktree`. Virtual grafts also hold `locked`. The branch is
  created at the base commit. An existing registration or branch of that
  name that is not this graft's is never overwritten: `graft new` stops
  and names it. A branch left over from an earlier graft of the same
  name is reused only if every commit on it is on another ref.
- **Reflink grafts** are built as before (ADR 0003, private repository in
  `mounts/.tmp-<id>/`, renamed into place) and then registered: the
  index moves into the registration and `.git` becomes a `.git` file.
  The repository has no objects of its own at that point, so nothing is
  lost. Scratch checkouts (Integrate, Radar, layer builds, materialize)
  are never registered.
- **Virtual grafts** are registered before they are mounted. The mount
  serves `.git` as a read-only file instead of a directory; its index is
  still built lazily, the first time `.git` is used (ADR 0008), so
  creating a virtual graft stays independent of the repository's size.
  git inside the graft then reads and writes its state directly on
  Root's filesystem, not through the mount.
- **Settings.** The first registration in a Root sets
  `extensions.worktreeConfig = true` in Root's config, with git's lock
  protocol (`config.lock`, then rename). A Root that sets it to `false`
  is refused with a clear error. `config.worktree` sets
  `core.checkStat` and `core.trustctime` for virtual grafts, and
  `core.excludesFile` for grafts with layers.
- **Prune** removes the registration (only if its `gitdir` names this
  graft) and the `graft-live/<name>` branch (unless another worktree has
  it checked out). The lost-work check (AGENTS.md §3.9) for a registered
  graft counts commits reachable from its `HEAD` or `graft-live/<name>`
  that are not reachable from the base or any other ref in Root.
- **A reflink graft removed with `git worktree remove`** shows as
  `missing` in `graft ls`; `graft prune <name>` then releases its
  Blueprint resources, pin and record. Releasing resources (databases,
  volumes) is never done implicitly.
- **`graft run`** on a registered graft adds the paths in K to the
  policy.

**Invariants:** a graft's layout never changes after creation; Graft
never removes a registration or branch that is not its own graft's;
prune never loses a commit that is on no other ref without `--force`;
the default (`worktrees` off) is byte-for-byte ADR 0003.

**Tests:** `registered_grafts_are_listed_as_git_worktrees`,
`registered_graft_commits_land_on_its_branch_in_root`,
`git_worktree_remove_then_prune_cleans_up_and_keeps_work`,
`prune_of_a_registered_graft_leaves_nothing_behind`,
`prune_refuses_commits_only_on_the_graft_branch`,
`a_registered_graft_cannot_take_a_branch_another_worktree_has`,
`the_setting_only_affects_new_grafts`,
`registered_graft_layers_are_ignored`,
`integrate_lands_a_registered_graft`,
`run_in_a_registered_graft_can_commit_but_not_touch_root` (e2e,
`tests/e2e/worktrees.rs`); registration, branch reuse and refusal,
unregister and lost-work units in `crates/graft-root/tests/worktree.rs`;
`a_registered_graft_serves_git_as_a_read_only_link` (the mount's `.git`,
without mounting); `registered_virtual_graft_is_a_locked_git_worktree`
in the NFS and FUSE e2e suites. Every existing test runs unchanged with
the default.

## Consequences

- Every tool that lists worktrees lists registered grafts, and an agent
  can be pointed at one from any worktree picker.
- A registered graft behaves like any worktree: it cannot check out a
  branch another worktree has; branches it creates are Root's branches;
  `git config` inside it writes Root's config; Root's hooks run in it.
  This is why it is opt-in.
- Commits made in a registered graft are in Root's object store at once.
- Under `graft run`, a registered graft can create objects in Root's
  object store and move its own branch, but no other branch; `git
  checkout -b`, `git stash` and `git fetch` need refs it may not write.
- `extensions.worktreeConfig` stays set in Root's config after the last
  registered graft is pruned. It is harmless and git's documented way to
  have per-worktree settings.
- `graft ls` counts the registration (mostly the index) in a graft's
  disk use.
- Format 14 (docs/on-disk-format.md): additive; grafts from earlier
  formats keep their private repositories.
