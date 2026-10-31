# Status

Phase checklist for Graft. Acceptance criteria are copied from
[AGENTS.md §9](../AGENTS.md#9-phases) and ticked only when met and verified.
Items marked *target* are performance goals; the PR that meets or misses
them records the measured number.

## Phase 0 — Foundation

Status: done (#1)

- [x] `cargo build`, `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check`, `cargo deny check` pass on Linux and macOS in CI.
- [x] `graft doctor` correctly reports reflink support on APFS and btrfs, and its absence on ext4.
  - APFS: supported (clonefile), on the macOS runner and locally.
  - ext4: not supported, on the Ubuntu runner and a loop mount.
  - btrfs and XFS (`reflink=1`): supported (FICLONE), on loop mounts in the privileged job.
- [x] Every non-FS crate has `#![forbid(unsafe_code)]`. Enforced by `repo_policy::unsafe_code_is_forbidden_outside_fs_and_sandbox`; `graft-fs` and `graft-sandbox` use `#![deny(unsafe_code)]` with per-module allows.

## Phase 1 — Root registry and metadata store

Status: in review (`feat/root-registry`)

- [x] `graft init` is idempotent and refuses non-git directories with a clear error.
  - `root::init_registers_then_is_idempotent`, `root::init_refuses_a_directory_that_is_not_a_repository`; also refuses bare and empty repositories.
- [x] Schema migrations are tested from an empty DB.
  - `graft-store/tests/migrations.rs`: empty directory, empty file, re-open, newer-version refusal, rollback of a failed migration, hash pin on released migrations.
- [x] Concurrent CLI invocations do not corrupt the DB (test with parallel processes).
  - `root::parallel_inits_register_each_root_once_without_corruption` (24 `graft init` processes over 2 repos, then mixed `init`/`root`), and `graft-store/tests/concurrency.rs` (8 processes writing 201 grafts and racing for one name). `integrity_check` is clean; 0 failures in 30 repeated local runs.
- [x] Graft names are validated (lowercase, digits, hyphens, ≤ 40 chars) and unique per Root.
  - `GraftName` (also: must start with a letter or digit and not end with a hyphen); a partial unique index among unpruned grafts; `names_are_unique_among_unpruned_grafts`.

## Phase 2 — Reflink grafts

Status: not started

- [ ] On APFS and btrfs, creating a graft of a 1 GB working tree uses < 5 MB of new disk (*target*), measured by `graft ls`.
- [ ] Two grafts from the same Root can be created, edited and pruned independently.
- [ ] A graft never shares a git branch lock with Root or another graft.
- [ ] Prune leaves no files, mounts or DB rows behind (verified by test).
- [ ] Copy fallback works on ext4 and prints a single clear warning.

## Phase 3 — Layer cache

Status: not started

- [ ] With a Node fixture, the second and later `graft new` calls do not run the install command.
- [ ] Changing the lockfile produces a new layer; the old one is kept while referenced.
- [ ] `graft gc` never removes a layer referenced by a live graft (tested).
- [ ] Time from `graft new` to first passing `npm test` on the fixture is < 3 s after the first graft (*target*).

## Phase 4 — Benchmark harness

Status: not started

- [ ] One command runs the whole matrix and produces the summary.
- [ ] Results are reproducible within ±10% across three runs on the same machine.
- [ ] Summary committed for at least one Linux and one macOS machine.

## Phase 5 — Virtual filesystem: read path (Linux)

Status: not started

- [ ] `graft new` on the large fixture completes in < 1 s regardless of repo size (*target*).
- [ ] `ls -R`, `cat`, `grep -r`, `rg` and `cargo check` / `tsc --noEmit` produce identical results in a graft and in Root.
- [ ] Daemon restart remounts existing grafts.
- [ ] Unmount on prune works even if a process holds files open (lazy unmount with a warning).

## Phase 6 — Delta store and op log

Status: not started

- [ ] Full backend conformance suite passes, including concurrent writers.
- [ ] Property test: replaying the op log from empty reproduces the delta exactly.
- [ ] `git status` inside a graft matches `graft status`.
- [ ] Building and running the test suite of the medium fixture inside a graft succeeds.
- [ ] A crash (kill -9 of graftd) mid-write loses at most the in-flight operation; the delta is consistent on restart.

## Phase 7 — macOS virtual backend

Status: not started

- [ ] No kernel extension or system extension approval required.
- [ ] Conformance suite passes on macOS.
- [ ] Medium fixture builds and tests inside a graft on macOS.
- [ ] Benchmarks recorded against the reflink backend on the same machine.

## Phase 8 — Bud and Ring

Status: not started

- [ ] Bud of a graft with 1,000 changed files completes in < 200 ms (*target*).
- [ ] Rewinding then re-applying produces an identical delta (test).
- [ ] Pruning a graft does not remove blobs used by its buds or rings.

## Phase 9 — Sandboxed run

Status: not started

- [ ] A test process inside `graft run` fails to write to Root's working tree, another graft, and `$HOME` (outside allowed paths).
- [ ] Common agents and toolchains run normally inside the sandbox (test with a fixture script that runs git, cargo/npm and a network call if allowed).
- [ ] Clear error messages when the kernel lacks Landlock support.

## Phase 10 — Blueprints

Status: not started

- [ ] Two grafts of the web fixture run their dev servers and test suites at the same time with no port or database collision.
- [ ] Prune removes the graft's database, volumes and port reservations.
- [ ] Template rendering errors are reported with file and line.

## Phase 11 — Radar

Status: not started

- [ ] Two grafts editing the same function are reported at symbol level within 5 s of the second write (*target*).
- [ ] Edits to different functions in the same file are reported at path level only.
- [ ] A seeded behavioural conflict (both pass alone, fail together) is detected by speculative merge.
- [ ] Radar adds < 5% overhead to write latency (*target*).

## Phase 12 — Integrate

Status: not started

- [ ] Non-overlapping grafts integrate with zero manual steps.
- [ ] A reordering conflict that line-based git merge fails is resolved by the structural merge step (fixture).
- [ ] A failing test gate returns the graft to `Live` with the failure output attached.
- [ ] Root never receives a commit that failed the gate (test).
- [ ] Output is plain git history: `git log` and `git fsck` are clean.

## Phase 13 — Scheduler

Status: not started

- [ ] On a fixture task touching three modules, the plan places tightly coupled files in the same node.
- [ ] Hub-file changes are always in the first node.
- [ ] Admission rejects a write to a symbol claimed by another live graft and names that graft.
- [ ] Plans are deterministic for the same input.

## Phase 14 — MCP server

Status: not started

- [ ] An MCP client test harness can run the full cycle: plan → new → run → radar → integrate → prune.
- [ ] Tool schemas are documented and covered by snapshot tests.
- [ ] Destructive tools refuse without confirmation.

## Phase 15 — Materialize, hardening and first release

Status: not started

- [ ] Materialize round-trips: materialize → edit → re-import produces the expected delta.
- [ ] 24 h of fuzzing with no crashes.
- [ ] README states measured results, not targets.
- [ ] Release workflow produces signed binaries for both platforms.
