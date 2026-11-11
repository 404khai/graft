# 0018: Scheduler, partitioning and claims
Status: accepted

## Context

AGENTS.md Phase 14: split a multi-part task into grafts that rarely
collide, and order them so interfaces land first.

- A file- and symbol-level dependency graph from tree-sitter (imports,
  references); hub detection; partitioning by community detection (Leiden
  or Infomap, with an ADR).
- `graft plan <task.md>` prints an ordered task DAG: each node is a set of
  files or symbols to claim, a suggested graft name and its dependencies,
  with a "contract" node for hub changes first. `--apply` creates the
  grafts, capped at the DAG's width.
- Claims: each graft holds soft claims; a write outside a claim is
  admitted against live claims (allowed, queued, or rejected naming the
  conflicting graft).

Acceptance: coupled files land in one node; hub changes are always in the
first node; admission rejects a write to a symbol another live graft
claims and names it; plans are deterministic.

## Options considered

**What a task names**

- A. Ask a model to read the task. Not deterministic, and Graft has no
  model.
- B. The task's code spans and path-like words: every `` `path` ``,
  `` `path::symbol` `` or bare symbol (defined in exactly one file) that
  exists at Root's HEAD is a seed. Text that names nothing is ignored, and
  `graft plan` lists what it found, so the author can see and fix it.

**Partitioning**

- C. Infomap. Good on flow, but iterative and random-seeded; no
  maintained Rust crate.
- D. Leiden. Guarantees connected communities; the reference
  implementations are randomised; no maintained Rust crate.
- E. Louvain modularity optimisation with a fixed node order (files
  sorted by path), followed by Leiden's connectivity refinement (a
  community whose induced subgraph is disconnected is split into its
  components). Deterministic, no dependency, ~150 lines, and it keeps the
  property that matters here: every node is internally connected.

**The graph**

Nodes are tracked files at Root's HEAD in Rust, TypeScript, Python and
Go. An import that resolves to another tracked file adds weight 1; every
identifier in a file that names a symbol defined in a file it imports
adds 1 more. Resolution, per language: Rust `mod x;` and `use
crate::/super::/self::` paths against the crate's `src/`; Python
absolute and relative imports (`a/b.py`, `a/b/__init__.py`);
TypeScript relative specifiers (`.ts`, `.tsx`, `index.ts`); Go imports
under the module path in `go.mod` (every non-test file of the package).
Unresolved imports (dependencies outside the repo) add nothing.

**Enforcing claims**

- F. Only advisory: report violations. Agents ignore advice.
- G. At write time where Graft sees writes, and at integrate time
  everywhere. Virtual grafts: when graftd journals a file (on flush,
  fsync or close), an admission hook compares the symbols the write
  changes (against the base) with other live grafts' claims; a refused
  write is not journaled, the file keeps its previous contents, the call
  fails with `EACCES` and the verdict is recorded. Reflink grafts never
  pass writes through Graft, so their claims are enforced when they
  integrate: an entry whose changes touch another live graft's claim
  stops with the verdict. `graft claim` asks for admission explicitly.

## Decision

B + E + G.

- **Hubs** are files imported by at least 3 others and by at least a
  fifth of the files with any edge. Seeds in hubs form the contract node,
  which every other node depends on.
- **Nodes** are the communities that contain seeds, restricted to the
  seeds, ordered topologically by import edges between them (an edge
  that would close a cycle is dropped, lowest-index first), ties by the
  node's first path. Each gets a graft name from its files' common
  directory (`contract` for the hub node), made unique with `-2`, `-3`.
- **Width** is the size of the largest level of the DAG. `--apply`
  creates the nodes whose dependencies have integrated, at most `width`
  of them, and records their claims; rerunning it after integrations
  creates the next level.
- **Claims** (schema v10, `claims`): `(graft, path, symbol or NULL for the
  whole file, plan order)`. Admission for a write by graft G to symbols S
  of path P, against claims of other *live* grafts:
  - none → `allowed`;
  - held by a graft earlier in the same plan (it lands first) → `queued`:
    refused now, and admitted once that graft integrates;
  - otherwise → `rejected`.
  Verdicts that refuse name the holder and the symbol. A file claim covers
  every symbol in it, and changes outside any symbol.
- **Verdict log** (`admissions`): refused writes, with holder, path and
  symbol, shown by `graft claims`.

## Consequences

- Plans are a pure function of Root's HEAD and the task text.
- Hard write-time admission exists for virtual grafts only; reflink
  grafts are held at integrate time. Both are tested.
- A refused write in a virtual graft is lost unless the writer retries
  (the error comes back from `close`, which many tools ignore); the
  verdict log is where an agent or person finds out why.
- Admission judges file contents when they are journaled. A file emptied
  in place is admitted, because `open(O_TRUNC)` then `write` can journal
  the empty file first on NFS; its next contents are checked. Deleting or
  renaming a claimed file is not checked at write time; integrate still
  catches it (a deleted file changes all its symbols).
- Claims fail open: if graftd cannot read the store, writes are admitted
  with a warning rather than lost.
- New: schema v10, `graft plan`, `graft claim`, `graft claims`; graft-fs
  gains an `Admission` hook that graftd fills from graft-sched.
