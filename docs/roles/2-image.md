# Role 2: image carriers

## Goal

Hide envelope bytes inside PNG images: `StealthPngCarrier` (invisible, needs
lossless sending) and `RobustPngCarrier` (visible pattern that survives
messenger JPEG recompression). Also build `poly damage` so the team can test
what messengers do to files.

## Owned files

- `crates/poly-image/` (`src/stealth.rs`, `src/robust.rs`, `src/lib.rs`)
- The `poly damage` subcommand in `crates/poly-cli/src/main.rs` (only that match arm and its args)

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
  fine (the envelope parser ignores bytes after its CRC, and the envelope's own
  `payload_len` tells you its length: header bytes 15..19, big-endian u32, total = 23 + payload_len).
- `decode` must never panic and must return an error on junk (including non-PNG files and random PNGs).
- Must work with `CarrierOptions::default()`.

The envelope format is in `docs/SPEC.md` §2: magic `POLY`, 23 bytes of overhead, CRC-32 at the end.

## Done criteria

Remove `#[ignore]` from the `roundtrip` test in `src/stealth.rs` and
`src/robust.rs`; both call `poly_core::test_utils::assert_carrier_roundtrip`,
which checks random envelopes of several sizes up to `capacity`,
`CarrierTooSmall` just above capacity, junk rejection and truncated files.
Then:

```sh
poly encrypt --to bob.pub --algo fake --carrier stealth-png --text hi --out msg.png
poly decrypt --key bob.key --pub bob.pub msg.png
```

## Test commands

```sh
cargo test -p poly-image -- --include-ignored
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Order of work

1. **MVP: StealthPngCarrier.** Hide data in the least significant bit of R, G
   and B (1 bit per channel × 3 channels = 3 bits per pixel). Cover image from
   `opts.cover_image` (decode with the `image` crate), otherwise a generated
   default (e.g. a gradient or noise pattern seeded by `opts.seed`).
   Capacity = width × height × 3 / 8 bytes (a 512×512 cover ≈ 98 KB). Output
   PNG only. Only survives lossless (file/document) sending.
2. **RobustPngCarrier v0:** a QR code via a crate, to prove the path end to end.
3. **RobustPngCarrier v1:** custom grid.
   - 1024×1024 image: a power of two below the ~1200 px limit where messengers start resizing.
   - 16×16 px cells: each covers 2×2 whole 8×8 JPEG blocks, so the cell color survives JPEG.
   - 4 well-separated colors → 2 bits per cell.
   - Corner finder markers to locate the grid after resizing/cropping.
   - Reed-Solomon with about 50% parity.
   - Raw capacity = (1024/16)² = 4096 cells × 2 bits = 1024 bytes → about 500 usable.
4. **`poly damage`:** JPEG recompression at a given quality and resizing
   (`image` crate); Opus re-encode through `ffmpeg` if installed (skip with a
   message if not). Example: `poly damage msg.png --jpeg 70 --resize 800 --out damaged.jpg`.
5. **Stretch:** decode RobustPng from a JPEG (what messengers actually deliver),
   make the grid look like art, not a barcode.

## Pitfalls

- PNG only for StealthPng: any lossy format destroys the LSBs.
- Convert the cover to RGB8/RGBA8 first; keep alpha untouched (alpha = 0 pixels may get their RGB zeroed by some encoders).
- Don't trust dimensions from a file: a huge declared size must not allocate unbounded memory. Check limits before decoding.
- `decode` on any PNG returns *something* for StealthPng (every PNG has LSBs). That's fine, since the engine then
  rejects it as `BadMagic`. But check the first 4 extracted bytes are `POLY` and return `NotThisCarrier` early to save time.
- An ML-KEM envelope is ≥ 1199 bytes and does **not** fit RobustPng (~500 bytes). Test with `--algo fake` (111 bytes for "hi") or RSA (367).
- If `poly damage` needs a new CLI dependency, coordinate with the lead (it touches the shared `Cargo.toml`).
- Use `cargo add -p poly-image <crate>`, then move the version into `[workspace.dependencies]` in the PR.
