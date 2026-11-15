# Security policy

Graft confines agent processes (`graft run`), serves files over local
FUSE and NFS, and runs build commands from `graft.toml`, so reports about
sandbox escapes, path traversal, writes outside a graft, or commands
running without approval are especially welcome.

## Reporting a vulnerability

Please report privately through
[GitHub's private vulnerability reporting](https://github.com/404khai/graft/security/advisories/new),
not in a public issue. Include the Graft version (`graft --version`),
`graft doctor` output, and steps to reproduce.

You will get an acknowledgement within a few days. Fixes ship in a patch
release, and the advisory is published once one is available.

## Supported versions

The latest release gets security fixes.

## Known limits

Some limits are documented, not bugs. For example, ADR 0009 notes that
other local users can reach a graft's NFS port on a shared Mac. See
`docs/decisions/`.
