# Golden vectors

Produced with **FakeCipher + RawDebugCarrier** by `crates/poly-engine/tests/golden.rs`
and parsed by `crates/poly-core/tests/vectors.rs`. Both tests fail if the code
stops producing exactly these bytes, so a silent format change is caught.

Regenerate only when `docs/SPEC.md` changes, with the lead's approval:

```sh
POLY_REGEN_VECTORS=1 cargo test -p poly-engine --test golden
```

## Fixed key

| | value |
|---|---|
| fake secret key | 32 bytes of `0x11` |
| fake public key | SHA-256(`"polyphonia/fake/public-key"` ‖ secret) |
| contact_id | `abecdbe8cf6cfa0b` |

## `envelope_min.bin` (23 bytes): empty payload

| offset | bytes | field | value |
|---|---|---|---|
| 0 | `504f4c59` | magic | "POLY" |
| 4 | `01` | version | 1 |
| 5 | `ff` | algorithm | Fake |
| 6 | `00` | flags | none |
| 7 | `abecdbe8cf6cfa0b` | contact_id | |
| 15 | `00000000` | payload_len | 0 |
| 19 | `bda23cbc` | CRC-32 | |

## `envelope_hidden.bin` (143 bytes): hidden mode

Message `the real message`, decoy `a harmless decoy`, passphrase `correct horse`.
FakeCipher is deterministic in hidden mode (zero nonces, no random slot), so
this file is stable.

| offset | size | field | value |
|---|---|---|---|
| 0 | 15 | header | same as `envelope_min.bin` |
| 15 | 4 | payload_len | `00000078` = 120 |
| 19 | 0 | kem_block | Fake has none |
| 19 | 60 | slot A | nonce 12×`00` ‖ padded `"a harmless decoy"` (32 B) ‖ fake tag (16 B) |
| 79 | 60 | slot B | nonce 12×`00` ‖ padded `"the real message"` (32 B) ‖ SHA-256(passphrase)[..16] |
| 139 | 4 | CRC-32 | `13af963f` |

Padded slot plaintext = u16 length `0010` ‖ 16 bytes of text ‖ 14 zero bytes = 32 bytes.

```sh
poly inspect tests/vectors/envelope_hidden.bin
```

## `envelope_session.bin` (151 bytes): hidden-mode session message

Same key and texts as `envelope_hidden.bin`, sent as a session message
(SPEC §3 "Session mode"). FakeCipher's session key for a recipient is
SHA-256(`"polyphonia/fake/session"` ‖ public key), so this file is stable.

| offset | size | field | value |
|---|---|---|---|
| 0 | 15 | header | `504f4c59 01 ff 01 abecdbe8cf6cfa0b` (flags = SESSION) |
| 15 | 4 | payload_len | `00000080` = 128 = 8 + 2 × 60 |
| 19 | 8 | session_id | `d9d58587a62fb2f3` |
| 27 | 60 | slot A | as in `envelope_hidden.bin` (the tag covers the new header) |
| 87 | 60 | slot B | as in `envelope_hidden.bin` |
| 147 | 4 | CRC-32 | `ce1f4801` |

## ML-KEM-768 vectors

Produced with **MlKemCipher** by `crates/poly-crypto/tests/golden.rs`
(regenerate with `POLY_REGEN_VECTORS=1 cargo test -p poly-crypto --test golden`,
only when `docs/SPEC.md` changes, with the lead's approval).
`crates/poly-crypto/tests/spec_reference.rs` decodes both files with an
independent implementation of SPEC §2–§3, so they are checked against the
spec, not only against the code that wrote them.

| | value |
|---|---|
| secret key (seed d ‖ z) | 64 bytes of `0x22` |
| public key | ML-KEM-768 encapsulation key from that seed (1184 bytes) |
| contact_id | `35818cf22b7ec19f` |

### Deterministic RNG

ML-KEM encapsulation and the slots need randomness. The vectors use a fully
specified test RNG (NOT SECURE) instead of `rand`'s `StdRng`, whose algorithm
may change between versions:

```
stream = SHA-256(domain || 0u64 BE) || SHA-256(domain || 1u64 BE) || ...
```

Bytes are taken from the stream in this order: 32 bytes for ML-KEM
encapsulation (the message m of FIPS 203), 12 bytes for the slot A nonce, then
12 bytes for the slot B nonce (hidden mode) or `slot_len` bytes for the random
slot B (normal mode).

### `mlkem_envelope_hidden.bin` (1231 bytes): hidden mode

Message `the real message`, decoy `a harmless decoy`, passphrase `correct horse`,
RNG domain `polyphonia/test-rng/mlkem-hidden`.

| offset | size | field | value |
|---|---|---|---|
| 0 | 15 | header | `504f4c59 01 01 00 35818cf22b7ec19f` (POLY, v1, ML-KEM-768, no flags, contact_id) |
| 15 | 4 | payload_len | `000004b8` = 1208 = 1088 + 2 × 60 |
| 19 | 1088 | kem_block | ML-KEM-768 ciphertext |
| 1107 | 60 | slot A | nonce (12) ‖ AES-256-GCM(key_A, padded decoy, 32 B) ‖ tag (16) |
| 1167 | 60 | slot B | nonce (12) ‖ AES-256-GCM(key_B, padded message, 32 B) ‖ tag (16) |
| 1227 | 4 | CRC-32 | `449a8f1c` |

### `mlkem_envelope_normal.bin` (1199 bytes): normal mode

Message `lunch at noon?`, RNG domain `polyphonia/test-rng/mlkem-normal`.

| offset | size | field | value |
|---|---|---|---|
| 0 | 15 | header | same as above |
| 15 | 4 | payload_len | `00000498` = 1176 = 1088 + 2 × 44 |
| 19 | 1088 | kem_block | ML-KEM-768 ciphertext |
| 1107 | 44 | slot A | nonce (12) ‖ AES-256-GCM(key_A, padded message, 16 B) ‖ tag (16) |
| 1151 | 44 | slot B | 44 random bytes from the RNG |
| 1195 | 4 | CRC-32 | `bb7a809e` |

### `mlkem_envelope_session.bin` (151 bytes): hidden-mode session message

A session message in the session started by `mlkem_envelope_hidden.bin`
(session_key = HKDF-SHA256(S of that file, info `"polyphonia/v1/session-key"`)),
with the same texts and passphrase, RNG domain
`polyphonia/test-rng/mlkem-session` (12 bytes slot A nonce, then 12 bytes
slot B nonce). The same message as a full message is 1231 bytes.

| offset | size | field | value |
|---|---|---|---|
| 0 | 15 | header | `504f4c59 01 01 01 35818cf22b7ec19f` (flags = SESSION) |
| 15 | 4 | payload_len | `00000080` = 128 = 8 + 2 × 60 |
| 19 | 8 | session_id | `a1eeece999f2d0de` = HKDF-SHA256(session_key, info `"polyphonia/v1/session-id"`)[..8] |
| 27 | 60 | slot A | AES-256-GCM with key_A derived from session_key (padded decoy) |
| 87 | 60 | slot B | AES-256-GCM with key_B derived from session_key (padded message) |
| 147 | 4 | CRC-32 | `83363b19` |

## RSA-2048 vector

Produced with **RsaCipher** by `crates/poly-crypto/tests/golden.rs` and
decoded independently by `crates/poly-crypto/tests/spec_reference.rs`.

| | value |
|---|---|
| `rsa_public_key.der` | 270 bytes, PKCS#1 DER (2048-bit modulus) |
| `rsa_secret_key.der` | 1218 bytes, PKCS#8 DER |
| contact_id | `0c9c5eb5c8254b2d` |

The key files are the reference. They were generated once with the test RNG
(domain `polyphonia/test-rng/rsa-key`) and are only regenerated if missing,
because RSA key generation depends on the `rsa` crate's prime search. The
envelope uses the same test RNG (see "Deterministic RNG" above); bytes are
drawn in this order: 32 bytes for S, the OAEP seed (drawn by the `rsa`
crate), 12 bytes for the slot A nonce, 12 bytes for the slot B nonce. An
`rsa` crate upgrade that changes how OAEP draws its seed changes this file:
regenerate it then, with the lead's approval.

### `rsa_envelope_hidden.bin` (399 bytes): hidden mode

Message `the real message`, decoy `a harmless decoy`, passphrase `correct horse`,
RNG domain `polyphonia/test-rng/rsa-hidden`.

| offset | size | field | value |
|---|---|---|---|
| 0 | 15 | header | `504f4c59 01 02 00 0c9c5eb5c8254b2d` (POLY, v1, RSA-2048, no flags, contact_id) |
| 15 | 4 | payload_len | `00000178` = 376 = 256 + 2 × 60 |
| 19 | 256 | kem_block | RSA-OAEP-SHA256(public key, S) |
| 275 | 60 | slot A | nonce (12) ‖ AES-256-GCM(key_A, padded decoy, 32 B) ‖ tag (16) |
| 335 | 60 | slot B | nonce (12) ‖ AES-256-GCM(key_B, padded message, 32 B) ‖ tag (16) |
| 395 | 4 | CRC-32 | `74683d9d` |
