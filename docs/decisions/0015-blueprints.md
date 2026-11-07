# 0015: Blueprints
Status: accepted

## Context

AGENTS.md Phase 11 gives every graft its own runnable environment, declared
in `graft.toml` under `[blueprint.<name>]`: layers, setup commands, env
files, environment variables with templates, ports and services
(`postgres-template`, Docker volumes and networks). `graft new --blueprint
<name>` provisions it, `graft env <name>` prints the resolved environment,
and prune releases everything. Acceptance: two grafts of a web fixture run
their dev servers and tests at once without port or database collisions;
prune removes the database, volumes and port reservations; template errors
name the file and line.

Decisions needed: the template language, how ports are allocated and kept,
how services are provisioned and cleaned up, how setup commands are trusted
and run, how env files work, and where the resolved environment is used.

## Options considered

**Templates**

- A. A template crate (Tera, MiniJinja, Handlebars). Loops, filters and
  conditionals Graft does not need, a large dependency, and errors that are
  not in Graft's file:line format.
- B. A small `{{ name.path }}` substitution written for Graft: variables
  only, no expressions. Unknown variables and unclosed braces are errors
  with file, line and column; `{{{{` escapes a literal `{{`.

**Ports**

- C. Allocate on every `graft run`. The graft's URLs change between runs,
  and two runs could race for one port.
- D. Allocate once at `graft new`: bind `127.0.0.1:0`, take the port the
  kernel chose, close it, and record `(graft, name, port)` with a unique
  index on `port` across the Root. The reservation lasts until prune. A
  port another process took in the meantime shows up as a bind error in
  the app, the same as on any machine; `graft env` shows the port.
- E. A Linux network namespace per graft, so apps bind their usual port.
  Needs unprivileged user namespaces, which ADR 0014 avoided; deferred.

**Postgres databases**

- F. Talk to Postgres with a Rust driver. A new dependency, plus TLS and
  auth configuration that libpq already reads from the user's environment.
- G. Run `createdb --template=<template> <db>` and `dropdb --if-exists <db>`,
  which use the user's libpq settings (`PGHOST`, `PGUSER`, `~/.pgpass`).
  The database name is `<template>_<graft>` with hyphens as underscores,
  validated as a plain identifier of at most 63 bytes.

**Docker**

- H. Docker Compose integration. Graft would have to understand compose
  files.
- I. Name and create the resources and let the app use them:
  `kind = "docker-volume"` and `kind = "docker-network"` create
  `graft-<graft>-<name>` with the label `dev.graft.id=<graft-id>`, and
  expose the name as `{{service.<name>.name}}`. `COMPOSE_PROJECT_NAME` is
  set to `graft-<graft>` so a compose project is namespaced too. Prune
  removes only resources that carry the graft's label.

**Setup commands and trust**

- J. Run `setup` unconfined at `graft new`. Repository contents are
  untrusted (AGENTS.md §11).
- K. Each setup command, service and env file of a blueprint is approved
  with `graft trust` (one hash per command, per service, so editing one
  needs approval again), and `setup` runs through `graft run`'s sandbox in
  the new graft, with the blueprint's environment. Env files rendered from
  the graft's own checkout need no approval: they write only inside it.

**Env files**

- L. Each `env_files` entry is either a path that Root has but git ignores
  (such as `.env`), copied from Root's working tree into the graft, or a
  template ending in `.example` or `.template` from the graft's own
  checkout, rendered into the same path without the suffix. A table form
  `{ from = "...", to = "...", render = true|false }` covers anything else.

## Decision

B + D + G + I + K + L.

- **Schema.** `[blueprint.<name>]` with `layers` (paths of `[[layer]]`
  entries to apply; without it every layer applies), `setup`, `env_files`,
  `env` (string templates), `[[...port]]` (`name`, optional
  `container_port`, exposed as `{{port.<name>}}`), and `[[...service]]`
  (`name`, `kind`, and kind-specific keys). Unknown keys are errors.
- **Variables.** `graft.name`, `graft.id`, `graft.path`, `graft.blueprint`,
  `root.path`, `port.<name>`, `service.<name>.<field>` (`database` for
  Postgres, `name` for Docker), and `env.<NAME>` for the user's own
  environment. `env` values may refer to `port.*` and `service.*` but not
  to each other.
- **Provisioning on `graft new --blueprint <name>`**, after the graft
  exists: reserve ports, create services, copy and render env files,
  then run setup confined. Every resource is recorded before it is created,
  so a failure part-way is cleaned up by the same code as prune: the graft
  is pruned (forced) and `graft new` reports both the failure and whether
  the clean-up worked.
- **State.** Schema v7: `grafts.blueprint`, and `graft_resources (graft_id,
  kind, name, value)` holding ports (unique per Root), databases and Docker
  resources. Prune releases each resource, then deletes the rows.
- **Use.** `graft run` adds the blueprint's environment to the command.
  `graft env <name>` prints it as `NAME=value` lines, sorted by name, with values of names
  matching `*KEY*`, `*TOKEN*`, `*SECRET*`, `*PASSWORD*` redacted unless
  `--reveal` is given (AGENTS.md §11).
- **Prune order.** Databases and Docker resources are removed before the
  graft's files; a failure stops the prune with the graft marked pruned,
  so rerunning `graft prune` retries.

## Consequences

- No new dependencies: Postgres and Docker are driven through their CLIs,
  which must be on `PATH` for blueprints that use them.
- Ports are reserved, not held: Graft never binds them while the app is
  not running.
- On-disk format 9 (schema v7).
- The network-namespace option (E) waits for namespace support.
