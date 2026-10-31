# Blueprint reference (`graft.toml`)

A Blueprint is the declarative recipe for a graft's environment. It lives in
`graft.toml` at the top of the repository and is read from the commit a
graft is created from, never from a working tree.

> Status: only `[[layer]]` exists so far. Ports, services, env files and
> templates arrive with the Blueprint phase.

Unknown keys are errors. Every error names the file, line and column:
`graft.toml:2:8: layer path must not contain . or .. components`.

## `[[layer]]`

A layer is a directory tree built once per input hash and shared by every
graft, for example `node_modules`. See [ADR 0004](decisions/0004-layer-cache-and-build-trust.md).

```toml
[[layer]]
path = "node_modules"
inputs = ["package.json", "package-lock.json"]
build = "npm ci"
toolchain = "node --version"
```

| Key | Required | Meaning |
| --- | --- | --- |
| `path` | yes | Directory the build produces, relative to the top level. Must not be tracked in git, inside `.git`, or overlap another layer. |
| `inputs` | yes | Files whose contents (at the graft's base commit) determine the layer. At least one; none inside `path`. |
| `build` | yes | Shell command (`sh -c`) run in the graft's top level, with `GRAFT_NAME`, `GRAFT_ID` and `GRAFT_LAYER` set. Must create `path`. Its output goes to stderr. |
| `toolchain` | no | Shell command run in Root's top level whose stdout is part of the key, so a new compiler or runtime gets a new layer. |

The key also includes the OS and architecture.

### What happens on `graft new`

1. Each layer's key is computed. If a layer is missing from the cache, or
   declares a `toolchain`, its commands must be approved (below).
2. A missing layer is built in the new graft, moved into the cache, and
   reflinked back. A present layer is reflinked in without running anything.
3. Layer paths are added to the graft's `.git/info/exclude`, so they never
   show as changes and never block `graft prune`.

Writes to a layer inside a graft only change that graft's copy.

### Approving commands: `graft trust`

Graft never runs a command from `graft.toml` that you have not approved.
`graft new` refuses, lists the commands, and points to:

```sh
graft trust              # approve the layer commands in graft.toml at HEAD
graft trust --from REF   # ... at another revision
```

Approval covers the exact `path`, `build` and `toolchain` of a layer. Edit
any of them and the layer needs approval again.

### Removing unused layers: `graft gc`

```sh
graft gc             # remove layers no graft uses
graft gc --dry-run   # report only
```

Layers referenced by any graft are never removed. Layers being built by a
concurrent `graft new` are skipped.
