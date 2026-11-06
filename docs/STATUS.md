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

Status: done (#3, #4)

- [ ] On APFS and btrfs, creating a graft of a 1 GB working tree uses < 5 MB of new disk (*target*), measured by `graft ls`.
  - 1 GiB in 10,000 files (`scripts/measure-graft-disk.sh`, `bench` workflow): APFS **804 KiB**, btrfs **808 KiB**, XFS 1.2 MiB. Met.
  - 1 GiB in 100,000 files, APFS (local): **7.7 MiB**. Missed. Nearly all of it is the graft's git index (8.0 MB, ~80 bytes per entry), the same size as Root's own index; gix cannot write the smaller index v4. Left unticked because it holds only for trees with fewer, larger files.
- [x] Two grafts from the same Root can be created, edited and pruned independently.
  - `grafts::two_grafts_are_independent` (e2e), conformance `independent_grafts`.
- [x] A graft never shares a git branch lock with Root or another graft.
  - `grafts::grafts_do_not_take_branch_locks`: Root and two grafts all have `main` checked out and commit on it; Root's branches and worktree list are untouched.
- [x] Prune leaves no files, mounts or DB rows behind (verified by test).
  - `grafts::prune_leaves_nothing_behind`: no working tree, empty mounts dir, no rows, no `refs/graft/*`.
  - Removal retries a bounded number of times when a pass leaves a directory not empty (seen on the macOS runner with the 200k-file large fixture), and otherwise names what is left: `backend::tests::*`, `reflink_backend::prunes_a_graft_with_many_files_and_nothing_else` (20,000 files, a read-only directory, a symlink to Root).
- [x] Copy fallback works on ext4 and prints a single clear warning.
  - `grafts::warns_exactly_once_when_copying_instead_of_reflinking` passes on the Ubuntu runner (ext4); the `bench` run on an ext4 loop mount printed exactly one warning and copied the 1 GiB tree correctly (`graft ls`: 1.0 GiB). `warns_once_when_graft_home_is_on_another_filesystem` covers the cross-filesystem case.

## Phase 3 — Layer cache

Status: done (#5)

- [x] With a Node fixture, the second and later `graft new` calls do not run the install command.
  - `layers::node_fixture_reuses_node_modules_and_passes_npm_test_quickly` (real `npm ci`, required in CI) and `layers::builds_once_and_reuses_the_layer` (exact build count).
- [x] Changing the lockfile produces a new layer; the old one is kept while referenced.
  - `layers::a_lockfile_change_makes_a_new_layer_and_keeps_the_old`.
- [x] `graft gc` never removes a layer referenced by a live graft (tested).
  - `layers::gc_never_removes_a_referenced_layer`, `layers::gc_racing_graft_new_never_strands_a_graft` (6 `graft new` and 6 `graft gc` processes at once; 0 failures in 15 local runs), store and foreign-key tests.
- [x] Time from `graft new` to first passing `npm test` on the fixture is < 3 s after the first graft (*target*).
  - 0.13–0.16 s on Apple Silicon (APFS), measured by the Node e2e test.

## Phase 4 — Benchmark harness

Status: in review (`test/worktree-benchmarks`)

- [x] One command runs the whole matrix and produces the summary.
  - `bench/run.sh`.
- [x] Results are reproducible within ±10% across three runs on the same machine.
  - Linux (GitHub runner, XFS): 159 of 160 values within ±10%. The macOS runner (a VM) was not: 17 of 113; see `bench/results/macos-arm64-apfs/NOTE.md` at 62afd44 (those results are superseded by the Phase 8 baseline and kept in git history).
- [ ] Summary committed for at least one Linux and one macOS machine.
  - Linux: full matrix committed. macOS: small and medium only (large did not finish on the runner), with timings that are not reproducible. The Phase 8 baseline adds a macOS large run (1 and 4 agents), but its reflink cells errored in `graft prune`; see Phase 8.

## Phase 5 — Virtual filesystem: read path (Linux)

Status: done (PR #7)

- [x] `graft new` on the large fixture completes in < 1 s regardless of repo size (*target*).
  - Bench (GitHub ubuntu runner, XFS, 3 runs): median create 47 ms on large (≈200k files), 22–86 ms on medium, 22–38 ms on small, with the fixture's `node_modules` layer already cached.
  - Not met when the layer is not cached yet: the first graft of each cell took 18.1 s on large, because it builds the layer in a throwaway reflink checkout (≈11 s) and then removes that checkout (≈7 s). That is layer-build cost, not mount cost; fixing it is in scope for Phase 8 (performance).
- [x] `ls -R`, `cat`, `grep -r`, `rg` and `cargo check` / `tsc --noEmit` produce identical results in a graft and in Root.
  - `tools_see_the_same_tree_as_root` and `type_checkers_agree_with_root`, in the CI `fuse` job (rg and tsc required).
- [x] Daemon restart remounts existing grafts.
  - `a_restarted_daemon_remounts_grafts` (kill -9 graftd), in the CI `fuse` job.
- [x] Unmount on prune works even if a process holds files open (lazy unmount with a warning).
  - `unmounts_lazily_when_a_file_is_open`, `prune_unmounts_lazily_when_busy`, in the CI `fuse` job.

## Phase 6 — Delta store and op log

Status: done (PR #9)

- [x] Full backend conformance suite passes, including concurrent writers.
  - `passes_the_conformance_suite` in `crates/graft-fs/tests/fuse_backend.rs` (CI `fuse` job). The same operations pass through the view API on every platform (`crates/graft-fs/tests/view_writes.rs`).
- [x] Property test: replaying the op log from empty reproduces the delta exactly.
  - `crates/graft-store/tests/delta_props.rs`: replay equals the stored delta, and the overlay equals a plain filesystem model (300 cases each). Breaking tombstone handling on purpose makes it fail.
- [x] `git status` inside a graft matches `graft status`.
  - `git_status_matches_graft_status` (CI `fuse` job): adds, edits, renames, a chmod, a symlink, nested directories and ignored files.
- [x] Building and running the test suite of the medium fixture inside a graft succeeds.
  - `scripts/virtual-medium-fixture.sh` (CI `fuse` job): `npm test` passes in 221 ms inside the graft; after an edit inside it, `graft status` and `git status` agree. `graft new` took 4.5 s on that run because the `node_modules` layer was built cold (see Phase 8).
- [x] A crash (kill -9 of graftd) mid-write loses at most the in-flight operation; the delta is consistent on restart.
  - `a_crash_mid_write_loses_at_most_the_operation_in_flight` (CI `fuse` job: a completed write, a mkdir and a file still open when graftd is killed all survive); `a_crash_loses_nothing_written_before_it` through the view API.

Not measured: write throughput and in-graft build time against Root (the benchmark harness has no in-graft workloads; Phase 8 adds them).

## Phase 7 — macOS virtual backend

Status: done (#10, #11; benchmarks recorded in `docs/record-bench-baselines`)

- [x] No kernel extension or system extension approval required.
  - NFSv3 loopback served by `graftd` (nfsserve) and mounted with the built-in `mount_nfs`, as an ordinary user: no `sudo`, no extension, no approval (ADR 0009). Verified on macOS 27; the CI `nfs` job runs as the runner's user.
- [x] Conformance suite passes on macOS.
  - `passes_the_conformance_suite` in `crates/graft-fs/tests/nfs_backend.rs`, plus `git status` parity, `git commit`, layers, busy unmount and AppleDouble tests; the virtual graft e2e suite (including `kill -9` of graftd) also runs on NFS. Passed locally on macOS 27; CI `nfs` job.
- [x] Medium fixture builds and tests inside a graft on macOS.
  - `scripts/virtual-medium-fixture.sh` locally on macOS 27: `npm test` passes in the graft (3.1 s), again after an edit, and `graft status` and `git status` agree. Cold `graft new` took 28 s, nearly all of it building the `node_modules` layer (Phase 8).
- [x] Benchmarks recorded against the reflink backend on the same machine.
  - `bench/results/nfs-backend/` (Actions run 41999454838, GitHub `macos-latest` runner, APFS; timings not reproducible on that VM, disk numbers are). `graft new` on NFS is metadata only after the first graft: ≈50–60 ms against 3.7–4.0 s for reflink on medium, and 63 ms on large (4 agents) where a worktree takes 32 s. Disk added for 16 medium grafts: 44.9 MiB (NFS), 209.4 MiB (reflink), 2.9 GiB (worktrees). Prune on medium: ≈30 ms (NFS) against 1.6–2.7 s (reflink). The first NFS graft is slower than the first reflink graft (6.4 s against 5.5 s on medium, 60 s against 30 s on large), nearly all of it building the `node_modules` layer. 4 of 6 large reflink runs failed in `graft prune` (`Directory not empty`), fixed in `fix/prune-dir-not-empty`.

Known limits (ADR 0009): `fsync` on an NFS graft is not durable against power loss until the file is journaled; other local users could reach a graft's NFS port; no xattrs (AppleDouble files are refused); new directories are `0755`.

## Phase 8 — Performance: faster than worktrees

Status: in progress, as stacked PRs. Done: in-graft workloads and the worktree gate (#12, ADR 0010). Profiling: `GRAFT_LOG=graft::timing=debug graft new` logs each step, `scripts/profile-graft-new.sh` compares them with `git worktree add` and `npm ci`, and the `Profile` workflow runs it on the bench runners (`test/profile-graft-new`).

Baseline before any optimisation (`bench/results/`; first recorded from Actions run 42005356393, now replaced by run 42113580319 on `main` with the prune fix; `gate.md` in each directory). Every gate fails today. The figures below are from 42005356393; with 42113580319 the Linux XFS gate is 31 beats, 12 ties, 14 losses, and the macOS large reflink cells have numbers (5 of 6 runs; one prune still failed after the retries). Profile of the first graft: `bench/results/profiles/` (cloning the freshly built layer is the cold loss on XFS). Done: parallel layer cloning (#16, ADR 0011): a warm graft now creates faster than a worktree on XFS and ext4; the cold graft on XFS still loses by about 10% (cloning a just-built layer). Tried and rejected: flushing the layer first, or cloning it into the cache instead (#17, see the profile notes).

- Linux, XFS, create matrix (188 of 189 values within ±10%): the default `graft` method beats worktrees on 40 values, ties 3 and loses 14. It wins on disk (≈10× less) and on ready time for later agents (213 ms against 535 ms on small). It loses on create for the first graft (398 ms against 71 ms on small, 3.74 s against 1.21 s on medium), on create for every medium graft (≈1.5 s against ≈1.2 s), on ready for the first agent on medium (≈3.9 s against ≈2.65 s, the cold layer), and on small-fixture prune (≈41 ms against ≈33 ms). `graft-fuse` loses 14, mostly the cold first agent (large: 16.5 s against 12.8 s to create).
- In-graft workloads, medium (XFS 16 of 19, ext4 18 of 20 within ±10%): the reflink `graft` ties worktrees on builds, tests, `ls -R` and writes, and loses on `git status` (30 ms against 24 ms on XFS, 42 ms against 36 ms on ext4) and, by 1–8%, on `git diff`, `git commit`, `rg` and (XFS only) `find`. `graft-fuse` loses all 11 workloads: `rg` 20–24× slower, `write-10k-small` 27–39×, `ls -R` ≈8×, cold build 6.5–7×.
- macOS, APFS (runner VM, not reproducible: 35 of 138 matrix and 1 of 24 workload values within ±10%): on small and medium the reflink `graft` beats worktrees on 19 values, ties 18 and loses 1; `graft-nfs` beats 28, ties 8 and loses 2. On large every reflink cell errored in `graft prune` (`Directory not empty`), so the default backend has no large numbers yet (fixed in `fix/prune-dir-not-empty`); `graft-nfs` creates in 57 ms against 38 s and prunes in 34 ms against 17 s, but its first graft takes 70 s against 39 s. Inside a graft, NFS is 2–5× slower on `git` and `find`, 44× on a 1 GiB write, and its `write-10k-small` cleanup errored (`Directory not empty`).

- [ ] On the default backend, a graft beats the worktree baseline on create time, time to first passing test (cold layer cache included), disk added and prune time, for every fixture and agent count, on Linux (ext4, and XFS or btrfs) and macOS (APFS).
- [ ] On the default backend, every in-graft workload (build, rebuild, test, git, search, walk, writes) is at least as fast as the worktree baseline, and beats it wherever the backend can, on Linux and macOS. (Relaxed from "beats" with the owner's approval: a real-directory backend can match a worktree inside, not reliably beat it; ADR 0010.)
- [ ] No backend is slower than the worktree baseline on any metric; on ext4 the reflink backend at least matches it.
- [ ] Conformance suite, op log replay property test, crash-consistency test and e2e suite pass on every backend with every optimisation enabled; existing tests are only added to.
- [ ] Every caching mechanism is tested for immediate visibility within a graft and isolation from Root and other grafts.
- [ ] `fsync` and crash guarantees are unchanged from Phase 6 (`kill -9` of `graftd` mid-write, optimisations enabled).
- [ ] Every fallback path is tested, and `graft doctor` reports which mechanisms are active.
- [ ] Peak `graftd` memory stays within the configured cache limits on the large fixture with 16 grafts.
- [ ] Results are reproducible within ±10% over three runs on a machine that can reproduce them.

## Phase 9 — Bud and Ring

Status: not started

- [ ] Bud of a graft with 1,000 changed files completes in < 200 ms (*target*).
- [ ] Rewinding then re-applying produces an identical delta (test).
- [ ] Pruning a graft does not remove blobs used by its buds or rings.

## Phase 10 — Sandboxed run

Status: not started

- [ ] A test process inside `graft run` fails to write to Root's working tree, another graft, and `$HOME` (outside allowed paths).
- [ ] Common agents and toolchains run normally inside the sandbox (test with a fixture script that runs git, cargo/npm and a network call if allowed).
- [ ] Clear error messages when the kernel lacks Landlock support.

## Phase 11 — Blueprints

Status: not started

- [ ] Two grafts of the web fixture run their dev servers and test suites at the same time with no port or database collision.
- [ ] Prune removes the graft's database, volumes and port reservations.
- [ ] Template rendering errors are reported with file and line.

## Phase 12 — Radar

Status: not started

- [ ] Two grafts editing the same function are reported at symbol level within 5 s of the second write (*target*).
- [ ] Edits to different functions in the same file are reported at path level only.
- [ ] A seeded behavioural conflict (both pass alone, fail together) is detected by speculative merge.
- [ ] Radar adds < 5% overhead to write latency (*target*).

## Phase 13 — Integrate

Status: not started

- [ ] Non-overlapping grafts integrate with zero manual steps.
- [ ] A reordering conflict that line-based git merge fails is resolved by the structural merge step (fixture).
- [ ] A failing test gate returns the graft to `Live` with the failure output attached.
- [ ] Root never receives a commit that failed the gate (test).
- [ ] Output is plain git history: `git log` and `git fsck` are clean.

## Phase 14 — Scheduler

Status: not started

- [ ] On a fixture task touching three modules, the plan places tightly coupled files in the same node.
- [ ] Hub-file changes are always in the first node.
- [ ] Admission rejects a write to a symbol claimed by another live graft and names that graft.
- [ ] Plans are deterministic for the same input.

## Phase 15 — MCP server

Status: not started

- [ ] An MCP client test harness can run the full cycle: plan → new → run → radar → integrate → prune.
- [ ] Tool schemas are documented and covered by snapshot tests.
- [ ] Destructive tools refuse without confirmation.

## Phase 16 — Materialize, hardening and first release

Status: not started

- [ ] Materialize round-trips: materialize → edit → re-import produces the expected delta.
- [ ] 24 h of fuzzing with no crashes.
- [ ] README states measured results, not targets.
- [ ] Release workflow produces signed binaries for both platforms.
