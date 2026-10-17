# Role 3: music carriers

## Goal

Turn envelope bytes into a melody that sounds like music and back:
`MusicMidiCarrier` (exact decode from a MIDI file) first, then
`MusicWavCarrier` (rendered audio, decoded with FFT pitch detection), and make
it survive voice-note re-encoding.

## Owned files

- `crates/poly-music/` (`src/midi.rs`, `src/wav.rs`, `src/lib.rs`)

## The trait to implement

```rust
pub trait Carrier: Send + Sync {
    fn kind(&self) -> CarrierKind;               // RawDebug = 0, StealthPng = 1, RobustPng = 2, MusicWav = 3, MusicMidi = 4
    fn file_extension(&self) -> &'static str;    // "bin", "png", "wav", "mid"
    fn mime_type(&self) -> &'static str;
    /// Max envelope bytes this carrier can hold with these options (None = unlimited).
    fn capacity(&self, opts: &CarrierOptions) -> Option<usize>;
    fn encode(&self, envelope: &[u8], opts: &CarrierOptions) -> Result<Vec<u8>, PolyError>;
    /// Must return Err(PolyError::NotThisCarrier) quickly when the file isn't its format, so auto-detect moves on.
    fn decode(&self, file: &[u8]) -> Result<Vec<u8>, PolyError>;
}
#[derive(Default)]
pub struct CarrierOptions { cover_image: Option<Vec<u8>>, music_style: Option<String>, seed: Option<u64> }
```

Contract:
- `encode` returns `PolyError::CarrierTooSmall { needed, available }` when the envelope doesn't fit.
- `decode` output must **start with** the envelope bytes; trailing padding is
  fine. The envelope's `payload_len` (header bytes 15..19, big-endian u32,
  total length = 23 + payload_len) tells you when to stop.
- `decode` must never panic and must return an error on junk (non-MIDI/WAV files, random audio).
- Must work with `CarrierOptions::default()`; `opts.music_style` may pick a style later.

Envelope format: `docs/SPEC.md` §2 (magic `POLY`, 23 bytes of overhead, CRC-32 at the end).

## Done criteria

Remove `#[ignore]` from the `roundtrip` test in `src/midi.rs` and `src/wav.rs`;
both call `poly_core::test_utils::assert_carrier_roundtrip` (random envelopes
up to capacity, `CarrierTooSmall` above it, junk rejection, truncated files).
Then:

```sh
poly encrypt --to bob.pub --algo fake --carrier midi --text hi --out song.mid
poly decrypt --key bob.key --pub bob.pub song.mid
```

## Test commands

```sh
cargo test -p poly-music -- --include-ignored
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Order of work

1. **MVP: MusicMidiCarrier** (exact decode, `midly` crate).
   - Each note carries 5 bits: pitch from an 8-note scale (3 bits, since 8 = 2³)
     + one of 4 durations (2 bits, since 4 = 2²).
   - 8 notes per second → 40 bits per second.
   - A fixed background chord progression on another track/channel carries no data.
   - A start marker (a fixed note pattern) so the decoder knows where data begins.
   - Capacity: return `None` (unlimited) or a cap you choose for song length; document which.
2. **MusicWavCarrier** (`hound` + `rustfft`): render the same notes as pure
   sine tones between 300 and 3400 Hz (the telephone voice band every voice
   codec keeps) at 44,100 Hz sample rate (CD standard); decode with FFT pitch
   detection per note window.
3. Survive voice-note re-encoding: test with `poly damage` (role 2 adds Opus
   re-encode via ffmpeg) or `ffmpeg -i song.wav -c:a libopus -b:a 24k out.ogg` and back.
   Add redundancy/error correction inside your carrier if needed.
4. **Stretch:** live microphone decoding (coordinate with role 4 for the app side).

## Pitfalls

- Size: the ML-KEM kem_block alone is 1088 bytes = 8704 bits ≈ 3.6 minutes at
  40 bit/s. **Test with `--algo fake` first** ("hi" = 111-byte envelope ≈ 22 s).
  Real ML-KEM music messages need SESSION mode (role 1, later).
- Use only named constants for scale, durations, tempo and frequencies, with a comment saying why.
- MIDI timing: quantize durations to ticks so decoding is exact; don't rely on float timing.
- FFT: window length must resolve neighbouring scale notes; choose note frequencies far enough apart
  (and away from harmonics of each other) for robust detection.
- Avoid clicks: apply a short fade in/out per note in WAV; it also helps the FFT.
- `decode` must check the file signature (`MThd` / `RIFF....WAVE`) and return `NotThisCarrier` fast.
- Bound memory: don't allocate based on untrusted lengths in the file header.
