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

Status: done (#2)

- [x] `graft init` is idempotent and refuses non-git directories with a clear error.
  - `root::init_registers_then_is_idempotent`, `root::init_refuses_a_directory_that_is_not_a_repository`; also refuses bare and empty repositories.
- [x] Schema migrations are tested from an empty DB.
  - `graft-store/tests/migrations.rs`: empty directory, empty file, re-open, newer-version refusal, rollback of a failed migration, hash pin on released migrations.
- [x] Concurrent CLI invocations do not corrupt the DB (test with parallel processes).
  - `root::parallel_inits_register_each_root_once_without_corruption` (24 `graft init` processes over 2 repos, then mixed `init`/`root`), and `graft-store/tests/concurrency.rs` (8 processes writing 201 grafts and racing for one name). `integrity_check` is clean; 0 failures in 30 repeated local runs.
- [x] Graft names are validated (lowercase, digits, hyphens, ≤ 40 chars) and unique per Root.
  - `GraftName` (also: must start with a letter or digit and not end with a hyphen); a partial unique index among unpruned grafts; `names_are_unique_among_unpruned_grafts`.

## Phase 2 — Reflink grafts

Status: in review (`feat/reflink-grafts`)

- [ ] On APFS and btrfs, creating a graft of a 1 GB working tree uses < 5 MB of new disk (*target*), measured by `graft ls`.
  - 1 GiB in 10,000 files (`scripts/measure-graft-disk.sh`, `bench` workflow): APFS **804 KiB**, btrfs **808 KiB**, XFS 1.2 MiB. Met.
  - 1 GiB in 100,000 files, APFS (local): **7.7 MiB**. Missed. Nearly all of it is the graft's git index (8.0 MB, ~80 bytes per entry), the same size as Root's own index; gix cannot write the smaller index v4. Left unticked because it holds only for trees with fewer, larger files.
- [x] Two grafts from the same Root can be created, edited and pruned independently.
  - `grafts::two_grafts_are_independent` (e2e), conformance `independent_grafts`.
- [x] A graft never shares a git branch lock with Root or another graft.
  - `grafts::grafts_do_not_take_branch_locks`: Root and two grafts all have `main` checked out and commit on it; Root's branches and worktree list are untouched.
- [x] Prune leaves no files, mounts or DB rows behind (verified by test).
  - `grafts::prune_leaves_nothing_behind`: no working tree, empty mounts dir, no rows, no `refs/graft/*`.
- [x] Copy fallback works on ext4 and prints a single clear warning.
  - `grafts::warns_exactly_once_when_copying_instead_of_reflinking` passes on the Ubuntu runner (ext4); the `bench` run on an ext4 loop mount printed exactly one warning and copied the 1 GiB tree correctly (`graft ls`: 1.0 GiB). `warns_once_when_graft_home_is_on_another_filesystem` covers the cross-filesystem case.

## Phase 3 — Layer cache

Status: in review (`feat/layer-cache`)

- [x] With a Node fixture, the second and later `graft new` calls do not run the install command.
  - `layers::node_fixture_reuses_node_modules_and_passes_npm_test_quickly` (real `npm ci`, required in CI) and `layers::builds_once_and_reuses_the_layer` (exact build count).
- [x] Changing the lockfile produces a new layer; the old one is kept while referenced.
  - `layers::a_lockfile_change_makes_a_new_layer_and_keeps_the_old`.
- [x] `graft gc` never removes a layer referenced by a live graft (tested).
  - `layers::gc_never_removes_a_referenced_layer`, `layers::gc_racing_graft_new_never_strands_a_graft` (6 `graft new` and 6 `graft gc` processes at once; 0 failures in 15 local runs), store and foreign-key tests.
- [x] Time from `graft new` to first passing `npm test` on the fixture is < 3 s after the first graft (*target*).
  - 0.13–0.16 s on Apple Silicon (APFS), measured by the Node e2e test.

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
