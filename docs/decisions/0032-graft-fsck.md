# 0032: `graft fsck`
Status: accepted

## Context

Several of Graft's invariants are checked only by tests: replaying a
graft's op log from empty reproduces its delta (Phase 6), every referenced
blob exists and matches its hash, every layer a graft references is in the
cache, and prune leaves nothing behind (Phase 2). When a user reports
missing or wrong files, nothing checks those invariants on their machine.

## Options considered

1. **Check on open**, every time a store is opened. That is always current,
   but it makes every command pay for a full replay and blob hash.
2. **A read-only `graft fsck` command**, run on demand and exposed over MCP.
3. **`graft fsck --repair`.** Most damage has no safe automatic fix (a lost
   blob is lost), and a wrong repair destroys data (AGENTS.md §3.9).

## Decision

Option 2, read-only. For the Root of the current directory:

| Check | Severity | Hint |
| --- | --- | --- |
| `integrity_check`, schema version | error | bug report |
| each virtual graft: replay of its op log == stored delta (read twice before reporting, since a write can land between the two reads; graftd is asked to journal pending writes first) | error, names up to 5 differing paths | bug report; do not prune or integrate |
| referenced blobs (deltas and rings) exist, and hash to their names unless `--quick` | error | bug report |
| blobs referenced by nothing (superseded contents, normal until gc) | counted, not a finding | `graft gc` |
| each unpruned graft's layers exist | error | prune and recreate |
| each reflink graft's working tree exists | error | `graft prune --force` |
| entries of `mounts/` that belong to no graft (`.trash-*` excepted) | warning | check, then delete |

The exit code is 1 when there is any error. `--json` prints
`{root, ok, checked, findings}`. Under MCP, `graft_fsck` reports a failure
with code `fsck_failed`. No new dependencies.

## Consequences

- Users and agents can check the Phase 6 invariant on real data. The FUSE
  and NFS CI jobs now run `graft fsck` after real writes.
- A full check hashes every referenced blob, which can be slow for large
  deltas; `--quick` skips the hashing.
- It does not compare a reflink graft's files with anything: a reflink
  graft is a plain git checkout, and `git fsck` covers it.
- Repairs stay manual. A later ADR may add narrow, safe repairs, such as
  removing stray `mounts/` entries that are empty.
