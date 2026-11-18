# 0029: Reporting sandbox denials, and opt-in toolchain caches
Status: accepted

## Context

The Astrid field report (`reports/graft_report_astrid.md`) found `graft_run`
unusable for a Swift package on macOS, and misleading for Rust tests:

- clang's module cache, `xcrun`'s database (both in the per-user
  `confstr(3)` cache and temp directories, not `$TMPDIR`) and SwiftPM's
  caches (`~/Library/Caches/org.swift.swiftpm`, `~/.swiftpm`,
  `~/Library/org.swift.swiftpm`) are outside the graft, so writes failed.
- Reproducing it showed a second cause: SwiftPM wraps manifest and plugin
  compilation in `sandbox-exec`, and macOS refuses `sandbox_apply` inside
  a sandbox (`sandbox-exec: sandbox_apply: Operation not permitted`). No
  write rule changes that; SwiftPM's `--disable-sandbox` does, and has no
  environment-variable equivalent.
- Two tests failed only under the sandbox, and nothing said so; an agent
  could have "fixed" working code.
- MCP's `graft_run` had no per-call `write` (the CLI has `--write`).

## Options considered

- A. Make toolchain caches writable by default. Breaks Graft's guarantee
  that a run writes only inside its graft, by default.
- B. An opt-in `--dev-caches` (MCP `dev_caches`), plus `write` on MCP.
- C. Rewrite `swift` commands to add `--disable-sandbox`. Surprising, and
  wrong for wrappers and scripts.
- D. Report denials: count failed writes with no help from the kernel
  (impossible), or read Seatbelt's own log entries.

## Decision

B, D, and a hint instead of C.

- Each run's Seatbelt profile denies writes `(with message
  "graft-run-<session>")`. When a run fails, `graft run` reads
  `log show --style ndjson --predicate 'eventMessage CONTAINS "<tag>"'`
  for the run's duration and reports the denied paths, unique and in
  order: a warning on the CLI, `sandbox: {denied, hint}` in `graft_run`'s
  result. The tag makes the attribution exact; other processes' denials
  never match. `log show` needs no privileges; it costs about a second,
  so it runs only after a failure.
- Linux: Landlock logs nothing an unprivileged process can read, so
  `denied` is empty there.
- When a failed command runs SwiftPM (`swift`, `swift-*`, directly, via
  `xcrun` or as a word of `sh -c`), the hint explains the nested-sandbox
  limit and suggests `--disable-sandbox` with dev caches allowed. Graft's
  own sandbox still confines the build.
- `--dev-caches` adds the existing ones of: macOS
  `DARWIN_USER_CACHE_DIR`, `DARWIN_USER_TEMP_DIR`, SwiftPM's three
  directories and `~/Library/Developer/Xcode/DerivedData`; Linux
  `$XDG_CACHE_HOME` or `~/.cache`. Nothing is created.

## Consequences

- With `--dev-caches` and `swift test --disable-sandbox`, a Swift package
  builds and tests inside `graft run` with no denials (macOS 27, Swift
  6.4.2).
- Dev caches are shared with every other run and process of the user, so
  a confined run can write into them; that is why they are opt-in. `$HOME`
  itself stays read-only (`write_and_dev_caches_widen_one_run`).
- A failed confined run on macOS takes about a second longer.
- The list is best effort. When macOS is overloaded, the unified log drops
  kernel sandbox reports (seen at load averages of 25–55 on an 8-core
  machine: even a plain `sandbox-exec` denial was not recorded), and the
  list is empty although writes were refused. The test checks the log
  with a plain `sandbox-exec` denial first and skips only the list check,
  saying so, when the log records nothing.
- Tests: `a_failed_run_names_the_writes_the_sandbox_refused` (macOS),
  `write_and_dev_caches_widen_one_run`, `denials_are_parsed_from_the_unified_log`,
  `swiftpm_commands_are_recognised`, `tagged_profiles_name_the_tag_in_each_denial`.
