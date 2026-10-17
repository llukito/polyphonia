# Rules for AI agents in this repo

Polyphonia: encrypt a short text (ML-KEM-768 or RSA-2048) and hide it in a PNG
or a WAV/MIDI song. Rust workspace (`crates/`) + Swift macOS app (`app/`) via UniFFI.

## Before coding
- Read `docs/SPEC.md` and your `docs/roles/<n>.md`.

## Ownership
| Path | Owner |
|---|---|
| `crates/poly-core`, `crates/poly-engine`, `docs/SPEC.md` | role 1 (lead); others change them only via a PR the lead approves |
| `crates/poly-crypto` | role 1 |
| `crates/poly-image`, the `poly damage` subcommand | role 2 |
| `crates/poly-music` | role 3 |
| `crates/poly-ffi`, `app/`, `scripts/build-xcframework.sh` | role 4 |
| `crates/poly-platform-check`, `profiles/`, `docs/platform-tests.md`, AI features, demo | role 5 |

## Rules
- Edit only files you own. Need a change in poly-core, poly-engine or SPEC.md?
  Stop and write the proposed change for the lead instead of making it.
- Never change the envelope or payload format silently.
- Before every commit: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace` must pass.
- No magic numbers: every constant is named, with a comment saying where its value comes from.
- Never log plaintext, keys or passphrases (only the CLI's explicit output prints text).
- Branch per role (`role/<name>`), small commits, PR to main.
- Commits and PRs must contain no AI co-author or attribution lines.
