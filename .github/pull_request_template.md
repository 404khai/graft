## What changed

<!-- One or two sentences. Keep the PR small. -->

## Why

<!-- The bug or constraint this addresses. Link the issue: Fixes #123 -->

## How to test

<!-- The exact commands a reviewer can run. -->

## UI

<!-- Only for changes to the website (web/) or to CLI output: before/after screenshot or output. Otherwise delete this section. -->

## Checklist

- [ ] I ran `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings` and `cargo test --workspace`
- [ ] For `web/` changes: I ran `npm run build` in `web/`
- [ ] New behaviour has a test; a bug fix has a regression test
- [ ] The PR title is a conventional commit (`fix(cli): …`, `feat(fs): …`)
- [ ] This PR is small and focused
- [ ] I did not mix unrelated changes
