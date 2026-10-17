# Role 1: crypto + lead

## Goal

Implement the real ciphers, ML-KEM-768 (post-quantum) and RSA-2048 (for
comparison), so the payload is genuinely encrypted, exactly as `docs/SPEC.md`
§3 describes. You also own the shared contract (`poly-core`, `poly-engine`,
`docs/SPEC.md`) and review everyone's proposals to change it.

## Owned files

- `crates/poly-crypto/` (your main work)
- `crates/poly-core/`, `crates/poly-engine/`, `docs/SPEC.md` (lead; already implemented)
- `tests/vectors/`

## The trait to implement

```rust
pub trait Cipher: Send + Sync {
    fn algorithm(&self) -> Algorithm;
    fn generate_identity(&self) -> Result<Identity, PolyError>;   // Identity { public_key: Vec<u8>, secret_key: Vec<u8> }
    /// Build the full payload (kem_block + both slots) for the recipient.
    fn seal(&self, req: &SealRequest) -> Result<Vec<u8>, PolyError>;
    /// Open a payload; returns the text and which slot opened.
    fn open(&self, req: &OpenRequest) -> Result<Opened, PolyError>;
}
SealRequest { recipient_public_key, contact_id: [u8; 8], message, decoy: Option<String>, passphrase: Option<String>, aad: [u8; 15] }
OpenRequest { my_secret_key, contact_id: [u8; 8], payload, passphrase: Option<String>, aad: [u8; 15] }
Opened { text: String, slot: Slot }   // enum Slot { A, B }
```

Rules:
- decoy Some and passphrase None → `MissingPassphrase`; passphrase Some and decoy None → `InvalidRequest`.
- Normal mode: slot A = message, slot B = random bytes of the same length.
- Hidden mode: slot A = decoy, slot B = message.
- Open: if a passphrase is given, try B; if its tag verifies return B, else A.
  **Always attempt both** decryptions (timing). Nothing opens → `DecryptFailed`.

## Format (from SPEC)

```
payload = kem_block || slot_A || slot_B        slot = nonce(12) || ciphertext || tag(16)
plaintext = u16 BE length || UTF-8 text || zeros, padded to a multiple of 16 (both slots same size)
AAD = envelope header bytes 0..15 (req.aad)
```

| | kem_block | S (32 bytes) |
|---|---|---|
| ML-KEM-768 | 1088-byte ciphertext from `Encapsulate(pk)` | the shared key |
| RSA-2048 | `RSA-OAEP-SHA256(pk, S)`, 256 bytes, MGF1-SHA256, empty label | 32 random bytes |

```
key_A = HKDF-SHA256(ikm = S, salt = none,                                info = "polyphonia/v1/slot-a")
key_B = HKDF-SHA256(ikm = S, salt = Argon2id(passphrase, salt = contact_id), info = "polyphonia/v1/slot-b")
slot  = nonce || AES-256-GCM(key, nonce, padded plaintext, aad)
```

Argon2id (proposed in SPEC; confirm or change it there): v0x13, m = 19,456 KiB,
t = 2, p = 1, 32-byte output. Key encodings are proposed in SPEC §3; confirm them.

Already available in `poly_core::payload`: `Payload::split/join`,
`slot_pad`, `slot_unpad`, `padded_len`, `slot_len`, `split_slot`, and the
constants `KEM_BLOCK_LEN_*`, `NONCE_LEN`, `TAG_LEN`, `HKDF_INFO_SLOT_A/B`.
`crates/poly-crypto/src/fake.rs` is a working reference for the structure (but NOT the crypto).

Dependencies are already in `Cargo.toml`: `ml-kem`, `rsa`, `aes-gcm`, `hkdf`,
`sha2`, `argon2`, `rand`. Check their versions' APIs; the RustCrypto crates
change between releases, and `rand` 0.10 renamed `RngCore` to `Rng`.

## Done criteria

Remove the `#[ignore]` from these and make them pass:

- `crates/poly-crypto/src/mlkem.rs`: `mlkem_roundtrip`, `mlkem_kem_block_is_1088_bytes`
- `crates/poly-crypto/src/rsa.rs`: `rsa_roundtrip`

They call `poly_core::test_utils::assert_cipher_roundtrip`, which checks normal
mode, hidden mode (right / wrong / no passphrase), equal slot sizes, request
validation, tampering, wrong AAD, wrong key and truncation.

Then `poly keygen --algo mlkem` and the README demo work with `--algo mlkem`.

## Test commands

```sh
cargo test -p poly-crypto -- --include-ignored
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Order of work

1. **MVP:** `MlKemCipher` per SPEC; remove its `#[ignore]`s.
2. `RsaCipher` (RSA-2048-OAEP-SHA256 wrapping the 32-byte S).
3. Write a golden vector for each real algorithm with a fixed seed/RNG, if the
   crates allow deterministic encapsulation (document it in `tests/vectors/README.md`).
4. **Pitch demo:** a `tiny_rsa` module: 64-bit RSA key, factored live with
   Pollard's rho, to show why classical crypto breaks and ML-KEM doesn't.
   Expose it through a CLI subcommand (e.g. `poly tiny-rsa`).
5. **Stretch:** SESSION mode (flag bit 0): a saved session key, no KEM block.
   This is what makes ML-KEM practical for music (the 1088-byte KEM block alone is ~3.6 min at 40 bit/s).

## Pitfalls

- Never reuse a nonce with the same key: random 12-byte nonce per slot.
- Use constant-time tag comparison (AES-GCM does this for you; don't compare tags by hand).
- Open must not return early after trying B; try A too, then decide.
- Normal-mode slot B must be indistinguishable from a real slot: random bytes of exactly `slot_len`.
- ML-KEM decapsulation never fails (implicit rejection): a tampered kem_block gives a
  different S, so slot A then fails with `DecryptFailed`. That's expected.
- Map every crate error to `DecryptFailed` / `InvalidKey` / `Crypto(..)`; never put key bytes in error text.
- Argon2id is slow by design; the engine checks carrier capacity before calling `seal`, so don't do extra work there.
- As lead, review every PR that touches `poly-core`, `poly-engine` or `docs/SPEC.md`,
  and regenerate vectors (`POLY_REGEN_VECTORS=1 cargo test -p poly-engine --test golden`) only on intended format changes.
