# Role 5: AI features, platform tests, demo

## Goal

1. Optional local AI helpers: "Shorten" and "Generate decoy".
2. Find out exactly which messengers keep Polyphonia files intact.
3. Own the demo script and the pitch.

## Owned files

- `docs/platform-tests.md` (create it)
- AI feature code (where it lives is agreed with role 4, see below)
- Demo script and pitch material (e.g. `docs/demo.md`)

## Interfaces you use

No Rust trait to implement. You use:

- The CLI (works today with `--algo fake --carrier raw`):
  ```sh
  poly keygen --algo fake --out bob
  poly encrypt --to bob.pub --algo fake --carrier raw --text "hi" --out msg.bin
  poly decrypt --key bob.key --pub bob.pub msg.bin
  poly inspect msg.bin
  ```
- The engine's errors: `ChecksumMismatch` = the file was changed in transit
  (recompressed); `UnrecognizedFile` = no Polyphonia data found at all.
- Sizes (from `docs/SPEC.md`): the music carrier moves ~40 bits/s, so every
  byte saved in the message is 0.2 s less song. An ML-KEM envelope is ≥ 1199 bytes.

## Done criteria

- `docs/platform-tests.md` filled in for every messenger below.
- AI buttons work when Ollama runs and are hidden when it doesn't.
- A rehearsed demo script that runs end to end in under 3 minutes.

## Order of work

1. **Platform test matrix (start now; the raw and PNG carriers are enough).**
   Messengers: WhatsApp, Telegram, Signal, Discord, iMessage, Instagram, Messenger.
   For each carrier file type (`.bin`, `.png`, `.wav`, `.mid`) send it two ways:
   **normally** (as photo/audio) and **as a document/file**. On the receiving
   side compare:
   ```sh
   shasum -a 256 original.png received.png
   poly inspect received.png      # ok / checksum mismatch / unrecognized
   ```
   Record: messenger, carrier, send mode, hash same?, `poly inspect` result,
   what changed (format, dimensions, size). Use role 2's `poly damage` to
   reproduce what you find.
2. **AI (optional, local only):** a local LLM through LM Studio or Ollama.
   Built in `crates/poly-ai`; details in `docs/ai.md`.
   - Detect both with `GET /v1/models` on their OpenAI-compatible APIs:
     LM Studio at `http://127.0.0.1:1234/v1`, Ollama at
     `http://127.0.0.1:11434/v1` (their default ports); if neither answers,
     hide the AI buttons.
   - "Shorten": rewrite the message shorter so the song is shorter.
   - "Generate decoy": a believable harmless message for hidden mode.
   - Network code lives only in `poly-ai` (loopback only); `poly-core` and
     `poly-engine` stay network-free. Role 4 exposes it through `poly-ffi`
     (functions listed in `docs/ai.md`, "For the Mac app").
3. **Demo + pitch:** the story (hidden mode, post-quantum vs RSA, role 1's
   live "tiny RSA" factoring), the exact click path, and a fallback recording.

## Pitfalls

- Never send real secrets in tests; use the fake algorithm.
- Messenger behavior varies by platform and app version: record OS, app version and date per row.
- Some messengers strip or rename files; compare hashes, not file names.
- The AI must be local. Never send the message text to a cloud API, since that would
  defeat the whole product. Never log prompts containing message text.
- Both slots pad to the longer text, so a decoy much longer than the real message
  makes the file (and the song) longer. Ask the model for a decoy of similar length.
