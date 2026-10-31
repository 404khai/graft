# 0004: Layer cache and build trust
Status: accepted

## Context

A layer is a directory tree that depends only on declared inputs: for
example `node_modules` on `package-lock.json`. The layer cache builds each
layer once per input hash and shares it with every graft. AGENTS.md leaves
several questions open:

1. Where a build runs, and how its output enters the cache.
2. What goes into the key.
3. How grafts reference layers so that `graft gc` never removes one in use,
   including when `graft new` and `graft gc` run at the same time.
4. How a graft's own writes to a layer stay private.
5. **Who may run build commands.** §11 says commands from `graft.toml` run
   only inside `graft run`, which does not exist until the sandboxing phase,
   yet this phase runs `build` during `graft new`.

## Options considered

**Where builds run**

- A. In a scratch directory holding copies of the inputs. Works for
  `npm ci`, but not for builds that need the source tree (`cargo build`).
- B. In the new graft's working tree, then move the produced directory into
  the cache and reflink it back. Every build sees exactly the tree it will
  be used with.

**Keeping references**

- C. Marker files beside each layer. A second source of truth, and racy.
- D. SQLite tables (`layers`, `layer_refs`) next to the graft records, with
  foreign keys, plus a per-layer file lock shared by builders and gc.

**Trust** (the choice was made by the project owner)

- E. Run declared commands without asking. Anything able to edit
  `graft.toml` (any agent with write access to Root) could then run
  arbitrary commands at the next `graft new`.
- F. Never run commands automatically; only reuse layers built by hand.
- G. A trust gate: run a command only after the user has approved that
  exact command, with the approval recorded as a hash. A changed command is a
  new hash and needs approval again.

## Decision

B + D + G.

- **Configuration** comes from `graft.toml` in the graft's base commit,
  read from the object store, never from Root's working tree. Each
  `[[layer]]` has `path`, `inputs`, `build` and an optional `toolchain`
  command whose output is part of the key (for example `node --version`).
- **Key** = `blake3::derive_key("graft layer v1", …)` over the length-prefixed
  layer path, build command, toolchain command and its output, OS and
  architecture, and every input's path and contents at the base commit.
- **Build.** `sh -c <build>` runs in the graft's top level with
  `GRAFT_NAME`, `GRAFT_ID` and `GRAFT_LAYER` set, and its output goes to
  `graft new`'s stderr. The resulting directory is renamed into
  `layers/<key>/` via a temporary name, so a layer directory is always
  complete, and then reflinked back into the graft.
- **Sharing.** Each graft gets a reflinked (or, failing that, copied) tree,
  so its writes to `node_modules` change only its own copies. Layer paths
  are added to the graft's `.git/info/exclude` so they never show as
  changes, and they never block prune.
- **References.** A `layer_refs` row ties a graft to a layer; the foreign
  key to `grafts` cascades on delete, so pruning a graft releases its
  layers. `layers/<key>.lock` (an OS file lock) is held by `graft new` from
  "is the layer ready?" until its reference row is written, and by `graft
  gc` while it rechecks and deletes. gc never deletes a layer that has a
  reference, and the `layers` row cannot be deleted while references exist
  (foreign key, no cascade).
- **Trust.** Every command in a layer (`build`, `toolchain`) is identified
  by `blake3::derive_key("graft trust v1", path ‖ build ‖ toolchain)`.
  `graft new` refuses to run an untrusted layer's commands and prints them,
  together with the `graft trust` invocation that approves them. `graft
  trust` records the hashes in `trusted_commands`. Layers already in the
  cache are reused without running anything, so they need no approval
  unless they declare a `toolchain` command, which runs on every
  `graft new`.
- **Sandboxing** is added on top when `graft run` exists; the trust gate
  stays.

## Consequences

- The first graft of each lockfile pays for one build; every later graft
  costs a reflinked tree.
- Builds see the graft's real path. Tools that embed absolute paths
  (`cargo`'s fingerprints) may rebuild some of their outputs in other
  grafts; they stay correct, just less warm.
- An agent that runs `graft trust` itself approves a command, but it does so
  visibly, through an explicit command that prints what it approved. Editing
  `graft.toml` alone is never enough.
- Approvals are per Root and survive edits to unrelated layers.
