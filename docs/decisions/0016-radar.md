# 0016: Radar
Status: accepted

## Context

AGENTS.md Phase 12: detect overlapping work between live grafts while
agents work, at three levels:

- **path**: two grafts changed the same file;
- **symbol**: they changed the same function, type or export (tree-sitter,
  for Rust, TypeScript, Python and Go first);
- **behaviour**: a speculative merge of two overlapping grafts fails a
  configured fast test command although each passes alone.

`graft radar` shows overlaps by severity, `--watch` keeps it current.
Acceptance: a same-function overlap is reported at symbol level within 5 s
of the second write; edits to different functions of one file are path
level only; a seeded behavioural conflict is detected; Radar adds < 5% to
write latency.

The scope text says the path index is "updated from op log events in the
daemon". The default backend (reflink, ADR 0003; kept as the default by
ADR 0013's decision) has no op log and its writes never pass through
graftd, so an op-log-driven index would miss most grafts.

## Options considered

**Where Radar runs and what drives it**

- A. In graftd, from op log events. Live and push-based, but blind to
  reflink grafts, and it puts parsing next to the write path, which the
  < 5% overhead target forbids.
- B. A library (`graft-radar`) fed with each live graft's changes against
  its base, as `graft diff` already computes them for both backends
  (reflink: git status of the working tree; virtual: the delta). `graft
  radar` runs it once; `--watch` re-runs it every second and prints what
  changed. Nothing runs on the write path.

**Symbols**

- C. Line ranges only. Cheap, but "same function" cannot be told from
  "neighbouring lines".
- D. tree-sitter: parse the base and the new version of each changed file
  and name its definitions (functions, methods qualified by their type or
  class, types, traits, interfaces, classes, top-level constants and
  exports). A symbol changed if its text differs, or it was added or
  removed. Changes outside every definition (imports, top-level code)
  belong to no symbol.

**Behaviour checks**

- E. Merge into a temporary virtual graft. Needs graftd and a FUSE or NFS
  mount on every machine.
- F. Temporary reflink checkouts of the shared base under
  `$GRAFT_HOME/roots/<root-id>/radar/`: one with A's changes, one with
  B's, one with both, overlapping files merged with `git merge-file`
  (three-way, base as ancestor). A textual merge conflict is reported as a
  conflict without running tests. Otherwise the configured command runs,
  confined like `graft run`, in each checkout. Results are cached by a
  fingerprint of the changes and the command, so an unchanged pair is not
  re-tested.

## Decision

B + D + F.

- **Levels and severity**, highest first: `behaviour` (merged fails, both
  alone pass), `conflict` (the three-way merge of a shared file fails),
  `symbol` (a shared changed symbol), `path` (a shared changed file, no
  shared symbol, or a language Radar cannot parse).
- **Languages**: Rust (`.rs`), TypeScript (`.ts`, `.tsx`), JavaScript is
  not parsed yet, Python (`.py`), Go (`.go`).
- **`[radar]` in `graft.toml`** at Root's HEAD: `test` (shell command) and
  `timeout_secs` (default 300). The command must be approved with `graft
  trust`, like every other command from the repository. Without it,
  behaviour checks are skipped and `graft radar` says so.
- **When checks run**: `graft radar --check` runs the speculative merges
  for every overlapping pair before printing; `--watch` runs them in the
  background as overlaps appear. Plain `graft radar` shows cached results.
- **Pairs with different bases** are reported at path and symbol level;
  the behaviour check needs a common base and is skipped with a note.
- **Output**: a table, or `--json` for tools (the MCP server, later).
- **State**: schema v8 adds `radar_checks (fingerprint, command, outcome,
  output, checked_at)`. Scratch checkouts are removed after each check.

## Consequences

- Radar never slows writes: it reads what `graft diff` reads, when asked.
- Latency is the watch interval (1 s by default) plus one scan of the live
  grafts' changes.
- graftd emits no Radar events yet. The MCP phase, which is their first
  consumer, can ask for a snapshot (`graft radar --json`) or add a daemon
  stream on top of this library.
- New dependencies: `tree-sitter` and four grammar crates.
- On-disk format 10: `radar/` scratch directories and schema v8.
