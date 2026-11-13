# 0020: Materialize, fuzzing and release
Status: accepted

## Context

AGENTS.md Phase 16: `graft materialize <name> [--branch] [--dir]` for tools
that cannot run on a graft; chunk-level copy-up "if benchmarks justify
it"; fuzzing of the FUSE/NFS request handlers and the op log parser (24 h,
no crashes); final benchmarks in the README; `cargo install`, prebuilt
binaries and a Homebrew formula; signed binaries for Linux and macOS.

## Options considered

**What materialize writes, and how it comes back**

- A. The graft's working tree as is. Includes ignored files and layers
  (`node_modules`, build output): large, machine-specific, and not what
  `graft diff` shows.
- B. The graft's files as git sees them: the base tree with the graft's
  changes (contents, symlinks, the executable bit), as a directory that
  must not exist or be empty, or as one commit on the base in a new
  branch of Root. `graft import <name> (--dir | --branch)` is the way
  back: it makes the graft's files match the source, writing only what
  differs and removing what the source lacks, never touching ignored
  files. The round trip then gives exactly the source's difference from
  the base as the graft's delta.

**Chunk-level copy-up**

Not done. No measurement shows whole-file copy-up as the cost: the
in-graft workloads that lose on FUSE and NFS (ADR 0010) are many small
files and large sequential writes, not edits to large existing files.
It stays a candidate for the deferred performance work.

**Fuzzing**

cargo-fuzz (libFuzzer, nightly only, so in `fuzz/` outside the
workspace) with five targets:

- `fs_requests`: arbitrary sequences of the `GraftView` operations the
  FUSE and NFS handlers call (lookup, create, open, write, read, flush,
  fsync, release, mkdir, unlink, rmdir, rename, symlink, chmod, truncate,
  readdir); after each sequence, replaying the op log must reproduce the
  delta. The NFS wire protocol itself is parsed by `nfsserve`, which is
  not ours to fuzz.
- `oplog_rows`: arbitrary `op_log` and `delta` rows must decode or fail
  with `Corrupt`.
- `graft_toml`, `templates`, `source_analysis`: every parser of
  repository contents (untrusted, AGENTS.md §11).

A workflow runs each target for five rounds of ~4.9 h (hosted jobs stop
at 6 h), carrying the corpus between rounds: ~24.5 h per target.

**Signing**

- C. Apple Developer ID code signing and notarisation, and GPG
  signatures. Need the maintainer's certificates and keys as secrets.
- D. GitHub artifact attestations (`actions/attest-build-provenance`):
  Sigstore keyless signatures over each archive, binding it to this
  repository, workflow and commit; verified with `gh attestation verify`.
  No secrets.

## Decision

B, no chunking, the fuzzing above, and D. Releases are built for Linux
x86_64 and aarch64 and macOS arm64; a `v*` tag creates a **draft** release
with the archives, checksums and a generated Homebrew formula, which the
maintainer reviews and publishes, and publishes the formula in a tap.

## Consequences

- macOS binaries are not notarised: Gatekeeper quarantines a downloaded
  `graft` until it is cleared (`xattr -d com.apple.quarantine`) or built
  from source. C can be added once the maintainer has the certificates.
- No macOS x86_64 build (no supported hosted runner); build from source.
- Graft is licensed MIT; release archives include `LICENSE` and the
  Homebrew formula declares it.
