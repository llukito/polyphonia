# Polyphonia specification (v1)

This is the single source of truth for every binary format and interface.
Owner: role 1 (lead). Changes only through a PR the lead approves; any format
change also updates the golden vectors in `tests/vectors/`.

Reference implementation: `crates/poly-core` (formats, traits, errors) and
`crates/poly-engine` (pipeline). If code and this document disagree, that is a
bug: tell the lead.

## 1. Overview

```
text ──Cipher.seal──▶ payload ──Envelope──▶ envelope bytes ──Carrier.encode──▶ PNG / WAV / MIDI file
file ──Carrier.decode (auto-detect)──▶ envelope bytes ──Envelope──▶ payload ──Cipher.open──▶ text
```

- The Rust core is **stateless**: keys come in and go out as bytes. Key storage
  is the app's job (Keychain on macOS; plain files in the CLI, for testing).
- Every message has **two equal-size slots**. Slot A opens with the recipient's
  key. Slot B also needs a passphrase the two contacts agreed on in person.
  - Normal mode: A = real text, B = random bytes.
  - Hidden mode: A = decoy text, B = real text.
  - Without the passphrase nobody can tell whether B holds anything.

## 2. Envelope

All integers big-endian.

| offset | size | field | notes |
|---|---|---|---|
| 0 | 4 | magic | ASCII `POLY`. Random data matches 4 bytes only 1 in 2^32 (~4.3 billion) times, so auto-detect almost never gives false positives. |
| 4 | 1 | version | `1` |
| 5 | 1 | algorithm | `1` = ML-KEM-768, `2` = RSA-2048-OAEP, `0xFF` = Fake (testing only) |
| 6 | 1 | flags | bit 0 = SESSION (no KEM block, uses a saved session key; reserved, MVP always writes 0). All other bits must be 0. |
| 7 | 8 | contact_id | first 8 bytes of SHA-256(recipient public key). 64 bits makes a collision between two contacts practically impossible while costing few bytes in the slow music carrier. |
| 15 | 4 | payload_len | u32. u32 because images can carry about 98 KB, more than u16's 65,535. |
| 19 | N | payload | §3 |
| 19+N | 4 | CRC-32 | IEEE CRC-32 of bytes [0, 19+N). |

Total overhead: **23 bytes**.

The CRC detects *accidental* damage (e.g. messenger recompression) so the app
can say "file was compressed in transit". It is **not** security; AES-GCM
handles tampering.

### Parsing rules (`Envelope::from_bytes`)

Checks run in this order, each with a distinct error:

1. fewer than 4 bytes → `Truncated`; magic ≠ `POLY` → `BadMagic`
2. fewer than 19 + payload_len + 4 bytes → `Truncated`
3. CRC mismatch → `ChecksumMismatch`
4. version ≠ 1 → `UnsupportedVersion`
5. unknown algorithm → `UnknownAlgorithm`
6. reserved flag bit set → `ReservedFlags`

The CRC is checked before the field values so a damaged file reports
`ChecksumMismatch` rather than a misleading "unknown algorithm".
Bytes after the CRC are **ignored** (carriers may pad). The parser must never
panic on any input (enforced by a proptest).

### Known limitation

The header is plaintext, so someone who knows the format can tell a file is
Polyphonia once they extract its bits. Fine for the hackathon; future work is
encrypting the header.

## 3. Payload

```
payload = kem_block || slot_A || slot_B
slot    = nonce (12) || ciphertext || tag (16)
```

### KEM block

Present only when the SESSION flag is 0. Its length is fixed by the algorithm
(constants in `poly_core::payload`):

| algorithm | kem_block | source |
|---|---|---|
| ML-KEM-768 | 1088 bytes | FIPS 203 ciphertext size |
| RSA-2048-OAEP | 256 bytes | 2048-bit modulus / 8 |
| Fake | 0 bytes | |

### Slots

- `slot_A` and `slot_B` have **identical length**.
- nonce = 12 bytes (AES-GCM standard nonce), random per slot.
- tag = 16 bytes (full AES-GCM tag).
- Slot plaintext before encryption:
  `u16 text length || UTF-8 text || zero padding`.
  Both slots are padded to the same size: `2 + len(longer text)` rounded up to
  a multiple of 16 bytes, which hides the exact message length a little.
  u16 is enough because messages are short (the music carrier moves ~40 bit/s).
- In normal mode slot B is `slot_len` random bytes.

Sizes: `slot_len = 12 + padded + 16`, `payload_len = kem_block + 2 × slot_len`.
`poly_core::payload::payload_len(algorithm, flags, max_text_len)` computes it,
so the engine checks carrier capacity before doing any crypto.

Example envelope sizes for a 2-byte message ("hi", padded to 16):

| algorithm | envelope bytes |
|---|---|
| Fake | 23 + 0 + 2×44 = 111 |
| RSA-2048 | 23 + 256 + 88 = 367 |
| ML-KEM-768 | 23 + 1088 + 88 = 1199 |

### Associated data

AAD for AES-GCM = envelope header bytes **0..15** (magic through contact_id),
15 bytes. `payload_len` is excluded because it depends on the payload itself;
tampering with it breaks parsing or the CRC anyway.

### Key derivation (role 1 implements)

1. Shared secret **S** (32 bytes):
   - ML-KEM-768: `(kem_block, S) = Encapsulate(recipient_pk)`; receiver `S = Decapsulate(sk, kem_block)`.
   - RSA-2048: S = 32 random bytes; `kem_block = RSA-OAEP-SHA256-Encrypt(pk, S)` (MGF1-SHA256, empty label).
2. `key_A = HKDF-SHA256(ikm = S, salt = none, info = "polyphonia/v1/slot-a")`, 32 bytes.
3. `key_B = HKDF-SHA256(ikm = S, salt = Argon2id(passphrase, salt = contact_id), info = "polyphonia/v1/slot-b")`, 32 bytes.
   Argon2id makes guessing the passphrase slow.
4. Each slot: `AES-256-GCM(key, random 12-byte nonce, padded plaintext, aad)`.

Argon2id parameters (proposed by the lead; role 1 confirms before the first
real vectors): version 0x13, memory 19,456 KiB, iterations 2, parallelism 1
(the OWASP minimum recommendation), output 32 bytes. The 8-byte contact_id
meets Argon2's 8-byte minimum salt length.

Key encodings (proposed; role 1 confirms):

| algorithm | public_key | secret_key |
|---|---|---|
| ML-KEM-768 | 1184-byte encapsulation key | 64-byte seed (d ‖ z, FIPS 203) |
| RSA-2048 | PKCS#1 DER | PKCS#8 DER |
| Fake | 32 bytes | 32 bytes |

### Seal and open rules

- `decoy` Some and `passphrase` None → `MissingPassphrase`.
- `passphrase` Some and `decoy` None → `InvalidRequest` (a passphrase alone
  would be silently ignored, which would mislead the user).
- **Open:** if a passphrase is given, try slot B; if its tag verifies, return
  B. Otherwise return slot A. Always attempt **both** decryptions so timing
  doesn't reveal which slot opened. Nothing opens → `DecryptFailed`.
- A flipped byte in the KEM block or slot A, a wrong AAD or a wrong key →
  `DecryptFailed` when opening without a passphrase.

### FakeCipher (testing only, NOT SECURE)

Same layout, no encryption: nonce = 12 zero bytes, ciphertext = padded
plaintext, slot A tag = SHA-256(`"polyphonia/fake/slot-a"` ‖ public key ‖ aad ‖
nonce ‖ ct)[..16], slot B tag = SHA-256(passphrase)[..16]. Keys: secret = 32
random bytes, public = SHA-256(`"polyphonia/fake/public-key"` ‖ secret).

## 4. Traits

Defined in `crates/poly-core/src/traits.rs`; types in `types.rs`.

```rust
pub trait Cipher: Send + Sync {
    fn algorithm(&self) -> Algorithm;
    fn generate_identity(&self) -> Result<Identity, PolyError>; // Identity { public_key, secret_key }
    /// Build the full payload (kem_block + both slots) for the recipient.
    fn seal(&self, req: &SealRequest) -> Result<Vec<u8>, PolyError>;
    /// Open a payload; returns the text and which slot opened.
    fn open(&self, req: &OpenRequest) -> Result<Opened, PolyError>;
}

pub struct SealRequest { recipient_public_key: Vec<u8>, contact_id: [u8; 8], message: String,
                         decoy: Option<String>, passphrase: Option<String>, aad: [u8; 15] }
pub struct OpenRequest { my_secret_key: Vec<u8>, contact_id: [u8; 8], payload: Vec<u8>,
                         passphrase: Option<String>, aad: [u8; 15] }
pub struct Opened { text: String, slot: Slot }  // enum Slot { A, B }

pub trait Carrier: Send + Sync {
    fn kind(&self) -> CarrierKind;
    fn file_extension(&self) -> &'static str;   // "bin", "png", "wav", "mid"
    fn mime_type(&self) -> &'static str;
    /// Max envelope bytes this carrier can hold with these options (None = unlimited).
    fn capacity(&self, opts: &CarrierOptions) -> Option<usize>;
    fn encode(&self, envelope: &[u8], opts: &CarrierOptions) -> Result<Vec<u8>, PolyError>;
    /// Must return Err(PolyError::NotThisCarrier) quickly when the file isn't its format.
    fn decode(&self, file: &[u8]) -> Result<Vec<u8>, PolyError>;
}

#[derive(Default)]
pub struct CarrierOptions { cover_image: Option<Vec<u8>>, music_style: Option<String>, seed: Option<u64> }
```

Carrier contract details:

- `encode` returns `CarrierTooSmall { needed, available }` if the envelope doesn't fit.
- `decode` output must **start with** the envelope; trailing padding is allowed.
- `decode` must never panic and must reject junk with an error.
- Every carrier must work with `CarrierOptions::default()`.
- Conformance: `poly_core::test_utils::assert_carrier_roundtrip` and
  `assert_cipher_roundtrip` (feature `test-utils`).

## 5. Carriers

| id | CarrierKind | ext | MIME | owner | capacity |
|---|---|---|---|---|---|
| 0 | RawDebug | bin | application/octet-stream | lead | unlimited |
| 1 | StealthPng | png | image/png | role 2 | 3 bits/pixel (LSB of R, G, B); a 512×512 cover ≈ 98 KB |
| 2 | RobustPng | png | image/png | role 2 | ~500 bytes (1024×1024, 16 px cells, 2 bits/cell, RS ~50%) |
| 3 | MusicWav | wav | audio/wav | role 3 | 40 bit/s |
| 4 | MusicMidi | mid | audio/midi | role 3 | 40 bit/s |

The carrier id is not stored in the file; it is detected.

Capacity notes:

- An ML-KEM envelope (≥ 1199 bytes) does **not** fit RobustPng's ~500 bytes,
  and takes ~4 minutes of music at 40 bit/s (1088-byte KEM block = 8704 bits ≈
  3.6 min alone). Use Fake or RSA for those carriers until SESSION mode exists.

## 6. Engine

`crates/poly-engine`:

- `Engine::default()` registers every cipher and carrier.
- `encrypt(EncryptRequest { message, decoy, passphrase, recipient_public_key, algorithm, carrier, options })`
  → `EncodedFile { bytes, extension, mime, carrier, algorithm }`.
  Computes the envelope size first and returns `CarrierTooSmall` before any crypto.
- `decrypt(DecryptRequest { file, my_secret_key, my_public_key, passphrase })`
  → `Decrypted { text, slot, carrier, algorithm }`.
- `inspect(file)` → detected carrier + envelope, no key needed.
- `contact_id(public_key)`.

Auto-detect: try each carrier's `decode` in `CarrierKind` order (RawDebug,
StealthPng, RobustPng, MusicWav, MusicMidi); the first result that parses as an
envelope wins. If some carrier decoded but the CRC failed, remember
`ChecksumMismatch` and return it if nothing else works ("compressed in
transit"); otherwise `UnrecognizedFile`. Then, if
`contact_id(my_public_key)` ≠ the envelope's contact_id → `WrongRecipient`.

## 7. Errors

One enum, `poly_core::PolyError`, shared by every crate (mirrored as a UniFFI
error in poly-ffi). Messages never contain plaintext, keys or passphrases.

| group | variants |
|---|---|
| envelope | `BadMagic`, `UnsupportedVersion`, `UnknownAlgorithm`, `ReservedFlags`, `Truncated`, `ChecksumMismatch`, `PayloadTooLarge` |
| payload | `MalformedPayload`, `MessageTooLong` |
| crypto | `MissingPassphrase`, `InvalidRequest`, `InvalidKey`, `DecryptFailed`, `WrongRecipient`, `Crypto` |
| carriers | `NotThisCarrier`, `CarrierTooSmall { needed, available }`, `UnrecognizedFile`, `Carrier` |
| general | `NotImplemented` |

## 8. FFI (role 4)

`crates/poly-ffi`, UniFFI proc-macros, Swift module `PolyphoniaCore`. Exports
`generate_identity(algorithm)`, `contact_id(public_key)`, `encrypt(request)`,
`decrypt(request)`, `inspect(file)` and the matching records/enums/errors.
`scripts/build-xcframework.sh` produces `build/PolyphoniaCore.xcframework` and
`build/swift/PolyphoniaCore.swift` (needs full Xcode).

## 9. CLI key files (testing only)

```
polyphonia-public-key v1 <fake|mlkem|rsa>
<hex>
```

Secret key files use the header `polyphonia-secret-key v1` and are created
with mode 0600. The app does **not** use these; it stores keys in the Keychain.

## 10. Future work

- SESSION mode (flag bit 0): reuse a saved session key, no KEM block. Needed
  for practical ML-KEM music messages.
- Encrypt the header.
