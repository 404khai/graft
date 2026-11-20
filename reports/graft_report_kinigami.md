# Using graft 0.2.0 on Kinigami: field report

# Take 1 — one graft, CLI, landing with `integrate`

Author: Claude Opus 5.5, acting as engineering lead on Kinigami. Date: 2026-11-19. Machine: Apple M-series, macOS, APFS, with the disk about 99% full during the session.

Kinigami is a deterministic Zig physics simulation with unusual provenance rules:
- every result embeds the git revision and a working-tree-dirty flag;
- validation criteria must be committed and hashed **before** the code they govern;
- history is never rewritten.

That makes it a strict test of whether a workspace tool keeps git honest.

This report covers what I **ran**, not only what the README says. I used graft for one real task: adding the gyroscopic term to the integrator and correcting the body-contact criteria, v2 then v3. That task took 7 commits and was landed with `graft integrate`. Separately, I tried the NFS backend, the sandbox and layers.

## Numbers I measured

| Action | Result |
| --- | --- |
| `graft init` | 0.04 s |
| `graft new` (reflink, 201 tracked files) | **0.06–0.07 s**, 44 KiB unshared |
| `graft new --backend nfs` | 0.97 s, 44 KiB |
| First `tools/zig build test` in a fresh graft | 2 min 41 s (includes a 474 MB toolchain download and a cold `.zig-cache`); all tests pass |
| Graft disk after build plus a ReleaseSafe build | 538 MiB → 802 MiB, of which 474 MB is the re-downloaded pinned toolchain |
| `graft integrate` (no test gate) | 1.06 s |
| Provenance inside a graft | `code_revision b098e80`, `working_tree_dirty false`: **correct** |
| Sandbox (`graft run`) | a write inside the graft succeeded; a write into Root failed with "Operation not permitted" |

## What I love

- **Creation is effectively free.** At 60 ms and 44 KiB, spinning up a workspace for an experiment costs nothing. That changes behaviour: I would fork a graft to try "add the gyroscopic term" without the hesitation a worktree's full checkout brings.
- **It is still plain git inside.** `git log`, `git status` and `git rev-parse` all behave normally. Kinigami's build embeds `git rev-parse` and `git status` output, and the graft produced correct, clean provenance on the first try. This was my biggest worry, and it was unfounded.
- **The refusals are excellent.** `graft integrate` refused to land on a branch checked out in Root ("its working tree would fall behind") and named both fixes (`--onto`, `--update-checkout`). It also said plainly "no test gate ran". This tool tells you what it did *not* do, which is exactly what a strict project wants.
- **The sandbox is real.** `graft run` blocked a write to the main checkout with a kernel-level denial. For delegating to sub-agents, "it physically cannot touch Root" is a much stronger guarantee than an instruction in a prompt.
- **`graft ls`, `status` and `radar` are cheap situational awareness.** Changed-file counts and unshared disk at a glance; Radar's "No overlaps between live grafts" was instant.
- **`doctor` is honest and actionable.** For example, it flagged the space in the legacy `~/Library/Application Support/graft` path.

## Compared with git worktrees (and the agent-isolation worktrees I normally use)

| | git worktree | graft (reflink) |
| --- | --- | --- |
| Create | full checkout; fast here, slower on big repos | 60 ms, 44 KiB |
| Ignored build artefacts (`.toolchain/`, `.zig-cache`) | not copied; rebuilt per worktree | not copied either, unless declared as a layer |
| Git semantics | native; shows in `git worktree list` and editors | private repo by default; opt-in worktree registration |
| Landing | you merge or cherry-pick yourself, so history is exactly what you made | `integrate` **squashes** to one commit (see pain points) |
| Isolation | none, so an agent can write anywhere | sandboxed `graft run` |
| Overlap detection | none | Radar |
| Checkpoints | stash or commit | `ring` / `rewind` |

**Verdict for Kinigami:** graft wins on creation cost, isolation and awareness. Worktrees win on landing fidelity today. For a repo whose rules depend on the exact sequence of commits, landing fidelity decides.

## Pain points

1. **`graft integrate` squashed seven commits into one, under the first commit's message.** This was the serious one.
   - My graft held seven ordered commits: criteria v2 → register → solver → evaluator → criteria v3 → register → evaluator.
   - `integrate` produced a single commit, `8d37fb3`, titled *"test(criteria): body-contact criteria v2 and Q79 decision"*, which also contained the solver changes.
   - Kinigami's preregistration rule needs the criteria commits to precede the code commits, and its criteria register cites those commit hashes. After the squash they were unreachable from Root.
   - I recovered without rewriting history: I confirmed the trees matched, then made a content-neutral merge of the graft's original commits. A user who didn't notice would have lost auditable history and kept a misleading message.
   - **Ask:** preserve commits by default (fast-forward or merge) with squash as an option, or at minimum write a combined message listing every squashed commit and say "squashed N commits" in the output.
2. **Layers need `graft.toml` committed at HEAD.** I wanted the 474 MB pinned toolchain as a layer. `graft trust` answered "No graft.toml at HEAD; nothing to approve", and an untracked `graft.toml` silently didn't apply. In Kinigami an untracked file also flips the results' dirty flag. So the choice is: commit a non-project tooling file, or re-download 474 MB per graft.
   - **Ask:** support a per-Root, out-of-tree config, for example `$GRAFT_HOME/roots/<id>/graft.toml`, or a `--config` flag.
3. **Ignored-but-essential directories aren't carried over.** Without layers, every graft starts without `.toolchain/` and pays the download over the network, on a nearly full disk.
   - **Ask:** an opt-in "reflink these ignored paths from Root at create time" list, which would cost nothing on APFS.
4. **The legacy home path has a space** (`~/Library/Application Support/graft`, kept from before 0.2). Quoting everywhere is fragile for agents.
   - **Ask:** offer a one-command migration to `~/.graft`.
5. **No test gate means a silent single commit.** The "no test gate ran" note is good, but for a repo that already has a canonical `tools/zig build test`, a one-time prompt to set `[integrate] test` would help.

## Where graft fits in my workflow

- **Now:** experiments and risky solver changes (exactly the gyroscopic-term change), and sandboxed sub-agents via `graft run`. I'd land by **merging the graft's branch myself**, not with `integrate`, until commits are preserved.
- **Once layers work without a committed `graft.toml`:** a candidate study per graft, for example cand-a v6 next to a cand-b revision, with Radar watching overlap in `docs/decisions/README.md` and `criteria/REGISTER.md`. Those two files are the merge hot spots every Kinigami change touches.
- **Future:** `graft plan` for splitting the Kin subsystem phase (sensors, comms, actuators) across agents. Also `bud` and `ring` around preregistered evaluations, so a run can be reproduced from an exact checkpoint.

## On graft-workspace.vercel.app

- Fetching the site with a non-JavaScript reader returned **only the title** ("Copy-on-Write Workspaces for Agents"). The page appears to be client-side rendered. Agents, search engines and link previews will see nothing.
  - **Ask:** server-render or pre-render the core content: what it is, install, quickstart, the comparison with worktrees, and the benchmark table. Also add an `llms.txt`.
- Things I'd like to see on it:
  - the honest benchmark table from the README, including where grafts lose;
  - a "landing semantics" section stating exactly what `integrate` does to history;
  - a provenance note ("`git rev-parse` and `git status` behave normally inside a graft");
  - a layer example for pinned toolchains;
  - the MCP setup snippet.

## Not tested

I did not exercise `plan`, `claim`, Radar with a `[radar] test`, blueprints (ports, Postgres), `bud`, `ring`, `rewind` or the MCP server in this session, so I have no verdict on them.


---

# Take 2 — two parallel workstreams, driven over MCP

Date: 2026-11-19, the same day.

Two independent tasks ran at once, each in its own graft and handled by a sub-agent:
- **A:** a preregistered coupler capture-boundary vs timestep study, with new Zig code;
- **B:** cand-a v6, a robot candidate revision.

I drove graft through its **MCP server**. My session couldn't attach an MCP server mid-run, so I wrote a 40-line stdio JSON-RPC client and called the tools directly. Tools used:

> `graft_doctor`, `graft_plan`, `graft_new` (with `clone_ignored`), `graft_list`, `graft_ring`, `graft_status`, `graft_radar` and the `graft://radar` resource, `graft_integrate` (dry run), `graft_prune`, `graft_fsck`, `graft_stats`.

Plus the CLI for `graft claim` and `graft claims`.

## What worked well

- **The MCP server is clean and predictable.** `tools/list` returned all 18 tools. Results are JSON, and errors carry a code and a precise message. I passed `ring` where the parameter is `label`, and the reply was "graft_ring takes no argument ring", which is exactly right.
- **`clone_ignored` solved Take 1's biggest pain point.** `graft_new {"clone_ignored": [".toolchain"]}` reflinked the 474 MB pinned toolchain into each graft. That took 11–21 s on a nearly full disk, adding **45 KiB** each. There was no network download and no `graft.toml` to commit. This is the feature I'd put on the website's front page.
- **Claims are simple and effective.**
  - I claimed the three files of workstream A for its graft, then tried to claim one of them for B.
  - The reply was "rejected: src/qualify/main.zig is claimed by graft dt-capture", and the refusal was logged under "Refused writes" in `graft claims`.
  - That is the right primitive for keeping two agents out of each other's way.
- **Rings on reflink grafts work over MCP.** A "start" ring per graft before the agents began returned the commit and time.
- **`graft_integrate` with `dry_run: true` is a good pre-flight.** It reports "no test gate ([integrate] test is not set)" and gives a radar summary without touching history.
- **Prune knew the work was safe.** After I fetched the graft's commits onto a Root branch myself, `graft_prune` (without `force`) removed the graft, recognising the work was in Root.
- **`fsck` (ok, no findings) and `stats` (per-op latency of graftd)** are good health checks for an agent to run unattended.
- **Provenance still held.** Results produced in Root from the landed branch carried the right revision and clean flag. The official run matched the graft preview byte for byte (by report JSON).

## Pain points

1. **`graft_plan` placed nothing for this repo.**
   - The task named seven existing paths in code spans: Zig sources, JSON and Markdown. The plan came back with `width 0` and all seven `unresolved`.
   - The dependency analysis covers Rust, TypeScript, Python and Go only, so a Zig repo (or any repo whose task touches docs and data) gets no plan and no hint why.
   - **Ask:** fall back to file-level nodes for unsupported languages, and say "no parser for .zig; placed as whole files" rather than "unresolved".
2. **Landing still squashes.** `graft_integrate` documents "one commit each". For a preregistration repo I landed by `git fetch <graft path> HEAD:<branch>` instead, which kept the two commits (`Q81 preregistration` then the code) in order.
   - **Ask (repeat):** a `preserve_commits` option, or merge-commit landing.
3. **MCP tools can't be hot-added to a running agent session.** That's a client limitation, not graft's. It's worth a line in docs/mcp.md: "if your client is already running, `graft mcp` speaks plain JSON-RPC over stdio", with a minimal client snippet.
4. **Sub-agents don't run inside the sandbox by default.** I pointed sub-agents at graft paths by instruction. One lost its `cd` in a backgrounded command and built in Root. It touched only ignored build output, but `graft run` confinement would have prevented it.
   - **Ask:** document a pattern for in-process sub-agents. For example, an MCP `graft_run` that keeps a long-lived confined shell an agent can send commands to.
5. **Radar is file-granular for unsupported languages.** It showed no overlaps, which was correct here because one workstream stopped early. I couldn't test symbol-level overlap on Zig.

## Where graft now fits in my workflow

- **Default for parallel agent work on this repo:** one graft per workstream, made with `clone_ignored` for the toolchain. Claims go on each workstream's exclusive files, a ring goes on before each agent starts, and radar and an integrate dry run come before landing.
- **Landing:** fetch the graft branch and merge it myself, until commits can be preserved.
- **Next to try:** `bud` for exploring two design options from one checkpoint, and `graft run` for confining sub-agents. Real symbol-level radar needs a Zig parser.

## Not tested in Take 2

Blueprints, `bud`, `rewind`, `[radar] test`, and the NFS backend under MCP.

---

# Take 3: website work, a graft vs worktree benchmark, and two sessions in one checkout

Date: 2026-11-20. Author: Claude Opus 5.5, in the session that built Kinigami's project website (`web/`, PR #20). Same machine as Takes 1 and 2: Apple M-series, macOS, APFS, disk at 97% (17 GiB free).

## Why this take exists

I did the website work in a **git worktree** (`../kinigami-website`), the habit graft is meant to replace. It went wrong in exactly the way graft prevents:
- Another session (the cand-d operator) shares the Root checkout.
- Mid-task, it switched Root to `feat/cand-d-coupler-first` and committed Q85. My work was on `feat/website`, but before I moved to a worktree my untracked `web/` folder had sat in Root.
- So it went along with the branch switch. Anyone's next `git add -A` would have swept the whole website into a cand-d commit.
- I caught it, moved `web/` into a worktree, and later found a leftover `web/.vite/` cache in Root that I had to delete.

Nothing was lost, but only because I happened to look. The owner then asked me to switch to graft, benchmark it against the worktree I had used, delete the worktree, and move the operator session into a second graft.

## What I ran

- Removed the worktree (pushed, clean apart from a `.DS_Store`).
- Benchmarked `git worktree add` against `graft new`, 3 runs each, on the same revision (`feat/website` a70234c, 255 tracked files):
  - plain create;
  - create while carrying the 470 MB pinned Zig toolchain (`.toolchain/`, git-ignored);
  - teardown;
  - a real workload: `npm ci` and `npm run build` of the website.
- Created graft 1, `website`, from `feat/website`, then installed and built the site in it.
- Sent the operator session steps to move into graft 2, `cand-d`, made with `--clone-ignored .toolchain`.

## Numbers

| Action | git worktree | graft 0.2.0 (reflink) |
| --- | --- | --- |
| Create, plain (3 runs) | 0.145 / 0.070 / 0.068 s | **0.074 / 0.043 / 0.042 s** |
| Create + carry `.toolchain` (470 MB) | 2.86 / 2.79 / 2.48 s (`worktree add` + `cp -cR`, an APFS clone) | **1.12 / 1.44 / 1.67 s** (`--clone-ignored .toolchain`) |
| Size after plain create | 13 MB (`du`, real files) | **1.3 MiB** unshared (`graft ls`) |
| Remove, plain | 0.04–0.05 s | 0.02 s |
| Remove, with toolchain | 0.93–1.13 s | **0.02–0.03 s** (checked: the mount dir is gone and free disk didn't drop) |
| `npm ci` (warm npm cache) | 1.35 s | 0.79 s |
| `npm run build` (generate + tsc + vite) | 3.13 s | 2.14 s |
| Git inside | normal, branch `feat/website` | normal, branch `graft/website`, `origin` set, clean status |

**Caveats:**
- The `npm` rows ran worktree-first, so the graft had a warmer cache. I count them as a tie, not a graft win.
- `du` reports logical size. A worktree's 13 MB is real blocks, but my `cp -c` toolchain copy shares blocks, so its `du` (484 MB) overstates the physical cost. I didn't measure physical disk with `df` deltas; at 17 GiB free the noise was too large.

**Verdict:** for this 226-file repo, creation is fast either way, and the speed is not why graft matters here. What matters:
- carrying a 470 MB ignored toolchain in one flag, about twice as fast as cloning it by hand;
- teardown in 0.02 s instead of about 1 s;
- **not sharing a checkout with another agent**, which is the failure I actually hit.

## What I love

- **Isolation from other sessions is the real feature.** Graft's pitch is speed, but today's failure was two agents in one checkout, and a graft can't have its branch switched by someone else. That is the line I'd lead with for multi-session setups like this one.
- **A graft is a real git repo with `origin` already set.** From `graft/website` I can `git push origin HEAD:feat/website` and keep updating PR #20 directly. That avoids `integrate`'s squash (Takes 1–2) entirely for PR-based landing.
- **`--clone-ignored` works as a CLI flag too**, not only over MCP (Take 2). It is still the feature I'd put on the front page.
- **Prune is instant and honest.** `prune` refused a graft with work in it until I passed `--force`, and teardown of a 470 MB-carrying graft took 0.02 s.
- **`graft ls` DISK column.** The jump from 1.3 MiB to 79 MiB after `npm ci` told me exactly what installing dependencies cost, with no `du` needed.

## Pain points

1. **The space in the path broke me again.** Take 1 flagged `~/Library/Application Support/graft`. Today an unquoted `cd $G/web` in a benchmark failed with exit 1 and no useful message, so my first graft numbers were garbage until I re-ran with quotes. Agents write unquoted paths constantly.
   - **Ask (repeat, now with a real failure):** a one-command migration to `~/.graft`, or a space-free default for new installs.
2. **Grafts are hidden from humans.** The path is deep under `~/Library`, so the owner can't find my work in Finder or an editor the way they could find `../kinigami-website` next to the repo.
   - **Ask:** `graft new --at <dir>`, or a visible symlink farm such as `~/grafts/<root>/<name>`.
3. **`--clone-ignored` copies from whatever Root has checked out.** Root was on the other session's branch, which has no `web/`, so there was no `web/node_modules` to carry and I had to `npm ci` cold. In a shared Root, what a graft can inherit depends on someone else's checkout.
   - **Ask:** carry ignored directories from another graft (`--clone-ignored-from website`), or warn when a requested path doesn't exist in Root.
4. **Branch name mismatch.** A graft's branch is `graft/<name>`, so pushing to an existing feature branch needs `HEAD:feat/website` every time.
   - **Ask:** `graft new --from feat/website --track`, which would check out (or upstream) the feature branch itself.
5. **No way to tell other sessions which graft is theirs.** I had to explain graft 1 and graft 2 to the operator in prose.
   - **Ask:** an owner or session label on a graft, shown in `graft ls`. `claim` partly covers files, but not "this whole graft belongs to session X".

## Bench numbers I'd like to see

- **Scale:** create, prune and `git status` at 10k, 100k and 1M files. At 226 files the worktree is already fast, so graft's advantage shouldn't be judged on this repo.
- **Fan-out:** 16 grafts at once with `--clone-ignored .toolchain`, against 16 worktrees with `cp -c`. Wall time and physical disk (`df` delta, not `du`).
- **Physical disk accounting:** `graft ls` unshared vs real `df` deltas, so "1.3 MiB" can be trusted at 97% disk.
- **The NFS backend** on this repo: Take 1 measured 0.97 s to create. Is a Zig build inside it usable?
- **Cold vs warm:** `npm ci` and `zig build test` with cold caches in a graft vs a worktree, run in alternating order to remove the order bias in my table.

## State at the end of Take 3

- Worktree `../kinigami-website`: removed. Branch `feat/website` is pushed; PR #20 is open.
- Graft 1, `website`: base a70234c, site installed and built, clean.
- Graft 2, `cand-d`: requested from the operator session, which is to append its own experience as Take 4.
- No `graft integrate` was used. Landing stays on PRs, so commits are preserved.

## Take 3, part 2: driving graft over MCP, and pulling `main`

The owner asked me to drive graft over MCP and to pull `main` into my graft. PR #20 had merged by then, so `main` was `da86e87`. As in Take 2, I couldn't attach an MCP server to a running session, so I used a 30-line stdio JSON-RPC client. `tools/list` returned the same 18 tools.

| Call | Result |
| --- | --- |
| `graft_ring {name: website, label: before-main-pull}` | ok; returned a snapshot commit `bd19fa5`, not HEAD |
| `graft_run {command: [git, pull, --ff-only, origin, main]}` | ok in **980 ms**: fast-forward a70234c → da86e87, network allowed |
| `graft_run {command: [touch, <path outside graft>]}` | **refused**: `Operation not permitted`. The error JSON names the denied path and gives the fix (`write: [PATH]`, `dev_caches`, or `[sandbox] write`) |
| `graft_run {command: [touch, web/inside-test]}` | ok: writes inside the graft are allowed |

**What I love:**
- **`graft_run` closes Take 2's pain point 4.** Over MCP an agent can now run every command confined, with network allowed (`git pull` worked). The denial report is the best error I saw today: exact path, the reason, and three ways to allow it.
- **Pulling into a graft is plain git.** Nothing graft-specific was needed, and with a ring first the pull is reversible.

**Pain points:**
1. **`"sandbox": null` on success reads like "no sandbox".** On the failed run, the same field held `{denied: [...]}`. I nearly reported confinement as off.
   - **Ask:** always return `{"sandbox": {"confined": true, "denied": []}}`.
2. **The base goes stale after a pull.** After fast-forwarding to da86e87, `graft_status` and `graft ls` still show base `a70234c` with `changes: []`, although HEAD is two commits ahead. "Nothing changed since base" is then wrong in both directions.
   - **Ask:** track HEAD movement, or offer `graft rebase --to <ref>` / `graft rebase` semantics against a branch rather than only Root's head. Root here is on another session's branch, so "rebase onto Root's head" is the wrong target.
3. **The CLI needs a git cwd; MCP doesn't.** From a non-git directory, `graft path website` failed with "not a git repository". That made my cleanup step miss and leave a test file in the graft until I noticed.
   - **Ask:** accept `--root <path>` or `$GRAFT_ROOT` on every CLI command, as MCP effectively does.
4. **`graft_ring` returns a snapshot commit (`bd19fa5`) but doesn't say it isn't HEAD.** Fine once understood. One line, "ring commit (snapshot of HEAD a70234c + delta)", would help.

**State now:** graft `website` is at `main` da86e87, clean, with ring `before-main-pull` behind it. Graft `cand-d` (the operator's) is live at de5855c and using 44.5 MiB.

## Take 3, part 3: three grafts at once (website redesign + AGENTS.md)

Three live grafts on one Root:
- `website`: the site redesign;
- `cand-d`: the operator session's candidate work;
- `agents-grafts`: a one-line rule change, kept apart from the redesign so each lands as its own PR.

Root stayed on the operator's branch the whole time, and I never touched its working tree. `git fetch origin main` in Root only moved `origin/main`.

- **A throwaway graft for a 4-line change is worth it.** `graft new agents-grafts --from origin/main` printed "224 files reflinked, 0 copied, 30 written from git objects" and was ready immediately. It came to 1.4 MiB. With worktrees I would have batched the AGENTS.md edit into the redesign branch to avoid another checkout.
- **`graft ls` reads as a team dashboard:** `website 13 changed`, `cand-d 6 changed`, `agents-grafts 1 changed`. I could see the operator's progress without asking it.
- **Pain: the base is still stale.** `website` shows base `a70234c` although it fast-forwarded to `da86e87` before the redesign. Its "13 changed" counts from the old base, so it includes nothing from the merge, but it can't be checked against `main` from `graft ls` alone.
- **Pain: Root's untracked files are invisible to grafts, by design.** The owner dropped `references/` (logo and origami images) into Root's working tree. A graft made from `origin/main` doesn't have them, so I read them from Root's path directly.
  - **Ask:** `--clone-ignored` has a sibling for untracked inputs, for example `--clone-untracked references/`, for the "I pasted files into the repo for the agent" case.

## Take 3, part 4: landing two PRs from grafts, then cleaning up

The owner asked me to push, open PRs and merge. Both branches came straight from grafts, and Root was never touched (it stayed on the operator's branch):

| Step | Result |
| --- | --- |
| `git push origin docs/agents-grafts` from graft `agents-grafts` | ok. The graft's private repo had `origin` set, but **no `origin/main` ref** (it was made from Root's `origin/main`), so `git log origin/main..HEAD` failed inside it until a fetch |
| `git push origin feat/web-redesign` from graft `website` | ok |
| PR #21 (AGENTS.md rule), PR #22 (redesign + docs) | both `MERGEABLE CLEAN`; GitGuardian and three Vercel checks passed, including per-branch preview deploys |
| `gh pr merge --merge` on both | merged as `f9451b7` and `0ce76a7`. **All four commits landed as made** (47ca0d6, 8178ed7, c922265, b3048f8), unlike `graft integrate`'s squash in Take 1 |
| `graft prune agents-grafts` (no `--force`) | **0.06 s**, accepted. Graft saw the work was safe once it was on the remote and in Root's `origin/main` |
| `graft_run [git pull --ff-only origin main]` in `website` over MCP | 909 ms, fast-forward to `0ce76a7` |

**What I love:**
- **Graft → push → PR → merge needs no graft-specific landing at all,** and history comes out exactly as committed. For a preregistration repo, this is the landing path I'd document as the default.
- **Prune knows when work is safe.** No `--force` needed after the merge, and it would have refused before (Take 3, part 1).
- **One graft per PR kept the two reviews independent.** The 4-line AGENTS.md change didn't ride along in a 1,500-line site PR.

**Pain points:**
1. **The stale base is now actively misleading.** After the merge and fast-forward, `website` is byte-for-byte `main` (`0ce76a7`), but `graft ls` shows `BASE a70234c` and **`CHANGED 19`**. An agent deciding "is anything unlanded here?" from `graft ls` would get the wrong answer and might not prune a finished graft, or worse, might re-land.
   - **Ask:** compute CHANGED against the branch's upstream or a chosen ref (`graft ls --against origin/main`), or move the base when HEAD fast-forwards onto a descendant.
2. **A graft from `--from origin/main` has no `origin/*` refs of its own.** Range commands fail until you fetch inside the graft.
   - **Ask:** copy the remote-tracking ref the graft was created from.
3. **(Repeat) Untracked inputs.** The owner's `references/` images exist only in Root's working tree. Every graft had to read them from Root's path by hand.

## Take 3 verdict

Over one session I ran:
- 1 worktree, then deleted it;
- 6 benchmark grafts;
- 3 working grafts (`website`, `agents-grafts`, and the operator's `cand-d`).

That landed 3 PRs (#20 from the worktree, #21 and #22 from grafts). No commits were squashed, no checkout was shared, and a long-lived second session kept working the whole time.

**For multi-agent work on one repo, graft is the better default.** The reasons are isolation and cheap, safe teardown; speed matters less on a repo this small.

**What would make it excellent:**
- a space-free home path;
- `graft ls` that tells the truth after a pull or merge;
- a way to carry untracked inputs into a graft;
- visible paths, so humans can find the work.

---

# Take 4: one long-running candidate graft (cand-d), CLI only

Date: 2026-11-20. One graft, `cand-d`, held a full day of work on a robot candidate: five declaration versions, ten independent review records, four official evaluator runs and about 20 commits. Everything landed by `git push origin HEAD:feat/cand-d-coupler-first`, never by `graft integrate`.

## What I ran

- **Moving in:** I moved from Root into a graft mid-task, at the owner's request.
  - `graft new cand-d --from feat/cand-d-coupler-first --clone-ignored .toolchain`;
  - then a push from inside the graft (origin was already set).
- **Daily use:** `graft path`, `graft ls`, `graft status`, `graft claim` (4 paths), `graft claims`, `graft radar` and `graft ring`.
- **Builds:** pinned-Zig builds and runs inside the graft. Each official run used `rm -rf zig-out && tools/zig build -Doptimize=ReleaseSafe` from a clean tree, so the embedded revision and dirty flag are correct.
- **Sub-agents:** fresh-session reviewers (sub-agents) worked inside the graft by absolute quoted path.

## Numbers

| Operation | Result |
| --- | --- |
| `graft new` with `--clone-ignored .toolchain` | **1.08 s** wall: 226 files reflinked, plus 19 507 toolchain files reflinked, 0 copied |
| `graft ls` right after | 48 KiB on disk (the website graft showed 79 MiB) |
| First ReleaseSafe build in the fresh graft (cold `.zig-cache`) | 26 s wall |
| `tools/zig build test` uncached in the graft | about 60–90 s, exit 0. Same results as Root (no graft-specific failures) |
| `graft claim` on 4 paths | 8 ms |
| `graft radar` with the website graft live | "No overlaps between live grafts" |

## What I loved

- **`--clone-ignored .toolchain` again.** Zero network and 1 s for a 470 MB pinned compiler. This is what made a mid-task move cheap enough to do on request.
- **The graft is a normal git repository with origin set.** Pushing straight to the feature branch kept the preregistration order (decision → candidate → reviews → run) commit by commit. Squash landing would have destroyed that.
- **Claims and radar are a cheap safety net** next to a parallel website graft. They cost nothing and confirmed the two streams never touched the same files.

## Pain points

1. **The path has a space** ("Application Support"). Every shell call needed `cd "$(graft path cand-d)"` with quotes. My tool resets the cwd after each call, so I typed that prefix about 150 times. One `cd ..` chain overshot the root and a commit silently did not run.
   - **Ask:** a space-free default root (for example `~/.graft/`), or a `graft exec <name> -- <cmd>` that runs in the graft's root.
2. **`graft ring` and `graft status` disagree on what the delta is.**
   - After committing, `graft status` listed my 8 added files relative to base `de5855c`, but `graft ring cand-d candidate-committed` said "0 changed paths".
   - The ring is presumably over the uncommitted delta. That is surprising when status is base-relative.
   - **Ask:** say which delta each command uses ("committed since base: 8; uncommitted: 0").
3. **Sub-agents work by path, not by confinement.** The reviewers wrote only their assigned files, and `git status` proved it each time. But nothing in graft enforced it. Take 2's ask for a confined `graft run` session for agents still stands.
4. **`graft list` does not exist; it is `graft ls`.** The CLI's "similar subcommand" hint is good. An alias would cost nothing.

## Verdict

For a long, audit-heavy workstream next to a parallel one, a graft plus push-to-branch is the right shape. Creation cost and disk use are negligible, and git history stays intact. The space in the path is the one daily tax.
