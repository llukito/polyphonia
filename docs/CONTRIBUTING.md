# Contributing

## Branches

- `main` always builds and passes CI. Nobody pushes to it directly.
- One branch per role: `role/crypto`, `role/image`, `role/music`, `role/mac-app`, `role/ai-demo`.
  Short-lived topic branches off your role branch are fine (`role/image-qr`).
- Rebase on `main` often: `git fetch && git rebase origin/main`.

## Pull requests

1. Small commits with plain messages saying what changed and why.
2. Before every commit, all three must pass:
   ```sh
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
3. Open a PR to `main`. CI runs fmt, clippy and tests on Ubuntu.
4. Touch only files you own (see the table in `README.md`). A PR that changes
   another role's files needs that owner's review.
5. Commits and PRs must contain no AI co-author or attribution lines.

## Changing shared code

`crates/poly-core`, `crates/poly-engine` and `docs/SPEC.md` belong to the
lead (role 1). If you need a change there:

1. Don't edit them on your branch.
2. Write the proposal (what, why, the exact new type or format) in a GitHub
   issue or a message to the lead.
3. The lead makes the change in its own PR; you rebase on it.

Never change the envelope or payload format silently. A format change must
update `docs/SPEC.md` and regenerate the golden vectors in the same PR:

```sh
POLY_REGEN_VECTORS=1 cargo test -p poly-engine --test golden
```

## Finishing a stub

Every stub already has its conformance test, marked
`#[ignore = "role N: remove when implemented"]`. You're done when you have
deleted the `#[ignore]` line and the test passes:

```sh
cargo test -p <your-crate> -- --include-ignored   # run them before removing #[ignore]
```

## Code rules

- No magic numbers: every constant is named, with a comment saying where its value comes from.
- Never log plaintext, keys or passphrases. Only the CLI's explicit output prints text.
- Decoders must never panic on any input.
- Code must build and test on both macOS and Linux (except `app/` and the XCFramework script).
