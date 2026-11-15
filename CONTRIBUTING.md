# Contributing to Graft

Thanks for helping. Graft gives parallel coding agents copy-on-write
workspaces; see the [README](README.md) for what it does and
[docs/architecture.md](docs/architecture.md) for how.

## Before you start

- **Bugs:** open an issue with the bug template, including `graft doctor`
  output and the smallest set of commands that reproduces it.
- **Features or larger changes:** open an issue first, so we can agree on
  the approach before you write code. Changes that choose between real
  alternatives (a crate, an on-disk format, a protocol) get an ADR in
  `docs/decisions/`.
- **Security problems:** do not open an issue; see [SECURITY.md](SECURITY.md).

## Setup

Rust is pinned in `rust-toolchain.toml`, so `rustup` installs the right
version on first build. You need git; for the site, Node 20+.

```sh
git clone https://github.com/404khai/graft && cd graft
cargo build
cargo run -p graft-cli -- doctor
```

## Making a change

1. Fork, and branch from `main` as `<type>/<short-description>`, for example
   `fix/prune-dangling-mount` or `feat/radar-hunks` (no issue numbers or
   dates).
2. Keep the PR small and about one thing.
3. Add tests: every new behaviour has one, and every bug fix has a
   regression test that fails without the fix. Tests never touch your real
   `$GRAFT_HOME`; use `tempfile`.
4. Run what CI runs:

   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace
   ```

   For the site: `cd web && npm ci && npm run build`.

   FUSE, NFS and privileged filesystem tests are behind features and env
   vars (`GRAFT_TEST_FUSE=1 … --features fuse-tests`,
   `GRAFT_TEST_NFS=1 … --features nfs-tests`); CI runs them on every PR, so
   you only need them locally when you change `graft-fs` or `graft-daemon`.
5. Write commits as [Conventional Commits](https://www.conventionalcommits.org/):
   `<type>(<scope>): <lowercase imperative summary>`. Types: `feat`, `fix`,
   `perf`, `refactor`, `test`, `docs`, `build`, `ci`, `chore`. The scope is
   the crate without `graft-` (`cli`, `fs`, `store`, `mcp`, …), `web`, or
   `repo`. The PR title follows the same format.
6. Open the PR and fill in the template.

## Code rules

- `unsafe` only in `graft-fs` and `graft-sandbox`, each block with a
  `// SAFETY:` comment; every other crate has `#![forbid(unsafe_code)]`.
- No `unwrap`/`expect` in library code outside tests; `thiserror` in
  libraries, `anyhow` in binaries; `tracing`, not `println!`, in libraries.
- Public items have doc comments.
- New dependencies need a reason in the PR (why it, why not std, maintenance,
  license) and must pass `cargo deny check`.
- Any change to what Graft stores on disk is versioned in
  [docs/on-disk-format.md](docs/on-disk-format.md), with a migration.
- No command may delete files outside `$GRAFT_HOME` or a graft's own delta.

[AGENTS.md](AGENTS.md) has the full engineering standards. Coding agents
contributing to Graft should follow it, including its longer PR template.

## Review

`main` is protected: a PR needs one approving review, every CI check
passing, and resolved review threads. Reviews look for tests, a focused
diff and honest reporting (a missed benchmark target or a flaky test is
said, not hidden). PRs are squash-merged.

## License

By contributing you agree that your contribution is licensed under the
[MIT License](LICENSE).
