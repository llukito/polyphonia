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
  for practical ML-KEM music messages. Proposed design in §11.
- Encrypt the header.

## 11. SESSION mode (PROPOSAL, awaiting lead approval)

> Status: draft by role 1 for the lead to approve. Nothing here is
> implemented. Once approved, the normative parts move into §2–§8 and this
> section shrinks to a summary.

### Goal

An ML-KEM envelope is at least 1199 bytes: too big for RobustPng (~500 bytes)
and ~4 minutes of music. In SESSION mode a message carries an 8-byte session
id instead of the KEM block:

| message ("hi") | envelope bytes | music at 40 bit/s |
|---|---|---|
| ML-KEM-768, full | 23 + 1088 + 88 = 1199 | ~4 min |
| RSA-2048, full | 23 + 256 + 88 = 367 | ~73 s |
| any algorithm, SESSION | 23 + 8 + 88 = **119** | **~24 s** |

### How a session starts

Every full message (SESSION flag 0) from a real cipher establishes a session
as a side effect, at no extra cost in bytes. Both sides know S, so both
derive the same values:

```
session_key = HKDF-SHA256(ikm = S,           salt = none, info = "polyphonia/v1/session-key"), 32 bytes
session_id  = HKDF-SHA256(ikm = session_key, salt = none, info = "polyphonia/v1/session-id")[..8]
```

- The sender gets `session_key` back from `encrypt`, the receiver from `decrypt`.
  Each stores it (Keychain in the app). Only the 32-byte key is stored; the id
  is recomputed from it.
- Distinct HKDF info strings keep the session key independent of key_A/key_B
  of the full message (domain separation).
- Sessions are **one-directional** (sender to recipient), see open question 1.
- The app decides when to start a new session (e.g. every N messages or days)
  simply by sending a full message again.

### Envelope and payload in SESSION mode

- Envelope (§2): flags bit 0 = 1. The algorithm byte stays the algorithm that
  established the session, so a session inherits its security (an
  RSA-established session is not post-quantum). Envelope version stays 1:
  SESSION was reserved in v1 and no file has used it.
- Payload (§3):

  ```
  payload = session_id (8) || slot_A || slot_B        (SESSION flag = 1)
  payload = kem_block      || slot_A || slot_B        (SESSION flag = 0, unchanged)
  ```

  `poly_core::payload::kem_block_len(algorithm, FLAG_SESSION)` returns
  `SESSION_ID_LEN = 8` instead of 0, so `payload_len` and `Payload::split`
  keep working. `Payload::kem_block` holds the session id in SESSION mode
  (field kept to limit churn).
- Slots: unchanged, except that **S is replaced by session_key** in the key
  derivation: `key_A = HKDF(session_key, info slot-a)`,
  `key_B = HKDF(session_key, salt = Argon2id(passphrase, contact_id), info slot-b)`.
  Hidden mode works as before.
- AAD: unchanged (header bytes 0..15, which include the flags byte). The
  session id is not in the AAD; a changed id selects a wrong or unknown key,
  so decryption fails anyway.

### Changes to the traits (§4, poly-core)

```rust
pub struct SealRequest {  /* existing fields */
    pub flags: u8,                       // NEW: envelope flags, also used for the AAD
    pub session_key: Option<Vec<u8>>,    // NEW: Some iff flags has SESSION
}
pub struct OpenRequest {  /* existing fields */
    pub flags: u8,                       // NEW: from the envelope
    pub session_key: Option<Vec<u8>>,    // NEW: Some iff flags has SESSION (engine picks it)
}
pub struct Sealed {                      // NEW: Cipher::seal returns this instead of Vec<u8>
    pub payload: Vec<u8>,
    pub new_session_key: Option<Vec<u8>>,  // Some for a full message from a real cipher
}
pub struct Opened {       /* existing fields */
    pub new_session_key: Option<Vec<u8>>,  // NEW: Some for a full message from a real cipher
}
pub fn session_id(session_key: &[u8]) -> [u8; 8];  // NEW helper in poly_core
```

Rules:
- SESSION flag set and `session_key` None (or the reverse) gives `InvalidRequest`.
- `session_key` not 32 bytes gives `InvalidKey`.
- In SESSION mode `seal` doesn't need the recipient public key and `open`
  doesn't need the secret key, but the engine still checks contact_id.
- FakeCipher supports SESSION (NOT SECURE) so carriers and golden vectors can
  test it without real crypto: its `new_session_key` = SHA-256(
  `"polyphonia/fake/session"` || public key), and slots work as today.

### Engine changes (§6, poly-engine)

- `EncryptRequest` gets `session_key: Option<Vec<u8>>`. Some means SESSION:
  flags = 1, no KEM, capacity computed with the 8-byte session id.
- `EncodedFile` gets `session_key: Option<Vec<u8>>`: the new session from a
  full message, for the app to store.
- `DecryptRequest` gets `session_keys: Vec<Vec<u8>>`: every session key the
  receiver has stored. The engine computes each key's `session_id` and uses
  the one matching the payload. Keeps the core stateless.
- `Decrypted` gets `session_key: Option<Vec<u8>>`: the new session from a full message.
- `Inspected` exposes `session_id: Option<[u8; 8]>` (no key needed).
- `MVP_FLAGS` goes away; flags come from the request or the envelope.

### Errors (§7)

- New `PolyError::UnknownSession`: a SESSION message whose id matches no
  stored key, e.g. the full message that started the session never arrived.
  The app says "ask them to send a full message first". Grouped with crypto
  errors; mirrored in poly-ffi.

### FFI and app (§8, role 4)

- Mirror the new fields and `UnknownSession`; export `sessionId(sessionKey:)`.
- App: store the sender-side session key per contact (the one to use when
  sending) and the receiver-side keys as a list; pass the list to `decrypt`.
  Offer "start a new session" (= send a full message) and expire old keys.
- CLI (§9): `--session <file>` on encrypt, `--session-out <file>` on encrypt
  and decrypt, `--sessions <dir>` on decrypt. Same file header style as keys
  (`polyphonia-session-key v1`, mode 0600).

### Security notes

- **No forward secrecy inside a session.** Whoever steals a stored session key
  can read every message of that session (but not the long-term key or other
  sessions). Rotating sessions limits the damage. A ratchet (new key per
  message) is future work: messengers drop and reorder files, so keeping two
  ratchets in sync is hard.
- **Key reuse is safe within limits.** All messages of a session share key_A
  (and key_B for one passphrase); random 12-byte nonces keep AES-GCM safe up
  to 2^32 slot encryptions per key (NIST SP 800-38D), far beyond any session.
- **Linkability.** The session id is plaintext, so an observer who extracts the
  envelope can tell messages belong to the same session. The contact_id already
  reveals the recipient, so this adds little; fixed by header encryption (§10).
- **Unauthenticated, as today.** Anyone with Bob's public key can start a
  session to Bob, just as they can send him a full message. Sessions don't add
  sender authentication.

### Tests

- `assert_cipher_roundtrip` extended: a full message returns the same
  `new_session_key` on seal and open; a SESSION message opens with it (normal and
  hidden mode); a wrong session key gives `DecryptFailed`; the flag/key mismatch
  gives `InvalidRequest`.
- Engine e2e: establish, then a SESSION message through every carrier; an unknown
  session gives `UnknownSession`; capacity uses 8 bytes, not the KEM block.
- A Fake golden vector `envelope_session.bin`.

### Open questions for the lead

1. **One-directional or both ways?** Bob could reuse Alice's session to reply,
   but the envelope doesn't say who sent a message, so Bob's app would need
   the user to say which contact a session belongs to. Proposal: one-directional
   for the hackathon.
2. **FakeCipher SESSION support** as above (for carrier tests and vectors)? Proposal: yes.
3. **Trait change** `seal -> Sealed` touches all three ciphers (all role 1). OK?
4. **Order of work.** Proposal: (a) poly-core types + payload length + helper +
   vectors, by role 1 under your review; (b) ciphers; (c) engine; (d) FFI and
   CLI flags, with role 4.
