# Blueprint reference (`graft.toml`)

A Blueprint is the declarative recipe for a graft's environment. It lives in
`graft.toml` at the top of the repository and is read from the commit a
graft is created from, never from a working tree.

> Status: complete for this release: `[[layer]]`, `[sandbox]` and
> `[blueprint.<name>]`.

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

## `[sandbox]`

`graft run <name> -- <command>` lets the command write only to its graft
and a private temp dir (`TMPDIR`). Tool caches outside the graft can be
made writable here ([ADR 0014](decisions/0014-sandboxed-run.md)):

```toml
[sandbox]
write = ["~/.cargo", "~/.npm"]
```

| Key | Required | Meaning |
| --- | --- | --- |
| `write` | no | Paths the command may write below. Absolute, or starting with `~/` for the user's home directory. Not `/` or `~` itself, no `.` or `..` components. A path that does not exist when the command starts stays read-only, with a warning. |

Like layer commands, each path must be approved once with `graft trust`;
`graft run` refuses and lists unapproved paths. The table is read from
the graft's base commit, so an agent cannot widen its own sandbox by
editing `graft.toml` in its graft. For one run, the operator can add a
path with `graft run <name> --write <path> -- <command>`.

## `[blueprint.<name>]`

A blueprint gives each graft its own runnable environment: ports, a
database, Docker volumes and networks, env files and environment
variables, plus setup commands ([ADR 0015](decisions/0015-blueprints.md)).

```sh
graft new a --blueprint web   # provision it with the graft
graft env a                   # print the resolved environment (secrets redacted)
graft env a --reveal          # ... with secret values
graft run a -- npm run dev    # commands get the environment
graft prune a                 # drops the database, volumes, networks and ports
```

```toml
[blueprint.web]
layers = ["node_modules"]
setup = ["npm run db:migrate"]
env_files = [".env.example", ".secrets"]

[blueprint.web.env]
DATABASE_URL = "postgres://localhost:{{env.PGPORT}}/{{service.db.database}}"
APP_URL = "http://localhost:{{port.app}}/"

[[blueprint.web.port]]
name = "app"
container_port = 3000

[[blueprint.web.service]]
name = "db"
kind = "postgres-template"
template = "app_dev"

[[blueprint.web.service]]
name = "cache"
kind = "docker-volume"
```

| Key | Meaning |
| --- | --- |
| `layers` | `[[layer]]` paths to apply. Without it, every layer applies. |
| `setup` | Shell commands (`sh -c`) run in order in the new graft, confined like `graft run`, with the blueprint's environment. A failure removes the graft again. |
| `env_files` | Files to put into the graft. A path ending in `.example` or `.template` is a template in the graft's checkout, rendered to the same path without the suffix. Any other path is copied from Root's working tree (for ignored files such as `.secrets`). The table form `{ from = "...", to = "...", render = true }` covers the rest. |
| `env` | Environment variables, as templates. `graft run`, `graft env` and setup commands use them. `COMPOSE_PROJECT_NAME` defaults to `graft-<name>`. |
| `[[...port]]` | `name`, optional `container_port` (informational). A free loopback port is reserved per graft at `graft new` and kept until prune. |
| `[[...service]]` | `name` and `kind`: `postgres-template` (needs `template`: `createdb --template` makes `<template>_<graft>`), `docker-volume` or `docker-network` (`docker ... create`, named per graft and labelled). Graft drives the `createdb`/`dropdb` and `docker` CLIs; libpq reads `PGHOST`, `PGUSER` and so on as usual. |

### Templates

`{{ name }}` placeholders and nothing else; `{{{{` writes a literal `{{`.

| Variable | Value |
| --- | --- |
| `graft.name`, `graft.id`, `graft.path`, `graft.blueprint` | The graft |
| `root.path` | Root's working tree |
| `port.<name>` | The graft's reserved port |
| `service.<name>.database` | A Postgres service's database |
| `service.<name>.name` | A Docker service's volume or network name |
| `env.<NAME>` | A variable of the environment `graft` runs in |

Unknown variables are errors with the file, line and column, in
`graft.toml` when it is parsed and in env file templates when they are
rendered: `conf/app.env.template:2:7: unknown template variable port.nope`.

### Approval

`graft trust` approves each setup command, each service, and each env
file copied from Root. `graft new --blueprint` refuses, listing what is
unapproved, until then. Rendered env files need no approval: they are
read from the graft's own checkout and written inside it.

## `[radar]`

The fast test command for Radar's behaviour checks ([ADR 0016](decisions/0016-radar.md)).
When two live grafts overlap, `graft radar --check` (or `--watch`) merges
them in scratch checkouts of their base and runs it in each graft's
changes alone and in both merged. A failure that only appears merged is
reported as a `behaviour` overlap.

```toml
[radar]
test = "cargo test -q --lib"
timeout_secs = 120
```

| Key | Meaning |
| --- | --- |
| `test` | Shell command (`sh -c`), run confined in the scratch checkout's top level with `GRAFT_RADAR=1`. Approve it with `graft trust`. Without it, Radar reports path, symbol and merge-conflict overlaps only. |
| `timeout_secs` | How long one run may take (default 300). A run past it is killed and counts as failing. |

Layers already in the cache are cloned into the scratch checkouts; Radar
never builds one.

