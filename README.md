# Polyphonia

A macOS menu bar app that hides post-quantum encrypted messages inside
ordinary images and songs.

Type a short text, pick a contact, an algorithm (**ML-KEM-768** post-quantum,
or **RSA-2048** for comparison) and a carrier (**image** or **music**). The app
encrypts the text and hides it inside a PNG or a WAV/MIDI file. Send that file
in any messenger. The receiver drops it into their app, which auto-detects the
carrier, decrypts it and shows the text.

**Hidden mode:** every message has two equal-size slots. Slot A opens with the
normal key; slot B also needs a passphrase agreed in person. In hidden mode A
holds a harmless decoy and B the real text. Without the passphrase nobody can
tell whether B holds anything.

## Status

| part | state |
|---|---|
| envelope + payload formats, engine, auto-detect, CLI, FFI | done |
| FakeCipher (NOT SECURE) + RawDebug carrier | done, the whole pipeline runs today |
| ML-KEM-768, RSA-2048 | stubs → role 1 |
| StealthPng, RobustPng | stubs → role 2 |
| MusicMidi, MusicWav | stubs → role 3 |
| macOS app | not started → role 4 |

## Build and test

Requires Rust stable (pinned by `rust-toolchain.toml`). Works on macOS and Linux.

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Run one owner's ignored tests (once implemented):

```sh
cargo test -p poly-image -- --include-ignored
```

Mac app library (macOS with full Xcode only):

```sh
scripts/build-xcframework.sh   # → build/PolyphoniaCore.xcframework + build/swift/PolyphoniaCore.swift
```

## CLI demo

Copy-paste; uses the fake cipher and the raw carrier, which work today.

```sh
cargo build -p poly-cli
alias poly=./target/debug/poly

# Bob makes a key pair (bob.pub, bob.key)
poly keygen --algo fake --out bob

# Normal message
poly encrypt --to bob.pub --algo fake --carrier raw --text "lunch at noon?" --out msg.bin
poly decrypt --key bob.key --pub bob.pub msg.bin
#   text:      lunch at noon?
#   slot:      A

# Hidden mode: decoy in slot A, real text in slot B
poly encrypt --to bob.pub --algo fake --carrier raw \
  --text "the real plan" --decoy "happy birthday!" --passphrase "orchid" --out secret.bin
poly decrypt --key bob.key --pub bob.pub secret.bin                       # happy birthday! (slot A)
poly decrypt --key bob.key --pub bob.pub --passphrase orchid secret.bin   # the real plan  (slot B)
poly decrypt --key bob.key --pub bob.pub --passphrase tulip secret.bin    # happy birthday! (slot A)

# Header fields, no key needed
poly inspect secret.bin

# A damaged file is reported as "compressed in transit"
printf 'X' | dd of=msg.bin bs=1 seek=30 conv=notrunc 2>/dev/null
poly decrypt --key bob.key --pub bob.pub msg.bin
```

Passphrases given on the command line end up in shell history; the CLI is for
testing only.

## Repository layout

```
crates/poly-core     shared types, traits, errors, envelope + payload formats, test helpers
crates/poly-crypto   Cipher implementations (Fake, ML-KEM-768, RSA-2048)
crates/poly-image    image carriers (StealthPng, RobustPng)
crates/poly-music    music carriers (MusicMidi, MusicWav)
crates/poly-engine   encrypt/decrypt, carrier registry, auto-detect, RawDebug carrier
crates/poly-ffi      UniFFI wrapper for Swift
crates/poly-cli      the `poly` binary
crates/poly-platform-check  `platform-check` binary: messenger send tests (role 5)
crates/poly-ai       optional local AI (Shorten, Generate decoy) + `poly-ai-eval` (role 5)
profiles/            platforms.json (per-messenger send mode and steps), ai-prompts.json (role 5)
app/                 macOS app (Xcode project, role 4)
scripts/             build-xcframework.sh
tests/vectors/       golden files
docs/                SPEC, contributing guide, one guide per role
```

## Team and ownership

| role | area | owns |
|---|---|---|
| 1 (lead) | crypto + core | `poly-core`, `poly-engine`, `docs/SPEC.md`, `poly-crypto` |
| 2 | image | `poly-image`, the `poly damage` subcommand |
| 3 | music | `poly-music` |
| 4 | Mac app | `poly-ffi`, `app/`, `scripts/build-xcframework.sh` |
| 5 | AI + demo | `poly-platform-check`, `poly-ai`, `profiles/`, `docs/platform-tests.md`, `docs/ai.md`, `tests/ai-samples.json`, AI features, demo |

`poly-core`, `poly-engine` and `docs/SPEC.md` change only through a PR the lead approves.

## Docs

- [docs/SPEC.md](docs/SPEC.md): binary formats and interfaces, the single source of truth
- [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md): branch and PR workflow
- Role guides: [1 crypto](docs/roles/1-crypto.md) · [2 image](docs/roles/2-image.md) ·
  [3 music](docs/roles/3-music.md) · [4 Mac app](docs/roles/4-mac-app.md) ·
  [5 AI + demo](docs/roles/5-ai-demo.md)
- [CLAUDE.md](CLAUDE.md): rules for AI coding agents
- [tests/vectors/README.md](tests/vectors/README.md): golden files
- [docs/platform-tests.md](docs/platform-tests.md): messenger send tests and the platform profiles
